//! The editor's side of language servers: which buffer goes to which server,
//! keeping the server's copy current, and what to do with what it says.

use serde_json::{Value, json};
use std::path::Path;

use super::{Editor, Mode};
use crate::complete::{self, Completion};
use crate::lsp::{self, Client, Encoding, Event, Location, RawDiagnostic, Request, State, Suggestion};
use crate::syntax::language_for_path;
use crate::view::{Diagnostic, Lsp, Selection};

impl Editor {
    /// Once a frame: open any buffer not yet shown to a server, and send the
    /// text of any that changed. The whole text, not the edits - a server
    /// can never drift out of step that way, and one message per frame
    /// however many keys went into it is cheap next to what a server does
    /// with it.
    pub fn lsp_sync(&mut self) {
        for index in 0..self.views.len() {
            if self.views[index].lsp == Lsp::Untried {
                self.lsp_open(index);
            }
            let Lsp::Open { server, version, synced } = self.views[index].lsp else {
                continue;
            };
            let view = &mut self.views[index];
            let edits = view.edits();
            if edits == synced {
                continue;
            }
            let Some(path) = view.doc.path.clone() else {
                continue;
            };
            view.lsp = Lsp::Open { server, version: version + 1, synced: edits };
            let text = view.doc.text.to_string();
            self.servers[server].notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": lsp::uri(&path), "version": version + 1 },
                    "contentChanges": [{ "text": text }],
                }),
            );
        }
    }

    /// Find a server for a buffer, starting one if none is running for its
    /// project, and tell it the buffer is open.
    fn lsp_open(&mut self, index: usize) {
        let Some(jobs) = self.jobs.clone() else {
            // No run loop to hear the answers: the tests, which stay Untried.
            return;
        };
        self.views[index].lsp = Lsp::Without;
        if !self.lsp_enabled {
            return;
        }
        let Some(path) = self.views[index].doc.path.clone() else {
            return;
        };
        let Some(language) = language_for_path(Some(&path)).map(|config| config.name) else {
            return;
        };
        let Some(spec) = lsp::spec_for(language) else {
            return;
        };
        let absolute = absolute(&path);
        let root = lsp::root_for(spec, &absolute);

        let running = self
            .servers
            .iter()
            .position(|client| client.name == spec.name && client.root == root);
        let server = match running {
            Some(server) if self.servers[server].state == State::Exited => return,
            Some(server) => server,
            None => match Client::spawn(spec, root, self.servers.len(), jobs) {
                Ok(client) => {
                    self.servers.push(client);
                    self.servers.len() - 1
                }
                Err(err) => {
                    self.message = format!("{err:#}");
                    return;
                }
            },
        };

        let view = &mut self.views[index];
        view.lsp = Lsp::Open { server, version: 0, synced: view.edits() };
        let text = view.doc.text.to_string();
        self.servers[server].notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": lsp::uri(&absolute),
                    "languageId": language,
                    "version": 0,
                    "text": text,
                },
            }),
        );
    }

    /// After a write: servers that check on save - rust-analyzer running
    /// `cargo check` - do it now.
    pub(super) fn lsp_saved(&mut self) {
        self.lsp_sync();
        if let (Lsp::Open { server, .. }, Some(path)) = (self.view().lsp, self.view().doc.path.clone()) {
            self.servers[server].notify("textDocument/didSave", json!({ "textDocument": { "uri": lsp::uri(&path) } }));
        }
    }

    /// A buffer is closing: its server stops tracking it, and answers asked
    /// for by buffer index are no longer to be trusted.
    pub(super) fn lsp_closed(&mut self, index: usize) {
        for client in &mut self.servers {
            client.forget_definitions();
        }
        if let (Lsp::Open { server, .. }, Some(path)) = (self.views[index].lsp, self.views[index].doc.path.clone()) {
            self.servers[server].notify(
                "textDocument/didClose",
                json!({ "textDocument": { "uri": lsp::uri(&absolute(&path)) } }),
            );
        }
    }

    /// A message from server `server`, or `None` when it has gone away.
    pub fn lsp_message(&mut self, server: usize, message: Option<Value>) {
        let Some(client) = self.servers.get_mut(server) else {
            return;
        };
        let Some(message) = message else {
            client.exited();
            self.message = format!("{} stopped", client.name);
            for view in &mut self.views {
                if matches!(view.lsp, Lsp::Open { server: open, .. } if open == server) {
                    view.lsp = Lsp::Without;
                    view.diagnostics.clear();
                }
            }
            return;
        };
        let encoding = client.encoding;
        match client.handle(message) {
            Event::Nothing | Event::Ready => {}
            Event::Say(text) => self.message = text,
            Event::Diagnostics { path, diagnostics } => self.set_diagnostics(&path, diagnostics, encoding),
            Event::Definition { request, locations } => self.definition_answer(request, locations, encoding),
            Event::Completion { request, items } => self.completion_answer(request, items),
        }
    }

    fn set_diagnostics(&mut self, path: &Path, raw: Vec<RawDiagnostic>, encoding: Encoding) {
        let Some(view) = self
            .views
            .iter_mut()
            .find(|view| view.doc.path.as_deref().is_some_and(|open| absolute(open) == path))
        else {
            return;
        };
        let rope = &view.doc.text;
        let mut diagnostics: Vec<Diagnostic> = raw
            .into_iter()
            .map(|d| Diagnostic {
                start: lsp::from_position(rope, d.start.0, d.start.1, encoding),
                end: lsp::from_position(rope, d.end.0, d.end.1, encoding),
                severity: d.severity,
                message: d.message,
            })
            .collect();
        diagnostics.sort_by_key(|d| (d.start, d.severity));
        view.diagnostics = diagnostics;
    }

    /// `gd` with a server: ask it, and say so. `false` when there is no
    /// server that can answer, for the tree-sitter lookup to have a go.
    pub(super) fn lsp_definition(&mut self) -> bool {
        if !self.lsp_enabled {
            return false;
        }
        self.lsp_sync();
        let view = &self.views[self.current];
        let (Lsp::Open { server, .. }, Some(path)) = (view.lsp, view.doc.path.as_deref()) else {
            return false;
        };
        let client = &mut self.servers[server];
        let (line, character) = lsp::to_position(&view.doc.text, view.sel.head, client.encoding);
        let params = json!({
            "textDocument": { "uri": lsp::uri(path) },
            "position": { "line": line, "character": character },
        });
        let request = Request::Definition { view: self.current, head: view.sel.head, edits: view.edits() };
        if !client.request("textDocument/definition", "definitionProvider", params, request) {
            return false;
        }
        self.message = format!("asking {}", client.name);
        true
    }

    /// Ask the server what could finish the word starting at `start`. The
    /// popup does not wait for the answer: the buffer's own words are there
    /// at once, and what the server sends is folded in when it arrives, which
    /// is usually within a keystroke or two.
    ///
    /// Asked once per popup rather than once per keystroke. The list a server
    /// returns is for the position, not for the prefix, and typing more of the
    /// word only narrows what is already here - the same reason the buffer's
    /// candidates are gathered once.
    pub(super) fn lsp_complete(&mut self, start: usize, trigger: Option<char>) {
        if !self.lsp_enabled {
            return;
        }
        self.lsp_sync();
        let view = &self.views[self.current];
        let (Lsp::Open { server, .. }, Some(path)) = (view.lsp, view.doc.path.as_deref()) else {
            return;
        };
        let client = &mut self.servers[server];
        let (line, character) = lsp::to_position(&view.doc.text, view.sel.head, client.encoding);
        // The context says why: a server answers a `.` with what is on the
        // thing before it, and a bare ask with everything in scope.
        let context = match trigger {
            Some(c) => json!({ "triggerKind": 2, "triggerCharacter": c.to_string() }),
            None => json!({ "triggerKind": 1 }),
        };
        let params = json!({
            "textDocument": { "uri": lsp::uri(path) },
            "position": { "line": line, "character": character },
            "context": context,
        });
        let request = Request::Completion { view: self.current, start };
        client.request("textDocument/completion", "completionProvider", params, request);
    }

    /// The characters this buffer's server wants to be asked after: `.` for
    /// Python, where there is no word yet but plenty to offer.
    pub(super) fn completion_triggers(&self) -> Vec<char> {
        if !self.lsp_enabled {
            return Vec::new();
        }
        let Lsp::Open { server, .. } = self.views[self.current].lsp else {
            return Vec::new();
        };
        match self.servers.get(server) {
            Some(client) => client.completion_triggers(),
            None => Vec::new(),
        }
    }

    /// What the server offered, folded into the popup that asked - or opening
    /// one, when the question was asked at a `.` where the buffer itself had
    /// nothing to offer.
    fn completion_answer(&mut self, request: Request, items: Vec<Suggestion>) {
        let Request::Completion { view, start } = request else {
            return;
        };
        // A different buffer, or a word since abandoned: nobody is waiting.
        if self.current != view || self.mode != Mode::Insert || items.is_empty() {
            return;
        }
        let at = self.view().sel.head;
        // Typed something that is not part of a word since asking - a space,
        // a bracket - and the word the answer is about is over.
        let typed = self.view().doc.slice_str(start, at);
        if at < start || !typed.chars().all(complete::is_word) {
            return;
        }
        match self.completion.as_mut() {
            Some(completion) if completion.start == start => completion.extend(items),
            // The popup was closed, or is over a different word now.
            Some(_) => {}
            None => self.completion = Completion::from_server(start, items),
        }
    }

    fn definition_answer(&mut self, request: Request, locations: Vec<Location>, encoding: Encoding) {
        let Request::Definition { view, head, edits } = request else {
            return;
        };
        // Moved, typed or switched buffer since asking: the answer is to a
        // question nobody is waiting on any more.
        if self.current != view || self.view().sel.head != head || self.view().edits() != edits {
            return;
        }
        self.message.clear();
        let Some(location) = locations.first() else {
            // The server does not know - still indexing, or a name it cannot
            // resolve. What the tree can see is better than nothing.
            self.goto_definition_in_tree(true);
            return;
        };
        self.push_jump();
        let here = self.view().doc.path.as_deref().map(absolute);
        if here.as_deref() != Some(location.path.as_path())
            && let Err(err) = self.open_file(&location.path)
        {
            self.message = format!("{err:#}");
            return;
        }
        let (line, character) = location.position;
        let at = lsp::from_position(&self.view().doc.text, line, character, encoding);
        self.view_mut().sel = Selection::point(at);
        self.clamp_cursor();
        if locations.len() > 1 {
            self.message = format!("1 of {} definitions", locations.len());
        }
    }

    /// `]d` and `[d`: the next diagnostic after the cursor, or the one before,
    /// round the end of the buffer, and what it says.
    pub fn goto_diagnostic(&mut self, forward: bool) {
        let head = self.view().sel.head;
        let diagnostics = &self.view().diagnostics;
        let found = match forward {
            true => diagnostics.iter().find(|d| d.start > head).or(diagnostics.first()),
            false => diagnostics.iter().rev().find(|d| d.start < head).or(diagnostics.last()),
        };
        let Some(diagnostic) = found else {
            self.message = "no diagnostics".into();
            return;
        };
        let (start, severity) = (diagnostic.start, diagnostic.severity);
        let text = diagnostic.message.lines().next().unwrap_or("").to_string();
        self.view_mut().sel = Selection::point(start);
        self.clamp_cursor();
        self.message = format!("{}: {text}", severity.name());
    }

    /// What the current buffer's server says it is busy with.
    pub fn lsp_busy(&self) -> Option<String> {
        match self.view().lsp {
            Lsp::Open { server, .. } => self.servers[server].busy(),
            _ => None,
        }
    }

    /// `:lsp`: which servers are running, and what this buffer has.
    pub(super) fn lsp_report(&mut self) {
        let this = match self.view().lsp {
            Lsp::Open { server, .. } => format!("this buffer: {}", self.servers[server].name),
            Lsp::Without if !self.lsp_enabled => "language servers are off (:set lsp)".into(),
            Lsp::Without | Lsp::Untried => "no language server for this buffer".into(),
        };
        let servers: Vec<String> = self
            .servers
            .iter()
            .map(|client| {
                let state = match client.state {
                    State::Starting => "starting",
                    State::Ready => "ready",
                    State::Exited => "stopped",
                };
                format!("{} {state} in {}", client.name, client.root.display())
            })
            .collect();
        self.message = match servers.is_empty() {
            true => this,
            false => format!("{this}; {}", servers.join(", ")),
        };
    }

    /// `:set lsp` again after `:set nolsp`: buffers passed over while it was
    /// off get looked at again.
    pub(super) fn lsp_turned_on(&mut self) {
        for view in &mut self.views {
            if view.lsp == Lsp::Without {
                view.lsp = Lsp::Untried;
            }
        }
    }
}

/// A path as a server names it: absolute, with links resolved where the file
/// exists.
fn absolute(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| match std::env::current_dir() {
        Ok(dir) => dir.join(path),
        Err(_) => path.to_path_buf(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Severity;
    use crate::lsp::tests::sent;

    /// A scratch editor on a file at `path` - which need not exist - whose
    /// buffer is open in a ready server that answers nothing by itself.
    fn editor_with_server(path: &str, text: &str) -> (Editor, std::sync::mpsc::Receiver<Vec<u8>>) {
        let mut editor = Editor::scratch();
        let view = editor.view_mut();
        view.doc.text = ropey::Rope::from_str(text);
        view.doc.path = Some(path.into());
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({ "definitionProvider": true }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        (editor, written)
    }

    fn raw(start: (u32, u32), end: (u32, u32), severity: Severity, message: &str) -> RawDiagnostic {
        RawDiagnostic { start, end, severity, message: message.into() }
    }

    #[test]
    fn diagnostics_land_on_their_text_and_stay_on_it_through_edits() {
        let (mut editor, _written) = editor_with_server("/nowhere/a.rs", "let x = y;\nlet z = 1;\n");
        let path = absolute(Path::new("/nowhere/a.rs"));
        editor.set_diagnostics(
            &path,
            vec![
                raw((1, 4), (1, 5), Severity::Warning, "unused z"),
                raw((0, 8), (0, 9), Severity::Error, "no y\nhelp: define it"),
            ],
            Encoding::Utf16,
        );
        let starts: Vec<usize> = editor.view().diagnostics.iter().map(|d| d.start).collect();
        assert_eq!(starts, [8, 15], "in char offsets, in order");

        // A line typed above moves them both down with their text.
        editor.view_mut().edit_at(0, 0, "// new\n", None);
        let text = editor.view().doc.text.to_string();
        let covered: Vec<&str> = editor.view().diagnostics.iter().map(|d| &text[d.start..d.end]).collect();
        assert_eq!(covered, ["y", "z"]);
        assert_eq!(editor.view().edits(), 1);
    }

    #[test]
    fn bracket_d_walks_the_diagnostics_round_the_buffer() {
        let (mut editor, _written) = editor_with_server("/nowhere/a.rs", "one\ntwo\nthree\n");
        let path = absolute(Path::new("/nowhere/a.rs"));
        editor.set_diagnostics(
            &path,
            vec![raw((1, 0), (1, 3), Severity::Error, "bad two\nmore"), raw((2, 2), (2, 5), Severity::Hint, "ree")],
            Encoding::Utf16,
        );
        editor.goto_diagnostic(true);
        assert_eq!(editor.cursor_coords(), (1, 0));
        assert_eq!(editor.message, "error: bad two", "the first line of it");
        editor.goto_diagnostic(true);
        assert_eq!(editor.cursor_coords(), (2, 2));
        editor.goto_diagnostic(true);
        assert_eq!(editor.cursor_coords(), (1, 0), "round the end");
        editor.goto_diagnostic(false);
        assert_eq!(editor.cursor_coords(), (2, 2), "and back round the start");

        editor.view_mut().diagnostics.clear();
        editor.goto_diagnostic(true);
        assert_eq!(editor.message, "no diagnostics");
    }

    /// The same, with a server that completes as well as defines.
    fn editor_completing(path: &str, text: &str) -> (Editor, std::sync::mpsc::Receiver<Vec<u8>>) {
        let mut editor = Editor::scratch();
        let view = editor.view_mut();
        view.doc.text = ropey::Rope::from_str(text);
        view.doc.path = Some(path.into());
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({
            "completionProvider": { "triggerCharacters": ["."] },
        }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        editor.set_mode(Mode::Insert);
        (editor, written)
    }

    fn answer(id: &Value, items: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": id, "result": items })
    }

    #[test]
    fn the_server_is_asked_what_the_buffer_cannot_know() {
        let (mut editor, written) = editor_completing("/nowhere/a.py", "value = 1
va");
        editor.view_mut().sel = Selection::point(12);
        editor.open_completion(false);

        // The buffer answers at once, out of its own words.
        assert_eq!(editor.completion.as_ref().map(|c| c.len()), Some(1));

        let asked = sent(&written);
        let request = asked.iter().find(|m| m["method"] == "textDocument/completion").expect("a request");
        assert_eq!(request["params"]["position"], json!({ "line": 1, "character": 2 }));

        // And the server's answer joins it, in front, with its kinds.
        editor.lsp_message(0, Some(answer(&request["id"], json!([
            { "label": "validate", "kind": 3, "sortText": "a" },
            { "label": "value", "kind": 6, "sortText": "b" },
        ]))));
        let completion = editor.completion.as_ref().expect("still open");
        let items: Vec<(&str, Option<&str>)> =
            completion.items().map(|c| (c.text.as_str(), c.kind)).collect();
        assert_eq!(items, [("validate", Some("fn")), ("value", Some("var"))]);
        assert_eq!(items.len(), 2, "the buffer's own `value` gave way to the server's");
    }

    #[test]
    fn a_dot_asks_the_server_and_opens_on_what_it_says() {
        let (mut editor, written) = editor_completing("/nowhere/a.py", "self.");
        editor.view_mut().sel = Selection::point(5);
        editor.suggest_from_server('.');
        assert!(editor.completion.is_none(), "nothing to show until it answers");

        let asked = sent(&written);
        let request = asked.iter().find(|m| m["method"] == "textDocument/completion").expect("a request");
        editor.lsp_message(0, Some(answer(&request["id"], json!({
            "isIncomplete": false,
            "items": [
                { "label": "competitions", "kind": 5, "sortText": "02" },
                { "label": "save(self)", "kind": 2, "sortText": "01", "insertTextFormat": 2 },
            ],
        }))));
        let completion = editor.completion.as_ref().expect("a popup");
        let items: Vec<&str> = completion.items().map(|c| c.text.as_str()).collect();
        // `sortText` is the order the server meant, and a snippet label is cut
        // back to the name rather than pasted in with its brackets.
        assert_eq!(items, ["save", "competitions"]);

        // A character that is not a trigger asks nothing.
        let (mut editor, written) = editor_completing("/nowhere/a.py", "x,");
        editor.view_mut().sel = Selection::point(2);
        editor.suggest_from_server(',');
        assert!(sent(&written).iter().all(|m| m["method"] != "textDocument/completion"));
    }

    #[test]
    fn an_answer_about_a_word_that_is_over_is_dropped() {
        let (mut editor, written) = editor_completing("/nowhere/a.py", "va");
        editor.view_mut().sel = Selection::point(2);
        editor.open_completion(false);
        let asked = sent(&written);
        let request = asked.iter().find(|m| m["method"] == "textDocument/completion").expect("a request");

        // Typed past the word since asking - a space ends it.
        editor.view_mut().doc.text = ropey::Rope::from_str("va = ");
        editor.view_mut().sel = Selection::point(5);
        editor.lsp_message(0, Some(answer(&request["id"], json!([{ "label": "validate" }]))));
        assert!(editor.completion.is_none(), "nobody is waiting for that");
    }

    #[test]
    fn gd_asks_the_server_and_goes_where_it_answers() {
        let (mut editor, written) = editor_with_server("/nowhere/a.rs", "fn f() {}\nfn g() { f(); }\n");
        editor.view_mut().sel = Selection::point(19);
        editor.goto_definition(true);
        let asked = sent(&written);
        let request = asked.iter().find(|m| m["method"] == "textDocument/definition").expect("a request");
        assert_eq!(request["params"]["position"], json!({ "line": 1, "character": 9 }));
        assert_eq!(editor.message, "asking fake");

        let answer = |id: &Value, line: u32, character: u32| {
            json!({ "jsonrpc": "2.0", "id": id, "result": [{
                "uri": lsp::uri(Path::new("/nowhere/a.rs")),
                "range": { "start": { "line": line, "character": character }, "end": { "line": line, "character": character + 1 } },
            }]})
        };
        editor.lsp_message(0, Some(answer(&request["id"], 0, 3)));
        assert_eq!(editor.cursor_coords(), (0, 3));
        assert_eq!(editor.message, "");
        editor.jump_back();
        assert_eq!(editor.view().sel.head, 19, "and it is a jump, for ^o");
    }

    #[test]
    fn an_answer_to_a_question_nobody_is_waiting_on_is_dropped() {
        let (mut editor, written) = editor_with_server("/nowhere/a.rs", "fn f() {}\nfn g() { f(); }\n");
        editor.view_mut().sel = Selection::point(19);
        editor.goto_definition(true);
        let id = sent(&written).into_iter().find(|m| m["method"] == "textDocument/definition").unwrap()["id"].clone();

        // Moved on before the answer came.
        editor.view_mut().sel = Selection::point(12);
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [{
            "uri": lsp::uri(Path::new("/nowhere/a.rs")),
            "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 4 } },
        }]})));
        assert_eq!(editor.view().sel.head, 12);
    }

    #[test]
    fn a_server_that_does_not_know_leaves_it_to_the_tree() {
        let (mut editor, written) = editor_with_server("/nowhere/a.rs", "fn f() {}\nfn g() { f(); }\n");
        editor.view_mut().attach_syntax(&crate::theme::Theme::built_in());
        editor.view_mut().sel = Selection::point(19);
        editor.goto_definition(true);
        let id = sent(&written).into_iter().find(|m| m["method"] == "textDocument/definition").unwrap()["id"].clone();
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": null })));
        assert_eq!(editor.cursor_coords(), (0, 3), "tree-sitter found it in the file");
    }

    #[test]
    fn a_server_that_stops_takes_its_diagnostics_with_it() {
        let (mut editor, _written) = editor_with_server("/nowhere/a.rs", "x\n");
        let path = absolute(Path::new("/nowhere/a.rs"));
        editor.set_diagnostics(&path, vec![raw((0, 0), (0, 1), Severity::Error, "x")], Encoding::Utf16);
        editor.lsp_message(0, None);
        assert!(editor.view().diagnostics.is_empty());
        assert_eq!(editor.view().lsp, Lsp::Without);
        assert_eq!(editor.message, "fake stopped");
        // And `gd` goes back to the tree rather than asking a dead server.
        editor.goto_definition(true);
        assert_ne!(editor.message, "asking fake");
    }
}

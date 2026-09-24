//! The editor's side of language servers: which buffer goes to which server,
//! keeping the server's copy current, and what to do with what it says.

use serde_json::{Value, json};
use std::path::Path;

use super::{Editor, Mode};
use crate::complete::{self, Completion};
use crate::info::{self, Info};
use crate::lsp::{
    self, Client, CodeAction, Encoding, Event, Location, RawDiagnostic, Request, Signature, State,
    RawHint, Suggestion, Symbol, TextEdit, WorkspaceEdit,
};

/// The most project symbols put in the picker at once. A server asked about
/// one letter can answer with thousands, and the one wanted is near the top.
const SYMBOLS_SHOWN: usize = 200;

/// How long to wait before asking again for hints a server refused.
const HINTS_RETRY: std::time::Duration = std::time::Duration::from_secs(1);
use crate::picker::{Item, Picker, Source};
use crate::syntax::language_for_path;
use crate::view::{Diagnostic, Hint, Lsp, Selection};

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
            // The changes themselves when the server takes them and every
            // edit since it was last told is in the log; the whole text when
            // not. A keystroke in a long file is then a few bytes rather than
            // the file, and the server does not re-read what did not change.
            let (from, changes) = view.take_sync_log();
            let client = &self.servers[server];
            let content = match from == synced && !changes.is_empty() && client.incremental() {
                true => {
                    let place = |place: &crate::view::Place| {
                        let character = match client.encoding {
                            Encoding::Utf8 => place.utf8,
                            Encoding::Utf16 => place.utf16,
                        };
                        json!({ "line": place.line, "character": character })
                    };
                    changes
                        .iter()
                        .map(|change| json!({
                            "range": { "start": place(&change.start), "end": place(&change.end) },
                            "text": change.text,
                        }))
                        .collect()
                }
                false => vec![json!({ "text": view.doc.text.to_string() })],
            };
            self.servers[server].notify(
                "textDocument/didChange",
                json!({
                    "textDocument": { "uri": lsp::uri(&path), "version": version + 1 },
                    "contentChanges": content,
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
        // What the server is about to be given is the whole text: nothing
        // before it needs telling.
        view.take_sync_log();
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
            client.forget_about_buffers();
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
            Event::Hover { request, markup } => self.hover_answer(request, markup),
            Event::Signature { request, help } => self.signature_answer(request, help),
            Event::Format { request, edits } => self.format_answer(request, edits, encoding),
            Event::References { request, locations } => self.references_answer(request, locations),
            Event::Rename { request, edit } => self.rename_answer(request, edit, encoding),
            Event::CodeActions { request, actions } => self.code_actions_answer(request, actions),
            Event::ResolvedAction { request, action } => self.resolved_action(request, action),
            Event::InlayHints { request, hints } => self.hints_answer(request, hints, encoding),
            Event::HintsFailed { request: Request::InlayHint { view, .. } } => {
                if let Some(view) = self.views.get_mut(view) {
                    view.hints_failed = Some(std::time::Instant::now());
                }
            }
            Event::HintsFailed { .. } => {}
            Event::HintsStale => {
                for view in &mut self.views {
                    view.hints_asked = None;
                }
            }
            Event::WorkspaceSymbols { request, symbols } => self.workspace_symbols_answer(request, symbols),
            Event::ApplyEdit { id, edit } => self.apply_edit_request(server, id, edit, encoding),
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
                raw: d.raw,
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
        self.client().map(Client::completion_triggers).unwrap_or_default()
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

    /// `K`: ask the server what the thing under the cursor is. `false` when
    /// there is no server to ask, which is all the caller needs to know to say
    /// so.
    pub(super) fn lsp_hover(&mut self) -> bool {
        let Some((server, params, head, edits)) = self.at_cursor() else {
            return false;
        };
        let request = Request::Hover { view: self.current, head, edits };
        self.servers[server].request("textDocument/hover", "hoverProvider", params, request)
    }

    /// The call being typed: ask what it takes. Nothing is said when there is
    /// no server - a signature is offered, not asked for, and a buffer without
    /// one should not be reporting that on every bracket.
    pub(super) fn lsp_signature(&mut self, trigger: Option<char>) {
        let Some((server, mut params, head, _)) = self.at_cursor() else {
            return;
        };
        // Why it is being asked: a server answers a fresh `(` differently from
        // a `,` inside a call it has already described.
        params["context"] = match trigger {
            Some(c) => json!({ "triggerKind": 2, "triggerCharacter": c.to_string(), "isRetrigger": false }),
            None => json!({ "triggerKind": 1, "isRetrigger": false }),
        };
        let request = Request::Signature { view: self.current, head };
        self.servers[server].request("textDocument/signatureHelp", "signatureHelpProvider", params, request);
    }

    /// The characters this buffer's server wants to be shown a signature for:
    /// `(` and `,` nearly everywhere, and whatever else a language brackets
    /// its arguments with.
    pub(super) fn signature_triggers(&self) -> Vec<char> {
        self.client().map(Client::signature_triggers).unwrap_or_default()
    }

    /// The server this buffer is open in, when there is one and servers are
    /// on at all. What every "can it answer this?" question starts with.
    fn client(&self) -> Option<&Client> {
        if !self.lsp_enabled {
            return None;
        }
        match self.views[self.current].lsp {
            Lsp::Open { server, .. } => self.servers.get(server),
            _ => None,
        }
    }

    /// The server, and a `textDocument`/`position` for the cursor, once the
    /// buffer is in step with it. What every question about the place the
    /// cursor is in starts with.
    fn at_cursor(&mut self) -> Option<(usize, Value, usize, u64)> {
        if !self.lsp_enabled {
            return None;
        }
        self.lsp_sync();
        let view = &self.views[self.current];
        let (Lsp::Open { server, .. }, Some(path)) = (view.lsp, view.doc.path.as_deref()) else {
            return None;
        };
        let (head, edits) = (view.sel.head, view.edits());
        let (line, character) = lsp::to_position(&view.doc.text, head, self.servers[server].encoding);
        let params = json!({
            "textDocument": { "uri": lsp::uri(path) },
            "position": { "line": line, "character": character },
        });
        Some((server, params, head, edits))
    }

    /// What the server said about the thing under the cursor, in a box beside
    /// it - or a word in the status line when it said nothing, because `K` was
    /// asked for and silence is not an answer.
    fn hover_answer(&mut self, request: Request, markup: Option<String>) {
        let Request::Hover { view, head, edits } = request else {
            return;
        };
        if !self.still_asking(view, head, edits) {
            return;
        }
        self.message.clear();
        match markup.as_deref().and_then(|markup| Info::hover(markup, head)) {
            Some(info) => self.info = Some(info),
            None => self.message = "nothing known about that".into(),
        }
    }

    /// The signature of the call being typed. Unlike `K` this was not asked
    /// for, so a server with nothing to say about the bracket you just typed
    /// says nothing: the box simply does not appear.
    fn signature_answer(&mut self, request: Request, help: Option<Signature>) {
        let Request::Signature { view, head } = request else {
            return;
        };
        // Still typing the call it is about. Not the same position and not the
        // same text - both have moved on by the time an answer lands, because
        // typing an argument is what you were doing when you asked - but the
        // same buffer, still in insert mode, and still inside the call.
        if self.current != view || self.mode != Mode::Insert || self.view().sel.head < head {
            return;
        }
        let Some(help) = help else {
            // The call is over - a `)` is a character servers ask to hear
            // about too - and a signature for a call that has been closed is
            // a box in the way.
            if self.info.as_ref().is_some_and(|info| info.kind == info::Kind::Signature) {
                self.info = None;
            }
            return;
        };
        // Which overload, when there is more than one, so a wrong-looking
        // signature is recognisable as one of several rather than the answer.
        let label = match help.count > 1 {
            true => format!("{} ({}/{})", help.label, help.index + 1, help.count),
            false => help.label.clone(),
        };
        self.info = Info::signature(&label, help.active, head);
    }

    /// Whether the answer to a question about a place is still about where the
    /// cursor is: same buffer, same spot, nothing typed since.
    fn still_asking(&self, view: usize, head: usize, edits: u64) -> bool {
        self.current == view && self.view().sel.head == head && self.view().edits() == edits
    }

    /// `:fmt`: hand the buffer to whatever the server formats with - rustfmt,
    /// gofmt, black. `lines` is the range a `:'<,'>fmt` named, in which case it
    /// is the selection that goes rather than the file.
    ///
    /// `false` when there is nothing to ask, or nothing that can answer: the
    /// caller says so, since this was asked for out loud.
    pub(super) fn lsp_format(&mut self, lines: Option<(usize, usize)>) -> bool {
        let Some((server, mut params, _, edits)) = self.at_cursor() else {
            return false;
        };
        // What the buffer indents with, which is what the server formats to.
        // Its own configuration usually wins - rustfmt has a `rustfmt.toml` -
        // and these are what it falls back on.
        let indent = self.indent();
        params["options"] = json!({
            "tabSize": indent.width,
            "insertSpaces": !indent.tabs,
            "trimTrailingWhitespace": true,
            "insertFinalNewline": true,
        });
        // A position means nothing to a whole-file format; a range is the
        // lines, whole, because formatting half a line is not a thing to ask.
        params.as_object_mut().expect("an object").remove("position");

        let (method, capability) = match lines {
            None => ("textDocument/formatting", "documentFormattingProvider"),
            Some((first, last)) => {
                let doc = &self.views[self.current].doc;
                let start = doc.line_to_char(first);
                // To the start of the line after, so the last line goes over
                // whole - or to the end of the file, when there is no line
                // after it because the file does not end in a newline.
                let end = match last + 1 < doc.len_lines() {
                    true => doc.line_to_char(last + 1),
                    false => doc.text.len_chars(),
                };
                let encoding = self.servers[server].encoding;
                let (from, to) = (
                    lsp::to_position(&doc.text, start, encoding),
                    lsp::to_position(&doc.text, end, encoding),
                );
                params["range"] = json!({
                    "start": { "line": from.0, "character": from.1 },
                    "end": { "line": to.0, "character": to.1 },
                });
                ("textDocument/rangeFormatting", "documentRangeFormattingProvider")
            }
        };
        let request = Request::Format { view: self.current, edits };
        self.servers[server].request(method, capability, params, request)
    }

    /// The formatting the server asks for, applied as one undo step.
    ///
    /// Back to front: every edit's position is in the text as the server saw
    /// it, so applying the last one first means the ones still to come are
    /// still where they said they were. The cursor is put back on the line and
    /// column it was on, which after a reformat is the nearest thing to where
    /// you were.
    fn format_answer(&mut self, request: Request, edits: Vec<TextEdit>, encoding: Encoding) {
        let Request::Format { view, edits: asked } = request else {
            return;
        };
        if self.current != view {
            return;
        }
        // Typed since asking: the positions are about text that no longer
        // exists, and applying them would scramble the file.
        if self.view().edits() != asked {
            self.message = "the buffer changed while it was being formatted".into();
            return;
        }
        if edits.is_empty() {
            self.message = "already formatted".into();
            return;
        }

        let mut edits = edits;
        edits.sort_by_key(|edit| edit.start);
        let (line, column) = self.view().cursor_coords();

        self.begin_undo_group();
        for edit in edits.iter().rev() {
            let doc = &self.view().doc;
            let start = lsp::from_position(&doc.text, edit.start.0, edit.start.1, encoding);
            let end = lsp::from_position(&doc.text, edit.end.0, edit.end.1, encoding);
            self.view_mut().edit_at(start, end.saturating_sub(start), &edit.text, None);
        }
        self.end_undo_group();

        // The line and column it was on: after a reformat that is the nearest
        // thing there is to where you were.
        let doc = &self.views[self.current].doc;
        let line = line.min(doc.len_lines().saturating_sub(1));
        let at = doc.line_to_char(line) + column.min(doc.line_len_chars(line).saturating_sub(1));
        self.view_mut().sel = Selection::point(at);
        self.clamp_cursor();
        self.message = match edits.len() {
            1 => "formatted".into(),
            many => format!("formatted: {many} changes"),
        };
    }

    /// `ga`: what the server offers to do about where the cursor is - a fix
    /// for the diagnostic under it, an import to add, a refactor over the
    /// selection. `false` when there is no server to ask.
    pub(super) fn lsp_code_actions(&mut self) -> bool {
        let Some((server, mut params, head, edits)) = self.at_cursor() else {
            return false;
        };
        let view = &self.views[self.current];
        let encoding = self.servers[server].encoding;
        // A selection is the range to act on; without one it is the cursor,
        // which is a range of no width. Servers offer refactors over a
        // selection and fixes at a point, and this is the difference.
        let (from, to) = view.sel.range();
        let (start, end) = (
            lsp::to_position(&view.doc.text, from, encoding),
            lsp::to_position(&view.doc.text, to, encoding),
        );
        // The diagnostics the range touches, as the server sent them: a fix is
        // offered for a diagnostic the server recognises as its own.
        let diagnostics: Vec<Value> = view
            .diagnostics
            .iter()
            .filter(|d| d.start <= to && d.end >= from && !d.raw.is_null())
            .map(|d| d.raw.clone())
            .collect();
        params.as_object_mut().expect("an object").remove("position");
        params["range"] = json!({
            "start": { "line": start.0, "character": start.1 },
            "end": { "line": end.0, "character": end.1 },
        });
        params["context"] = json!({ "diagnostics": diagnostics });

        let request = Request::CodeAction { view: self.current, head, edits };
        if !self.servers[server].request("textDocument/codeAction", "codeActionProvider", params, request) {
            return false;
        }
        self.message = format!("asking {}", self.servers[server].name);
        true
    }

    /// What the server offered, as a picker of titles. Nothing offered is
    /// said out loud: `ga` was asked for, and an empty list that closes itself
    /// looks like a key that did nothing.
    fn code_actions_answer(&mut self, request: Request, actions: Vec<CodeAction>) {
        let Request::CodeAction { view, head, edits } = request else {
            return;
        };
        if !self.still_asking(view, head, edits) {
            return;
        }
        self.message.clear();
        if actions.is_empty() {
            self.message = "nothing to do here".into();
            return;
        }
        let Lsp::Open { server, .. } = self.views[self.current].lsp else {
            return;
        };
        let items = actions
            .iter()
            .enumerate()
            .map(|(index, action)| Item {
                text: action.title.clone(),
                detail: String::new(),
                id: index,
                target: String::new(),
            })
            .collect();
        self.actions = Some((server, actions));
        self.open_picker(Picker::new(Source::Actions, items));
    }

    /// One of them, chosen. An action that came with its edits is applied
    /// here; one that came as a title and a promise is asked about again, and
    /// one that is a command is handed back to the server to run - which is
    /// how an action that has to look at more than one file works.
    pub(super) fn run_code_action(&mut self, index: usize) {
        let Some((server, actions)) = self.actions.take() else {
            return;
        };
        let Some(action) = actions.into_iter().nth(index) else {
            return;
        };
        if action.needs_resolving() {
            let edits = self.view().edits();
            let request = Request::ResolveAction { view: self.current, edits };
            let asked = self.servers[server].request("codeAction/resolve", "codeActionProvider", action.raw, request);
            if !asked {
                self.message = format!("{} would not say what that does", self.servers[server].name);
            }
            return;
        }
        self.do_action(server, action);
    }

    /// An action with everything it needs: the edits, applied, and the
    /// command, handed to the server. Both, where there are both - the
    /// protocol allows it, and means the edits first.
    fn do_action(&mut self, server: usize, action: CodeAction) {
        let encoding = self.servers[server].encoding;
        if let Some(edit) = action.edit {
            match self.apply_workspace_edit(edit, encoding) {
                Ok((places, _)) => self.message = format!("{}: {}", action.title, changes(places)),
                Err(err) => {
                    self.message = format!("{err:#}");
                    return;
                }
            }
        }
        if let Some(command) = action.command {
            let params = json!({
                "command": command["command"],
                "arguments": command["arguments"],
            });
            let asked = self.servers[server].request(
                "workspace/executeCommand",
                "executeCommandProvider",
                params,
                Request::Execute,
            );
            if !asked {
                self.message = format!("{} cannot run {}", self.servers[server].name, command["command"]);
                return;
            }
            self.message = action.title;
        }
    }

    /// The rest of an action the server was asked to fill in.
    fn resolved_action(&mut self, request: Request, action: Option<CodeAction>) {
        let Request::ResolveAction { view, edits } = request else {
            return;
        };
        if self.current != view || self.view().edits() != edits {
            self.message = "the buffer changed while that was being worked out".into();
            return;
        }
        let Lsp::Open { server, .. } = self.views[self.current].lsp else {
            return;
        };
        match action {
            Some(action) if !action.needs_resolving() => self.do_action(server, action),
            _ => self.message = "nothing came back".into(),
        }
    }

    /// The server asking for an edit rather than answering with one, which is
    /// what running a command comes back as. It is a request: it wants to be
    /// told whether the edit was made.
    fn apply_edit_request(&mut self, server: usize, id: Value, edit: WorkspaceEdit, encoding: Encoding) {
        let applied = match edit.is_empty() {
            true => Ok((0, 0)),
            false => self.apply_workspace_edit(edit, encoding),
        };
        let result = match &applied {
            Ok((places, _)) => {
                if *places > 0 {
                    self.message = changes(*places);
                }
                json!({ "applied": true })
            }
            Err(err) => {
                self.message = format!("{err:#}");
                json!({ "applied": false, "failureReason": err.to_string() })
            }
        };
        if let Some(client) = self.servers.get(server) {
            client.respond(id, result);
        }
    }

    /// Ask for inlay hints for every buffer whose server has its latest text
    /// and has not been asked about it yet. Not while typing: hints about a
    /// line half typed are about to be wrong, and the ones there already are
    /// carried along with the text until they can be asked for again.
    pub fn lsp_hints(&mut self) {
        if !self.lsp_enabled || !self.inlayhints || self.mode == Mode::Insert {
            return;
        }
        for index in 0..self.views.len() {
            let view = &self.views[index];
            let Lsp::Open { server, synced, .. } = view.lsp else {
                continue;
            };
            let edits = view.edits();
            // Turned away - still indexing - a moment ago: that long again
            // before asking, or the asking is all the server hears.
            let retry = view.hints_failed.is_some_and(|when| when.elapsed() >= HINTS_RETRY);
            if retry {
                self.views[index].hints_failed = None;
                self.views[index].hints_asked = None;
            }
            let view = &self.views[index];
            let Some(path) = view.doc.path.as_deref() else {
                continue;
            };
            if synced != edits || view.hints_asked == Some(edits) {
                continue;
            }
            let encoding = self.servers[server].encoding;
            let (line, character) = lsp::to_position(&view.doc.text, view.doc.len_chars(), encoding);
            let params = json!({
                "textDocument": { "uri": lsp::uri(path) },
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": line, "character": character },
                },
            });
            let request = Request::InlayHint { view: index, edits };
            if self.servers[server].request("textDocument/inlayHint", "inlayHintProvider", params, request) {
                self.views[index].hints_asked = Some(edits);
            }
        }
    }

    /// Hints for a buffer, kept if the text is still what they were asked
    /// about. When it is not, the text has moved on and a new request is on
    /// its way or will be once typing stops.
    fn hints_answer(&mut self, request: Request, hints: Vec<RawHint>, encoding: Encoding) {
        let Request::InlayHint { view, edits } = request else {
            return;
        };
        if !self.inlayhints {
            return;
        }
        let Some(view) = self.views.get_mut(view).filter(|view| view.edits() == edits) else {
            return;
        };
        let rope = &view.doc.text;
        let mut hints: Vec<Hint> = hints
            .into_iter()
            .map(|hint| Hint { at: lsp::from_position(rope, hint.position.0, hint.position.1, encoding), label: hint.label })
            .collect();
        hints.sort_by_key(|hint| hint.at);
        view.hints = hints;
    }

    /// Ask for the names across the project that match `query`, for the
    /// workspace symbol picker. The buffer's own server first; any other that
    /// can answer if this buffer has none, since a Rust project's names are
    /// worth finding from its README too.
    pub(super) fn lsp_workspace_symbols(&mut self, query: String, token: u64) -> bool {
        if !self.lsp_enabled {
            return false;
        }
        self.lsp_sync();
        let own = match self.view().lsp {
            Lsp::Open { server, .. } => Some(server),
            _ => None,
        };
        let others = 0..self.servers.len();
        let request = Request::WorkspaceSymbol { token };
        let params = json!({ "query": query });
        own.into_iter().chain(others).any(|server| {
            self.servers[server].request("workspace/symbol", "workspaceSymbolProvider", params.clone(), request)
        })
    }

    /// What matched, into the picker - as long as it is still the picker that
    /// asked, and nothing has been typed into it since.
    fn workspace_symbols_answer(&mut self, request: Request, symbols: Vec<Symbol>) {
        let Request::WorkspaceSymbol { token } = request else {
            return;
        };
        if token != self.token() || !self.picker.as_ref().is_some_and(|picker| picker.source == Source::Workspace) {
            return;
        }
        let root = std::env::current_dir().ok();
        let items: Vec<Item> = symbols
            .into_iter()
            .take(SYMBOLS_SHOWN)
            .map(|symbol| {
                let shown = root
                    .as_deref()
                    .and_then(|root| symbol.location.path.strip_prefix(root).ok())
                    .unwrap_or(symbol.location.path.as_path());
                let line = symbol.location.position.0 as usize;
                let container = symbol.container.map(|name| format!("{name} ")).unwrap_or_default();
                Item {
                    text: symbol.name,
                    detail: format!("{} {container}{}:{}", symbol.kind, shown.display(), line + 1).trim_start().to_string(),
                    id: line + 1,
                    target: symbol.location.path.display().to_string(),
                }
            })
            .collect();
        if let Some(picker) = self.picker.as_mut() {
            picker.extend(items, true);
        }
    }

    /// `gr`: every use of the name under the cursor, in a picker. The
    /// declaration is included - looking at what uses a function, the function
    /// itself is one of the places you want to get back to.
    pub(super) fn lsp_references(&mut self) -> bool {
        let Some((server, mut params, head, edits)) = self.at_cursor() else {
            return false;
        };
        params["context"] = json!({ "includeDeclaration": true });
        let request = Request::References { view: self.current, head, edits };
        if !self.servers[server].request("textDocument/references", "referencesProvider", params, request) {
            return false;
        }
        self.message = format!("asking {}", self.servers[server].name);
        true
    }

    /// The uses the server found, as a list to walk. Each row is the line the
    /// use is on, read from the buffer if the file is open and from the disk
    /// if it is not, so the list says what the code does rather than only
    /// where it is.
    fn references_answer(&mut self, request: Request, locations: Vec<Location>) {
        let Request::References { view, head, edits } = request else {
            return;
        };
        if !self.still_asking(view, head, edits) {
            return;
        }
        self.message.clear();
        if locations.is_empty() {
            self.message = "no references".into();
            return;
        }

        let root = std::env::current_dir().ok();
        let mut cached: Option<(std::path::PathBuf, Vec<String>)> = None;
        let mut items = Vec::with_capacity(locations.len());
        for location in &locations {
            let line = location.position.0 as usize;
            // The open buffer first: it has what is on screen, which after an
            // edit is not what the file on disk says.
            let open = self.views.iter().find(|view| {
                view.doc.path.as_deref().map(absolute).as_deref() == Some(location.path.as_path())
            });
            let text = match open {
                Some(view) if line < view.doc.len_lines() => view.doc.line_str(line).trim().to_string(),
                Some(_) => String::new(),
                None => {
                    if cached.as_ref().is_none_or(|(path, _)| path != &location.path) {
                        let read = std::fs::read_to_string(&location.path).unwrap_or_default();
                        let lines = read.lines().map(str::to_string).collect();
                        cached = Some((location.path.clone(), lines));
                    }
                    let (_, lines) = cached.as_ref().expect("just set");
                    lines.get(line).map(|text| text.trim().to_string()).unwrap_or_default()
                }
            };
            // Shown relative to where jack was started, which is how every
            // other list of paths here reads.
            let shown = root
                .as_deref()
                .and_then(|root| location.path.strip_prefix(root).ok())
                .unwrap_or(location.path.as_path());
            items.push(Item {
                text,
                detail: format!("{}:{}", shown.display(), line + 1),
                id: line + 1,
                target: location.path.display().to_string(),
            });
        }
        self.open_picker(Picker::new(Source::References, items));
    }

    /// `gR`: rename the name under the cursor, everywhere the server knows of
    /// it. The new name was typed into the prompt; what comes back is a list
    /// of edits over however many files.
    pub(super) fn lsp_rename(&mut self, name: &str) -> bool {
        let Some((server, mut params, head, edits)) = self.at_cursor() else {
            return false;
        };
        params["newName"] = json!(name);
        let request = Request::Rename { view: self.current, head, edits };
        if !self.servers[server].request("textDocument/rename", "renameProvider", params, request) {
            return false;
        }
        self.message = format!("renaming to {name}");
        true
    }

    /// The rename the server worked out, applied. Files it touches that are
    /// not open are opened, and nothing is written: a rename you can see and
    /// undo is worth more than one that has already happened on disk.
    fn rename_answer(&mut self, request: Request, edit: WorkspaceEdit, encoding: Encoding) {
        let Request::Rename { view, head, edits } = request else {
            return;
        };
        if !self.still_asking(view, head, edits) {
            return;
        }
        if edit.is_empty() {
            self.message = "nothing to rename".into();
            return;
        }
        match self.apply_workspace_edit(edit, encoding) {
            Ok((places, files)) => {
                self.message = match files {
                    1 => format!("renamed: {places} places"),
                    files => format!("renamed: {places} places in {files} files"),
                }
            }
            Err(err) => self.message = format!("{err:#}"),
        }
    }

    /// Everything a server wants changed, over however many files. Each file
    /// is one undo step in its own buffer, back to front so that the edits
    /// still to come are still where the server said they were.
    ///
    /// The buffer that was in front stays in front: an edit is not a reason to
    /// be taken somewhere else.
    fn apply_workspace_edit(
        &mut self,
        edit: WorkspaceEdit,
        encoding: Encoding,
    ) -> anyhow::Result<(usize, usize)> {
        let was = self.current;
        let here = self.view().doc.path.as_deref().map(absolute);
        let (mut places, mut files) = (0, 0);

        for (path, edits) in edit.changes {
            if edits.is_empty() {
                continue;
            }
            let index = match self
                .views
                .iter()
                .position(|view| view.doc.path.as_deref().map(absolute).as_deref() == Some(path.as_path()))
            {
                Some(index) => index,
                None => {
                    self.open_file(&path)?;
                    self.current
                }
            };
            self.current = index;

            let mut edits = edits;
            edits.sort_by_key(|edit| edit.start);
            self.begin_undo_group();
            for edit in edits.iter().rev() {
                let doc = &self.views[index].doc;
                let start = lsp::from_position(&doc.text, edit.start.0, edit.start.1, encoding);
                let end = lsp::from_position(&doc.text, edit.end.0, edit.end.1, encoding);
                self.views[index].edit_at(start, end.saturating_sub(start), &edit.text, None);
            }
            self.end_undo_group();
            places += edits.len();
            files += 1;
        }

        // Back where it started, by path rather than by index: opening a file
        // can take the scratch buffer's place and move the indexes about.
        let back = here
            .and_then(|here| {
                self.views
                    .iter()
                    .position(|view| view.doc.path.as_deref().map(absolute).as_deref() == Some(here.as_path()))
            })
            .unwrap_or(was.min(self.views.len() - 1));
        self.switch_to(back);
        self.clamp_cursor();
        Ok((places, files))
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
pub(super) fn absolute(path: &Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| match std::env::current_dir() {
        Ok(dir) => dir.join(path),
        Err(_) => path.to_path_buf(),
    })
}

/// "1 change", "4 changes".
fn changes(places: usize) -> String {
    match places {
        1 => "1 change".into(),
        many => format!("{many} changes"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lsp::Severity;
    use crate::lsp::tests::sent;
    use std::path::PathBuf;

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
        RawDiagnostic { start, end, severity, message: message.into(), raw: Value::Null }
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

    /// A scratch editor whose server answers hovers and signatures.
    fn editor_asking(text: &str) -> (Editor, std::sync::mpsc::Receiver<Vec<u8>>) {
        let mut editor = Editor::scratch();
        let view = editor.view_mut();
        view.doc.text = ropey::Rope::from_str(text);
        view.doc.path = Some("/nowhere/a.py".into());
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({
            "hoverProvider": true,
            "signatureHelpProvider": { "triggerCharacters": ["(", ","] },
        }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        (editor, written)
    }

    /// The id of the one request of `method` that was sent.
    fn asked_for(written: &std::sync::mpsc::Receiver<Vec<u8>>, method: &str) -> Value {
        let sent = sent(written);
        let request = sent.iter().find(|m| m["method"] == method).expect("the request");
        request["id"].clone()
    }

    #[test]
    fn k_puts_what_the_server_says_in_a_box_by_the_cursor() {
        let (mut editor, written) = editor_asking("count = 1\n");
        editor.view_mut().sel = Selection::point(2);
        editor.hover();
        assert_eq!(editor.message, "asking...");

        let id = asked_for(&written, "textDocument/hover");
        let markup = "```python\n(variable) count: int\n```\n---\nHow many.";
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "contents": { "kind": "markdown", "value": markup },
        }})));

        let info = editor.info.as_ref().expect("a box");
        let lines: Vec<&str> = info.lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(lines, ["(variable) count: int", "", "How many."]);
        assert_eq!(info.kind, crate::info::Kind::Hover);
        assert_eq!(editor.message, "", "the box is the answer, not the status line");

        // Read once: the next key takes it away again.
        editor.dismiss_hover();
        assert_eq!(editor.info, None);
    }

    #[test]
    fn a_hover_answer_about_where_you_no_longer_are_is_dropped() {
        let (mut editor, written) = editor_asking("count = 1\n");
        editor.hover();
        let id = asked_for(&written, "textDocument/hover");
        editor.view_mut().sel = Selection::point(7);
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "contents": "about somewhere else",
        }})));
        assert_eq!(editor.info, None);
    }

    #[test]
    fn a_server_with_nothing_to_say_says_so_rather_than_nothing() {
        let (mut editor, written) = editor_asking("count = 1\n");
        editor.hover();
        let id = asked_for(&written, "textDocument/hover");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": null })));
        assert_eq!(editor.info, None);
        assert_eq!(editor.message, "nothing known about that");
    }

    #[test]
    fn k_without_a_server_says_there_is_none() {
        let mut editor = Editor::scratch();
        editor.hover();
        assert_eq!(editor.message, "no language server for this buffer");
        assert_eq!(editor.info, None);
    }

    #[test]
    fn a_bracket_asks_for_the_signature_of_the_call_being_typed() {
        let (mut editor, written) = editor_asking("f(\n");
        editor.set_mode(Mode::Insert);
        editor.view_mut().sel = Selection::point(2);
        editor.signature_hint('(');

        let id = asked_for(&written, "textDocument/signatureHelp");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "signatures": [{
                "label": "f(a: int, b: str) -> None",
                "parameters": [{ "label": [2, 8] }, { "label": [10, 16] }],
            }],
            "activeParameter": 0,
        }})));

        let info = editor.info.as_ref().expect("a box");
        assert_eq!(info.kind, crate::info::Kind::Signature);
        let line = &info.lines[0];
        assert_eq!(line.text, "f(a: int, b: str) -> None");
        let active = line.active.clone().expect("the parameter being typed");
        assert_eq!(&line.text[active], "a: int");

        // And it goes away with the call it is about: `esc` ends insert mode.
        editor.set_mode(Mode::Normal);
        assert_eq!(editor.info, None);
    }

    #[test]
    fn typing_the_argument_does_not_make_the_answer_stale() {
        let (mut editor, written) = editor_asking("f(\n");
        editor.set_mode(Mode::Insert);
        editor.view_mut().sel = Selection::point(2);
        editor.signature_hint('(');
        let id = asked_for(&written, "textDocument/signatureHelp");

        // What you do while waiting for it is type the argument it is about.
        editor.insert("1, ");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "signatures": [{ "label": "f(a: int, b: str)" }],
        }})));
        assert!(editor.info.is_some(), "still the call being typed");
    }

    #[test]
    fn closing_the_call_takes_its_signature_down() {
        let (mut editor, written) = editor_asking("f(\n");
        editor.set_mode(Mode::Insert);
        editor.view_mut().sel = Selection::point(2);
        editor.signature_hint('(');
        let first = asked_for(&written, "textDocument/signatureHelp");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": first, "result": {
            "signatures": [{ "label": "f(a: int)" }],
        }})));
        assert!(editor.info.is_some());

        // A `)` is asked about too, and the answer to it is that there is no
        // call being typed any more.
        editor.insert("1)");
        editor.signature_hint(',');
        let ids: Vec<Value> = sent(&written)
            .iter()
            .filter(|m| m["method"] == "textDocument/signatureHelp")
            .map(|m| m["id"].clone())
            .collect();
        let last = ids.last().expect("a second request").clone();
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": last, "result": null })));
        assert_eq!(editor.info, None);
    }

    #[test]
    fn a_character_the_server_did_not_ask_about_asks_nothing() {
        let (mut editor, written) = editor_asking("f(\n");
        editor.set_mode(Mode::Insert);
        editor.signature_hint('x');
        assert!(!sent(&written).iter().any(|m| m["method"] == "textDocument/signatureHelp"));
    }

    #[test]
    fn deleting_back_past_the_bracket_takes_the_signature_with_it() {
        let (mut editor, written) = editor_asking("f(\n");
        editor.set_mode(Mode::Insert);
        editor.view_mut().sel = Selection::point(2);
        editor.signature_hint('(');
        let id = asked_for(&written, "textDocument/signatureHelp");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "signatures": [{ "label": "f(a: int)" }],
        }})));
        assert!(editor.info.is_some());

        // Still inside the call: the signature is still what you are typing.
        editor.view_mut().sel = Selection::point(3);
        editor.update_info();
        assert!(editor.info.is_some());

        editor.view_mut().sel = Selection::point(1);
        editor.update_info();
        assert_eq!(editor.info, None, "back out of the call, and it is about nothing");
    }

    /// A scratch editor whose server answers about references and renames,
    /// on a real file in a real directory: both of those answer with paths,
    /// and a path that does not exist cannot be read or opened.
    fn editor_in_a_project(name: &str, files: &[(&str, &str)]) -> (Editor, PathBuf, std::sync::mpsc::Receiver<Vec<u8>>) {
        let dir = std::env::temp_dir().join(format!("jack_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Opening a file resolves its path, and on macOS the temporary
        // directory is under `/var`, which is a symlink to `/private/var`. So
        // the directory the test compares against has to be the resolved one,
        // or it is comparing two spellings of the same place.
        let dir = dir.canonicalize().unwrap_or(dir);
        for (file, text) in files {
            std::fs::write(dir.join(file), text).unwrap();
        }

        let mut editor = Editor::scratch();
        editor.open_file(dir.join(files[0].0)).unwrap();
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({ "referencesProvider": true, "renameProvider": true }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        (editor, dir, written)
    }

    #[test]
    fn gr_lists_every_use_with_the_line_it_is_on() {
        let (mut editor, dir, written) = editor_in_a_project(
            "refs",
            &[("a.rs", "fn count() {}
let n = count();
"), ("b.rs", "use crate::count;
")],
        );
        editor.references();
        assert_eq!(editor.message, "asking fake");
        let id = asked_for(&written, "textDocument/references");

        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            { "uri": lsp::uri(&dir.join("a.rs")), "range": { "start": { "line": 0, "character": 3 }, "end": { "line": 0, "character": 8 } } },
            { "uri": lsp::uri(&dir.join("a.rs")), "range": { "start": { "line": 1, "character": 8 }, "end": { "line": 1, "character": 13 } } },
            { "uri": lsp::uri(&dir.join("b.rs")), "range": { "start": { "line": 0, "character": 11 }, "end": { "line": 0, "character": 16 } } },
        ]})));

        let picker = editor.picker.as_ref().expect("a picker");
        let rows: Vec<&Item> = picker.matches().iter().map(|m| picker.item(m)).collect();
        let shown: Vec<&str> = rows.iter().map(|item| item.text.as_str()).collect();
        // The open buffer is read from the buffer; the other file from disk.
        assert_eq!(shown, ["fn count() {}", "let n = count();", "use crate::count;"]);
        let lines: Vec<usize> = rows.iter().map(|item| item.id).collect();
        assert_eq!(lines, [1, 2, 1], "line numbers, counted from one");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn space_capital_s_asks_the_server_as_you_type() {
        let (mut editor, dir, _) = editor_in_a_project("symbols", &[("a.rs", "fn main() {}\n"), ("b.rs", "struct Counter;\n")]);
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({ "workspaceSymbolProvider": true }));
        editor.servers[0] = client;

        let key = |c| crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char(c), crossterm::event::KeyModifiers::NONE);
        editor.open_workspace_symbol_picker();
        editor.picker_input(key('C'));
        let first = asked_for(&written, "workspace/symbol");
        editor.picker_input(key('o'));
        let second = asked_for(&written, "workspace/symbol");

        let found = json!([{
            "name": "Counter", "kind": 23, "containerName": "b",
            "location": { "uri": lsp::uri(&dir.join("b.rs")), "range": { "start": { "line": 0, "character": 7 }, "end": { "line": 0, "character": 14 } } },
        }]);
        // The answer to "C" arrives after "Co" was typed: it is about a query
        // nobody is looking at any more.
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": first, "result": [{
            "name": "Stale", "kind": 12,
            "location": { "uri": lsp::uri(&dir.join("a.rs")), "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } } },
        }]})));
        assert!(editor.picker.as_ref().expect("a picker").matches().is_empty());
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": second, "result": found })));

        let picker = editor.picker.as_ref().expect("a picker");
        let rows: Vec<&Item> = picker.matches().iter().map(|m| picker.item(m)).collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "Counter");
        assert!(rows[0].detail.starts_with("struct b "), "{}", rows[0].detail);
        assert!(rows[0].detail.ends_with("b.rs:1"), "{}", rows[0].detail);

        editor.picker_input(crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Enter, crossterm::event::KeyModifiers::NONE));
        assert_eq!(editor.view().doc.path.as_deref(), Some(dir.join("b.rs").as_path()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rename_reaches_files_that_were_not_open() {
        let (mut editor, dir, written) = editor_in_a_project(
            "rename",
            &[("a.rs", "fn count() {}
"), ("b.rs", "use crate::count;
")],
        );
        assert_eq!(editor.views().len(), 1, "only the one file is open");

        editor.start_rename();
        assert_eq!(editor.prompt.as_ref().expect("a prompt").input, "fn", "the word under the cursor");
        editor.prompt.as_mut().expect("a prompt").input = "total".into();
        editor.prompt_input(crossterm::event::KeyEvent::new(
            crossterm::event::KeyCode::Enter,
            crossterm::event::KeyModifiers::NONE,
        ));

        let id = asked_for(&written, "textDocument/rename");
        let edit = |line: u32, first: u32, last: u32| json!({
            "range": { "start": { "line": line, "character": first }, "end": { "line": line, "character": last } },
            "newText": "total",
        });
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": {
            "changes": {
                lsp::uri(&dir.join("a.rs")): [edit(0, 3, 8)],
                lsp::uri(&dir.join("b.rs")): [edit(0, 11, 16)],
            },
        }})));

        assert_eq!(editor.message, "renamed: 2 places in 2 files");
        assert_eq!(editor.views().len(), 2, "the other file was opened to be changed");
        // Still looking at the file it was asked from.
        assert_eq!(editor.view().doc.path.as_deref(), Some(dir.join("a.rs").as_path()));
        assert_eq!(editor.view().doc.text.to_string(), "fn total() {}\n");
        let other = editor.views().iter().find(|view| view.doc.path.as_deref() == Some(dir.join("b.rs").as_path()));
        assert_eq!(other.expect("open").doc.text.to_string(), "use crate::total;\n");
        // Nothing is written: a rename you can undo is worth more than one
        // that has already happened on disk.
        assert_eq!(std::fs::read_to_string(dir.join("b.rs")).unwrap(), "use crate::count;\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch editor whose server offers code actions and runs commands.
    /// Each test names its own directory: they run at once, and one that
    /// deletes a shared directory deletes the others' files with it.
    fn editor_acting(name: &str, text: &str) -> (Editor, PathBuf, std::sync::mpsc::Receiver<Vec<u8>>) {
        let dir = std::env::temp_dir().join(format!("jack_actions_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), text).unwrap();

        let mut editor = Editor::scratch();
        editor.open_file(dir.join("a.rs")).unwrap();
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({
            "codeActionProvider": { "resolveProvider": true },
            "executeCommandProvider": { "commands": ["fake.fix"] },
        }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        (editor, dir, written)
    }

    #[test]
    fn ga_asks_about_the_range_with_the_diagnostics_it_covers() {
        let (mut editor, dir, written) = editor_acting("range", "let x = 1;\nlet y = 2;\n");
        let path = absolute(&dir.join("a.rs"));
        editor.set_diagnostics(
            &path,
            vec![
                RawDiagnostic {
                    start: (0, 4),
                    end: (0, 5),
                    severity: Severity::Warning,
                    message: "unused x".into(),
                    raw: json!({ "message": "unused x", "data": { "id": 7 } }),
                },
                RawDiagnostic {
                    start: (1, 4),
                    end: (1, 5),
                    severity: Severity::Warning,
                    message: "unused y".into(),
                    raw: json!({ "message": "unused y" }),
                },
            ],
            Encoding::Utf16,
        );
        editor.view_mut().sel = Selection::point(4);
        editor.code_actions();

        let sent = sent(&written);
        let request = sent.iter().find(|m| m["method"] == "textDocument/codeAction").expect("asked");
        assert_eq!(request["params"]["range"]["start"], json!({ "line": 0, "character": 4 }));
        // Only the diagnostic the cursor is in, and as the server wrote it:
        // the `data` is what it recognises its own fix by.
        let context = &request["params"]["context"]["diagnostics"];
        assert_eq!(context.as_array().map(Vec::len), Some(1));
        assert_eq!(context[0]["data"]["id"], 7);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn choosing_an_action_that_came_with_its_edits_applies_them() {
        let (mut editor, dir, written) = editor_acting("edits", "let x = 1;\n");
        editor.code_actions();
        let id = asked_for(&written, "textDocument/codeAction");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            {
                "title": "Remove unused variable",
                "edit": { "changes": { lsp::uri(&dir.join("a.rs")): [{
                    "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 5 } },
                    "newText": "_x",
                }]}},
            },
            { "title": "Something else" },
        ]})));

        let picker = editor.picker.as_ref().expect("a picker");
        let titles: Vec<&str> = picker.matches().iter().map(|m| picker.item(m).text.as_str()).collect();
        assert_eq!(titles, ["Remove unused variable", "Something else"]);

        editor.run_code_action(0);
        assert_eq!(editor.view().doc.text.to_string(), "let _x = 1;\n");
        assert_eq!(editor.message, "Remove unused variable: 1 change");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_action_that_is_only_a_title_is_asked_about_again() {
        let (mut editor, dir, written) = editor_acting("resolve", "let x = 1;\n");
        editor.code_actions();
        let id = asked_for(&written, "textDocument/codeAction");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            { "title": "Import the thing", "kind": "quickfix", "data": { "token": 3 } },
        ]})));
        editor.run_code_action(0);

        let sent = sent(&written);
        let resolve = sent.iter().find(|m| m["method"] == "codeAction/resolve").expect("resolved");
        // The action goes back whole - the server knows it by its `data`.
        assert_eq!(resolve["params"]["data"]["token"], 3);

        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": resolve["id"], "result": {
            "title": "Import the thing",
            "data": { "token": 3 },
            "edit": { "documentChanges": [{
                "textDocument": { "uri": lsp::uri(&dir.join("a.rs")), "version": 1 },
                "edits": [{
                    "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                    "newText": "use thing;\n",
                }],
            }]},
        }})));
        assert_eq!(editor.view().doc.text.to_string(), "use thing;\nlet x = 1;\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_command_is_handed_back_and_the_edit_it_asks_for_is_made() {
        let (mut editor, dir, written) = editor_acting("command", "let x = 1;\n");
        editor.code_actions();
        let id = asked_for(&written, "textDocument/codeAction");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            { "title": "Fix it", "command": { "command": "fake.fix", "arguments": [1] } },
        ]})));
        editor.run_code_action(0);

        let run = sent(&written)
            .into_iter()
            .find(|m| m["method"] == "workspace/executeCommand")
            .expect("ran");
        assert_eq!(run["params"]["command"], "fake.fix");

        // What the command does comes back as a request of the server's own.
        editor.lsp_message(0, Some(json!({
            "jsonrpc": "2.0",
            "id": 99,
            "method": "workspace/applyEdit",
            "params": { "edit": { "changes": { lsp::uri(&dir.join("a.rs")): [{
                "range": { "start": { "line": 0, "character": 8 }, "end": { "line": 0, "character": 9 } },
                "newText": "2",
            }]}}},
        })));
        assert_eq!(editor.view().doc.text.to_string(), "let x = 2;\n");
        // And it is a request: it is told the edit was made.
        let answer = sent(&written).into_iter().find(|m| m["id"] == 99).expect("answered");
        assert_eq!(answer["result"]["applied"], true);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scratch editor whose server formats.
    fn editor_formatting(text: &str) -> (Editor, std::sync::mpsc::Receiver<Vec<u8>>) {
        let mut editor = Editor::scratch();
        let view = editor.view_mut();
        view.doc.text = ropey::Rope::from_str(text);
        view.doc.path = Some("/nowhere/a.rs".into());
        let (mut client, written) = Client::detached("fake");
        client.ready_with(json!({
            "documentFormattingProvider": true,
            "documentRangeFormattingProvider": true,
        }));
        editor.servers.push(client);
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 0, synced: 0 };
        (editor, written)
    }

    /// A replacement of lines `first..last` with `text`, as a server sends it.
    fn text_edit(first: u32, last: u32, text: &str) -> Value {
        json!({
            "range": { "start": { "line": first, "character": 0 }, "end": { "line": last, "character": 0 } },
            "newText": text,
        })
    }

    #[test]
    fn formatting_is_applied_back_to_front_as_one_undo_step() {
        let (mut editor, written) = editor_formatting("fn a(){}\nfn b(){}\nfn c(){}\n");
        editor.goto_line(2);
        editor.run_command("fmt");
        assert_eq!(editor.message, "formatting...");

        let id = asked_for(&written, "textDocument/formatting");
        // Two edits, sent in the order the file reads. Applying the first one
        // first would move the second one out from under itself.
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            text_edit(0, 1, "fn a() {}\n"),
            text_edit(2, 3, "fn c() {}\n"),
        ]})));

        assert_eq!(editor.view().doc.text.to_string(), "fn a() {}\nfn b(){}\nfn c() {}\n");
        assert_eq!(editor.message, "formatted: 2 changes");
        assert_eq!(editor.cursor_coords().0, 2, "still on the line it was on");

        // One undo takes the whole reformat back.
        editor.undo();
        assert_eq!(editor.view().doc.text.to_string(), "fn a(){}\nfn b(){}\nfn c(){}\n");
    }

    #[test]
    fn what_the_server_is_told_is_what_the_buffer_indents_with() {
        let (mut editor, written) = editor_formatting("fn a(){}\n");
        editor.run_command("set shiftwidth=2");
        editor.run_command("set expandtab");
        editor.run_command("fmt");
        let sent = sent(&written);
        let request = sent.iter().find(|m| m["method"] == "textDocument/formatting").expect("asked");
        assert_eq!(request["params"]["options"]["tabSize"], 2);
        assert_eq!(request["params"]["options"]["insertSpaces"], true);
        // A whole-file format is not about a position, and does not send one.
        assert!(request["params"].get("position").is_none());
    }

    #[test]
    fn a_range_formats_the_lines_it_names() {
        let (mut editor, written) = editor_formatting("fn a(){}\nfn b(){}\nfn c(){}\n");
        editor.run_command("2,3fmt");
        let sent = sent(&written);
        let request = sent
            .iter()
            .find(|m| m["method"] == "textDocument/rangeFormatting")
            .expect("the ranged request");
        assert_eq!(request["params"]["range"]["start"]["line"], 1);
        // To the start of the line after the last one named, so line 3 goes
        // over whole rather than up to its first character.
        assert_eq!(request["params"]["range"]["end"], json!({ "line": 3, "character": 0 }));
    }

    #[test]
    fn an_answer_about_text_that_has_since_been_typed_into_is_refused() {
        let (mut editor, written) = editor_formatting("fn a(){}\n");
        editor.run_command("fmt");
        let id = asked_for(&written, "textDocument/formatting");
        editor.set_mode(Mode::Insert);
        editor.insert("x");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            text_edit(0, 1, "fn a() {}\n"),
        ]})));
        assert!(editor.view().doc.text.to_string().starts_with('x'), "untouched");
        assert_eq!(editor.message, "the buffer changed while it was being formatted");
    }

    #[test]
    fn a_server_with_nothing_to_change_says_so() {
        let (mut editor, written) = editor_formatting("fn a() {}\n");
        editor.run_command("fmt");
        let id = asked_for(&written, "textDocument/formatting");
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [] })));
        assert_eq!(editor.message, "already formatted");
    }

    #[test]
    fn fmt_without_a_server_says_so() {
        let mut editor = Editor::scratch();
        editor.run_command("fmt");
        assert_eq!(editor.message, "no language server that formats this");
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

    #[test]
    fn hints_are_asked_for_once_per_text_and_kept_only_for_that_text() {
        let (mut editor, written) = editor_asking("x = f(1)\n");
        editor.servers[0].ready_with(json!({ "inlayHintProvider": true }));
        editor.lsp_hints();
        let id = asked_for(&written, "textDocument/inlayHint");
        editor.lsp_hints();
        assert!(sent(&written).is_empty(), "not again for the same text");

        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [
            { "position": { "line": 0, "character": 6 }, "label": "n:", "paddingRight": true },
        ]})));
        assert_eq!(editor.view().hints, [Hint { at: 6, label: "n: ".into() }]);

        // An answer about text that has changed since is not used.
        editor.view_mut().edit_at(0, 0, "y", Some(0));
        editor.view_mut().lsp = Lsp::Open { server: 0, version: 1, synced: editor.view().edits() };
        editor.lsp_hints();
        let id = asked_for(&written, "textDocument/inlayHint");
        editor.view_mut().edit_at(0, 0, "z", Some(0));
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "result": [] })));
        assert_eq!(editor.view().hints, [Hint { at: 8, label: "n: ".into() }], "the old ones, carried along");

        // Refused while the server indexes: asked again, but not at once.
        let id = {
            editor.view_mut().lsp = Lsp::Open { server: 0, version: 2, synced: editor.view().edits() };
            editor.lsp_hints();
            asked_for(&written, "textDocument/inlayHint")
        };
        editor.lsp_message(0, Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": -32801, "message": "content modified" } })));
        editor.lsp_hints();
        assert!(sent(&written).is_empty(), "not straight away");
        editor.view_mut().hints_failed = Some(std::time::Instant::now() - HINTS_RETRY);
        editor.lsp_hints();
        asked_for(&written, "textDocument/inlayHint");

        editor.run_command("set noinlayhints");
        assert!(editor.view().hints.is_empty());
    }

    /// What a server that takes ranges ends up with: the text it was opened
    /// with, and every change it was sent applied in order.
    fn replay(text: &str, sent: &[Value], encoding: Encoding) -> String {
        let mut mirror = ropey::Rope::from_str(text);
        for message in sent.iter().filter(|m| m["method"] == "textDocument/didChange") {
            for change in message["params"]["contentChanges"].as_array().unwrap() {
                let Some(range) = change.get("range") else {
                    mirror = ropey::Rope::from_str(change["text"].as_str().unwrap());
                    continue;
                };
                let at = |p: &Value| {
                    lsp::from_position(&mirror, p["line"].as_u64().unwrap() as u32, p["character"].as_u64().unwrap() as u32, encoding)
                };
                let (start, end) = (at(&range["start"]), at(&range["end"]));
                mirror.remove(start..end);
                mirror.insert(start, change["text"].as_str().unwrap());
            }
        }
        mirror.to_string()
    }

    #[test]
    fn changes_are_sent_as_ranges_that_rebuild_the_text() {
        for encoding in [Encoding::Utf16, Encoding::Utf8] {
            let start = "fn main() {\n    let a = \"żółw 🐢\";\n    call(a, a);\n}\n";
            let (mut editor, written) = editor_asking(start);
            editor.servers[0].ready_with(json!({ "textDocumentSync": { "openClose": true, "change": 2 } }));
            editor.servers[0].encoding = encoding;

            let mut sent_all = Vec::new();
            let mut sync = |editor: &mut Editor| {
                editor.lsp_sync();
                sent_all.extend(sent(&written));
            };
            // After the emoji, on a line with letters two bytes wide.
            let at = editor.view().doc.text.to_string().find("🐢").unwrap();
            let at = editor.view().doc.text.byte_to_char(at) + 1;
            editor.view_mut().edit_at(at, 0, " and\nmore", Some(at));
            sync(&mut editor);
            // Several changes in one command, across lines.
            editor.run_command("%s/a/ą/g");
            // Across a line break, then back again.
            editor.view_mut().edit_at(3, 12, "", Some(3));
            editor.undo();
            sync(&mut editor);
            editor.undo();
            editor.redo();
            sync(&mut editor);

            let changes: Vec<&Value> = sent_all.iter().filter(|m| m["method"] == "textDocument/didChange").collect();
            assert!(changes.len() >= 3, "{encoding:?}");
            assert!(changes.iter().all(|m| m["params"]["contentChanges"][0].get("range").is_some()), "ranges, not texts");
            assert_eq!(replay(start, &sent_all, encoding), editor.view().doc.text.to_string(), "{encoding:?}");
        }
    }

    #[test]
    fn a_server_that_wants_whole_texts_gets_them() {
        let (mut editor, written) = editor_asking("one\n");
        editor.servers[0].ready_with(json!({ "textDocumentSync": 1 }));
        editor.view_mut().edit_at(0, 0, "zero ", Some(0));
        editor.lsp_sync();
        let sent = sent(&written);
        assert_eq!(sent[0]["params"]["contentChanges"], json!([{ "text": "zero one\n" }]));
    }
}

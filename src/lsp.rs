//! Language servers: finding one, starting it, and speaking its protocol.
//!
//! A server is a child process talking JSON-RPC over its stdin and stdout.
//! Reading happens on a thread of its own and every message is handed to the
//! run loop on the same channel as keys and git signs, so nothing here ever
//! blocks a keystroke; writing goes through a thread too, since a server busy
//! indexing can stop reading for a while. What a message *means* to the
//! buffers is the editor's business - this file turns the protocol into a few
//! plain events and back.

use anyhow::{Context, Result, anyhow};
use ropey::Rope;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Sender, channel};
use std::thread;

use crate::stream::Message;

/// A server jack knows how to start, and which files it is for.
pub struct Spec {
    pub name: &'static str,
    command: &'static str,
    args: &'static [&'static str],
    /// Language names as the grammar registry has them.
    languages: &'static [&'static str],
    /// Files that mark the top of a project this server understands. The
    /// nearest one above the file wins; failing that, the repository.
    roots: &'static [&'static str],
}

/// In order of preference: for a language with two, the first one installed.
pub const SERVERS: &[Spec] = &[
    Spec {
        name: "rust-analyzer",
        command: "rust-analyzer",
        args: &[],
        languages: &["rust"],
        roots: &["Cargo.toml"],
    },
    Spec {
        name: "clangd",
        command: "clangd",
        args: &[],
        languages: &["c", "cpp"],
        roots: &["compile_commands.json", "compile_flags.txt", ".clangd", "CMakeLists.txt"],
    },
    Spec {
        name: "gopls",
        command: "gopls",
        args: &[],
        languages: &["go"],
        roots: &["go.mod", "go.work"],
    },
    Spec {
        name: "pyright",
        command: "pyright-langserver",
        args: &["--stdio"],
        languages: &["python"],
        roots: &["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"],
    },
    Spec {
        name: "pylsp",
        command: "pylsp",
        args: &[],
        languages: &["python"],
        roots: &["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"],
    },
    Spec {
        name: "typescript-language-server",
        command: "typescript-language-server",
        args: &["--stdio"],
        languages: &["javascript"],
        roots: &["package.json", "tsconfig.json", "jsconfig.json"],
    },
];

/// The server for a language, if one is installed.
pub fn spec_for(language: &str) -> Option<&'static Spec> {
    SERVERS
        .iter()
        .filter(|spec| spec.languages.contains(&language))
        .find(|spec| on_path(spec.command))
}

fn on_path(command: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path).any(|dir| dir.join(command).is_file())
}

/// Where a server for `file` should be started: the nearest directory above it
/// holding one of the spec's markers, else the nearest repository, else the
/// file's own directory.
pub fn root_for(spec: &Spec, file: &Path) -> PathBuf {
    let start = file.parent().unwrap_or(Path::new("."));
    let nearest = |markers: &[&str]| {
        start.ancestors().find(|dir| markers.iter().any(|marker| dir.join(marker).exists()))
    };
    nearest(spec.roots).or_else(|| nearest(&[".git"])).unwrap_or(start).to_path_buf()
}

/// How the server counts columns. The protocol's default is UTF-16 code units;
/// a server that offers UTF-8 is taken up on it, since that is cheaper to
/// convert and not every server gets surrogate pairs right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Utf8,
    Utf16,
}

/// A char offset in `rope` as the protocol's (line, character).
pub fn to_position(rope: &Rope, at: usize, encoding: Encoding) -> (u32, u32) {
    let at = at.min(rope.len_chars());
    let line = rope.char_to_line(at);
    let start = rope.line_to_char(line);
    let character: usize = rope
        .slice(start..at)
        .chars()
        .map(|c| match encoding {
            Encoding::Utf8 => c.len_utf8(),
            Encoding::Utf16 => c.len_utf16(),
        })
        .sum();
    (line as u32, character as u32)
}

/// The protocol's (line, character) as a char offset in `rope`, clamped to the
/// line: a position past the end of a line - which servers do send, for a
/// range that covers the line break - lands at its end.
pub fn from_position(rope: &Rope, line: u32, character: u32, encoding: Encoding) -> usize {
    let line = line as usize;
    if line >= rope.len_lines() {
        return rope.len_chars();
    }
    let start = rope.line_to_char(line);
    let mut units = 0usize;
    let mut offset = 0usize;
    for c in rope.line(line).chars() {
        if units >= character as usize || c == '\n' || c == '\r' {
            break;
        }
        units += match encoding {
            Encoding::Utf8 => c.len_utf8(),
            Encoding::Utf16 => c.len_utf16(),
        };
        offset += 1;
    }
    start + offset
}

/// `file://` and the absolute path, with anything but the plain characters of
/// a path percent-encoded.
pub fn uri(path: &Path) -> String {
    let absolute = path
        .canonicalize()
        .unwrap_or_else(|_| std::env::current_dir().map(|dir| dir.join(path)).unwrap_or(path.into()));
    encode(&absolute.to_string_lossy())
}

fn encode(path: &str) -> String {
    let mut out = String::from("file://");
    for byte in path.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

/// The path a `file://` URI names, or `None` for any other scheme.
pub fn path_of(uri: &str) -> Option<PathBuf> {
    let encoded = uri.strip_prefix("file://")?.as_bytes();
    let mut bytes = Vec::with_capacity(encoded.len());
    let mut index = 0;
    while index < encoded.len() {
        let hex = encoded.get(index + 1..index + 3).and_then(|pair| std::str::from_utf8(pair).ok());
        match (encoded[index], hex.and_then(|pair| u8::from_str_radix(pair, 16).ok())) {
            (b'%', Some(byte)) => {
                bytes.push(byte);
                index += 3;
            }
            (byte, _) => {
                bytes.push(byte);
                index += 1;
            }
        }
    }
    Some(PathBuf::from(String::from_utf8(bytes).ok()?))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    fn from_number(number: Option<u64>) -> Severity {
        match number {
            Some(2) => Severity::Warning,
            Some(3) => Severity::Info,
            Some(4) => Severity::Hint,
            // Unset is an error, as the protocol leaves it to the client and an
            // unlabelled complaint is safest taken seriously.
            _ => Severity::Error,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        }
    }
}

/// A diagnostic as it came: positions still in the server's terms, since only
/// the buffer it is for can turn them into offsets.
#[derive(Clone, Debug, PartialEq)]
pub struct RawDiagnostic {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub severity: Severity,
    pub message: String,
    /// The whole thing as it arrived, to be handed back when asking what
    /// could be done about it.
    pub raw: Value,
}

/// A place in a file, as a definition answer has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub position: (u32, u32),
}

/// An inlay hint as the server sends it: a position in its own encoding, and
/// the text, padding included.
#[derive(Clone, Debug, PartialEq)]
pub struct RawHint {
    pub position: (u32, u32),
    pub label: String,
}

/// A name a server knows of somewhere in the project.
#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: &'static str,
    /// What it is inside - the type of a method, the module of a function -
    /// when the server says.
    pub container: Option<String>,
    pub location: Location,
}

/// One thing a server offers to finish a word with.
#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub text: String,
    /// The item's kind, as the popup spells kinds: `fn`, `var`, `type`.
    pub kind: Option<&'static str>,
}

/// A call's signature as the server wrote it, and which parameter of it the
/// cursor is in - as a range of chars in the label, since that is what has to
/// be pointed at on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    pub label: String,
    pub active: Option<std::ops::Range<usize>>,
    /// Which of several overloads this is, and how many there are: `1/3`.
    pub index: usize,
    pub count: usize,
}

/// A piece of a buffer a server wants replaced: positions still in its terms,
/// since only the buffer they are for can turn them into offsets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEdit {
    pub start: (u32, u32),
    pub end: (u32, u32),
    pub text: String,
}

/// Everything a server wants changed, over however many files: a rename
/// touches every use of the name, and most of them are not the file you are
/// looking at. The edits for one file are in the order the server sent them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceEdit {
    pub changes: Vec<(PathBuf, Vec<TextEdit>)>,
}

impl WorkspaceEdit {
    pub fn is_empty(&self) -> bool {
        self.changes.iter().all(|(_, edits)| edits.is_empty())
    }
}

/// One thing a server offers to do about the place the cursor is in: a quick
/// fix for a diagnostic, an import to add, a refactor.
///
/// It carries either the edits themselves, a command for the server to run, or
/// neither - in which case it is a title and a promise, and asking for the
/// rest of it is a second request. Servers send the cheap shape by default
/// because working out every fix on the chance one is wanted is expensive.
#[derive(Clone, Debug, PartialEq)]
pub struct CodeAction {
    pub title: String,
    pub edit: Option<WorkspaceEdit>,
    /// `{ command, arguments }`, run by `workspace/executeCommand`.
    pub command: Option<Value>,
    /// The action as it arrived, which is what `codeAction/resolve` wants back
    /// when neither of the two above is filled in.
    pub raw: Value,
}

impl CodeAction {
    /// True when there is nothing here to do yet: the server sent a title and
    /// is waiting to be asked for the rest.
    pub fn needs_resolving(&self) -> bool {
        self.edit.is_none() && self.command.is_none()
    }
}

/// What a request was for, so the answer can be taken to the right place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Initialize,
    /// `gd`: the buffer and cursor it was asked from, and how many edits the
    /// buffer had had, so an answer that arrives after you have moved on is
    /// dropped rather than yanking you back.
    Definition { view: usize, head: usize, edits: u64 },
    /// `K`: what the server says about the thing under the cursor. Dropped
    /// the same way a definition is when you have moved on since.
    Hover { view: usize, head: usize, edits: u64 },
    /// The call being typed, and where it was asked from - the bracket, or
    /// the comma. Unlike the rest, what has been typed since does not make the
    /// answer stale: typing arguments is exactly what happens while it is in
    /// flight, and the signature is for the call, not for a character in it.
    Signature { view: usize, head: usize },
    /// `:fmt`: the whole buffer, or the lines a range named. `edits` is what
    /// the buffer had had when it was asked, because a formatting answer is a
    /// list of positions and applying it to text that has moved on would
    /// scramble the file rather than tidy it.
    Format { view: usize, edits: u64 },
    /// `gr`: every use of the name under the cursor. Stale the moment the
    /// cursor moves - the answer is a list about one name, and by then it
    /// would be a list about a different one.
    References { view: usize, head: usize, edits: u64 },
    /// `gR`: the name under the cursor, everywhere, replaced. What it is being
    /// renamed to travelled with the request and comes back in the edits.
    Rename { view: usize, head: usize, edits: u64 },
    /// `ga`: what could be done about the place the cursor is in - the
    /// diagnostics under it, most often.
    CodeAction { view: usize, head: usize, edits: u64 },
    /// A command the server offered, handed back to it to run.
    Execute,
    /// The rest of a code action, asked for once one has been chosen.
    ResolveAction { view: usize, edits: u64 },
    /// The popup asked what the server would offer. `start` is where the word
    /// being completed begins, which is what says whether the answer is still
    /// about the same word - typing more of it since is fine and expected,
    /// since a longer prefix only filters what came back.
    Completion { view: usize, start: usize },
    /// `<space>S`: names across the whole project matching what has been
    /// typed. `token` is the picker's search it answers; a search typed since
    /// has retired it.
    WorkspaceSymbol { token: u64 },
    /// The hints for a whole buffer, as it was after `edits` edits.
    InlayHint { view: usize, edits: u64 },
}

/// What a message from a server comes to.
#[derive(Debug, PartialEq)]
pub enum Event {
    Nothing,
    Ready,
    Diagnostics { path: PathBuf, diagnostics: Vec<RawDiagnostic> },
    Definition { request: Request, locations: Vec<Location> },
    Completion { request: Request, items: Vec<Suggestion> },
    /// What the server says about the thing under the cursor, still in its
    /// markdown. `None` when it had nothing to say about it.
    Hover { request: Request, markup: Option<String> },
    Signature { request: Request, help: Option<Signature> },
    Format { request: Request, edits: Vec<TextEdit> },
    References { request: Request, locations: Vec<Location> },
    CodeActions { request: Request, actions: Vec<CodeAction> },
    ResolvedAction { request: Request, action: Option<CodeAction> },
    /// The server asking for an edit to be made, rather than answering with
    /// one: what `workspace/executeCommand` usually comes back as. It is a
    /// request, so it wants an answer - `id` is what to answer.
    ApplyEdit { id: Value, edit: WorkspaceEdit },
    Rename { request: Request, edit: WorkspaceEdit },
    WorkspaceSymbols { request: Request, symbols: Vec<Symbol> },
    InlayHints { request: Request, hints: Vec<RawHint> },
    /// The server's hints have changed without the text changing - it has
    /// finished indexing, most often - and every buffer should ask again.
    HintsStale,
    HintsFailed { request: Request },
    /// Something the server wanted said: an error it could not recover from.
    Say(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Starting,
    Ready,
    Exited,
}

/// One running server.
pub struct Client {
    pub name: &'static str,
    pub root: PathBuf,
    pub state: State,
    pub encoding: Encoding,
    capabilities: Value,
    child: Option<Child>,
    writer: Sender<Vec<u8>>,
    next_id: u64,
    pending: HashMap<u64, Request>,
    /// Notifications sent before the server has answered `initialize`, which
    /// it is not allowed to be sent.
    queued: Vec<Vec<u8>>,
    /// Work the server says it is doing - indexing, mostly - by token: what
    /// it is called, and how far along, when the server says.
    progress: HashMap<String, (String, Option<u64>)>,
}

impl Client {
    /// Start `spec` in `root`. `index` is how its messages will be labelled on
    /// the channel.
    pub fn spawn(spec: &'static Spec, root: PathBuf, index: usize, tx: Sender<Message>) -> Result<Client> {
        let mut child = Command::new(spec.command)
            .args(spec.args)
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", spec.command))?;
        let stdout = child.stdout.take().ok_or_else(|| anyhow!("{} has no stdout", spec.name))?;
        let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("{} has no stdin", spec.name))?;

        thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            while let Ok(Some(body)) = read_frame(&mut reader) {
                let Ok(value) = serde_json::from_slice::<Value>(&body) else {
                    continue;
                };
                if tx.send(Message::Lsp { server: index, message: Some(value) }).is_err() {
                    return;
                }
            }
            let _ = tx.send(Message::Lsp { server: index, message: None });
        });

        let (writer, outgoing) = channel::<Vec<u8>>();
        thread::spawn(move || {
            for bytes in outgoing {
                if stdin.write_all(&bytes).and_then(|_| stdin.flush()).is_err() {
                    return;
                }
            }
        });

        let mut client = Client {
            name: spec.name,
            root,
            state: State::Starting,
            encoding: Encoding::Utf16,
            capabilities: Value::Null,
            child: Some(child),
            writer,
            next_id: 1,
            pending: HashMap::new(),
            queued: Vec::new(),
            progress: HashMap::new(),
        };
        client.initialize();
        Ok(client)
    }

    /// A client with no process behind it, and the bytes it would have
    /// written: for tests that play the server's part by hand.
    #[cfg(test)]
    pub fn detached(name: &'static str) -> (Client, std::sync::mpsc::Receiver<Vec<u8>>) {
        let (writer, written) = channel();
        let client = Client {
            name,
            root: PathBuf::from("/"),
            state: State::Starting,
            encoding: Encoding::Utf16,
            capabilities: Value::Null,
            child: None,
            writer,
            next_id: 1,
            pending: HashMap::new(),
            queued: Vec::new(),
            progress: HashMap::new(),
        };
        (client, written)
    }

    /// Straight to ready, as if `initialize` had been answered with these.
    #[cfg(test)]
    pub fn ready_with(&mut self, capabilities: Value) {
        self.capabilities = capabilities;
        self.state = State::Ready;
    }

    fn initialize(&mut self) {
        let root = uri(&self.root);
        let name = self.root.file_name().map_or("root".into(), |n| n.to_string_lossy().into_owned());
        let params = json!({
            "processId": std::process::id(),
            "clientInfo": { "name": "jack", "version": env!("CARGO_PKG_VERSION") },
            "rootUri": root,
            "workspaceFolders": [{ "uri": root, "name": name }],
            "capabilities": {
                "general": { "positionEncodings": ["utf-8", "utf-16"] },
                "textDocument": {
                    "synchronization": { "didSave": true },
                    "publishDiagnostics": { "versionSupport": false },
                    "definition": { "linkSupport": true },
                    "hover": { "contentFormat": ["markdown", "plaintext"] },
                    "formatting": { "dynamicRegistration": false },
                    "references": { "dynamicRegistration": false },
                    "inlayHint": { "dynamicRegistration": false },
                    "codeAction": {
                        "dynamicRegistration": false,
                        // Without this a server may answer only with bare
                        // commands, or with nothing: it is how a client says
                        // it understands a code action as an object rather
                        // than as a command to run.
                        "codeActionLiteralSupport": {
                            "codeActionKind": {
                                "valueSet": [
                                    "", "quickfix", "refactor", "refactor.extract",
                                    "refactor.inline", "refactor.rewrite", "source",
                                    "source.organizeImports", "source.fixAll",
                                ],
                            },
                        },
                        "isPreferredSupport": true,
                        // Every kind, and resolved in a second request when
                        // the server would rather not work them all out up
                        // front - which is every server worth asking.
                        "resolveSupport": { "properties": ["edit", "command"] },
                        "dataSupport": true,
                    },
                    // No `prepareSupport`: the answer to "can this be renamed"
                    // is the rename failing, which it says anyway.
                    "rename": { "dynamicRegistration": false, "prepareSupport": false },
                    "rangeFormatting": { "dynamicRegistration": false },
                    "signatureHelp": {
                        "contextSupport": true,
                        "signatureInformation": {
                            "documentationFormat": ["plaintext"],
                            // The offsets, rather than the parameter's label
                            // repeated as text: a signature with the same name
                            // twice in it cannot be marked by searching.
                            "parameterInformation": { "labelOffsetSupport": true },
                            "activeParameterSupport": true,
                        },
                    },
                    // Without this a server may decide the client is too
                    // simple to be worth completing properly: pyright answers
                    // a bare keyword list rather than what is on the thing
                    // before the dot. Snippets are declined on purpose - a
                    // template with holes in it is not something to paste
                    // into a buffer.
                    "completion": {
                        "contextSupport": true,
                        "completionItem": {
                            "snippetSupport": false,
                            "commitCharactersSupport": false,
                            "documentationFormat": ["plaintext"],
                            "deprecatedSupport": false,
                            "preselectSupport": false,
                            "insertReplaceSupport": false,
                        },
                        "completionItemKind": {
                            "valueSet": (1..=25).collect::<Vec<u8>>(),
                        },
                    },
                },
                "window": { "workDoneProgress": true },
                "workspace": { "configuration": true, "workspaceFolders": true, "symbol": { "dynamicRegistration": false }, "inlayHint": { "refreshSupport": true } },
            },
        });
        let id = self.take_id(Request::Initialize);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": "initialize", "params": params }));
    }

    fn take_id(&mut self, request: Request) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.pending.insert(id, request);
        id
    }

    fn write(&self, message: &Value) {
        let _ = self.writer.send(frame(message));
    }

    /// A notification, held back until the server is ready for it.
    /// Stop waiting on answers about buffers by index, once a buffer has
    /// closed and the indexes have moved. The answers are ignored on arrival.
    pub fn forget_about_buffers(&mut self) {
        self.pending.retain(|_, request| matches!(request, Request::Initialize));
    }

    /// The characters this server wants to be asked after - `.` for Python,
    /// and often `(` or `:` elsewhere. Empty when it named none, which means
    /// a word is the only thing worth asking about.
    pub fn completion_triggers(&self) -> Vec<char> {
        self.triggers("completionProvider", "triggerCharacters")
    }

    /// The characters that open a signature - `(` and `,` nearly everywhere -
    /// and the ones that mean the same call has moved on to another argument.
    pub fn signature_triggers(&self) -> Vec<char> {
        let mut chars = self.triggers("signatureHelpProvider", "triggerCharacters");
        chars.extend(self.triggers("signatureHelpProvider", "retriggerCharacters"));
        chars
    }

    fn triggers(&self, provider: &str, field: &str) -> Vec<char> {
        self.capabilities[provider][field]
            .as_array()
            .map(|list| {
                list.iter()
                    .filter_map(Value::as_str)
                    .filter_map(|text| text.chars().next())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Answer a request the server made. Only `workspace/applyEdit` needs
    /// this: the rest are answered where they arrive.
    pub fn respond(&self, id: Value, result: Value) {
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    }

    pub fn notify(&mut self, method: &str, params: Value) {
        let bytes = frame(&json!({ "jsonrpc": "2.0", "method": method, "params": params }));
        match self.state {
            State::Starting => self.queued.push(bytes),
            State::Ready => {
                let _ = self.writer.send(bytes);
            }
            State::Exited => {}
        }
    }

    /// A request, if the server is ready to take one and says it can answer
    /// `capability`. `false` means ask something else.
    pub fn request(&mut self, method: &str, capability: &str, params: Value, request: Request) -> bool {
        if self.state != State::Ready || !self.can(capability) {
            return false;
        }
        let id = self.take_id(request);
        self.write(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        true
    }

    /// Whether the server takes changes as ranges rather than whole texts:
    /// `textDocumentSync` is the kind, or an object with the kind as `change`.
    pub fn incremental(&self) -> bool {
        let sync = &self.capabilities["textDocumentSync"];
        sync.as_u64().or_else(|| sync["change"].as_u64()) == Some(2)
    }

    /// Whether the server's capabilities name `capability` as anything but
    /// absent or `false` - it may be `true` or an object of options.
    pub fn can(&self, capability: &str) -> bool {
        !matches!(self.capabilities.get(capability), None | Some(Value::Null) | Some(Value::Bool(false)))
    }

    /// What the server says it is busy with, for the status line.
    pub fn busy(&self) -> Option<String> {
        let (title, percent) = self.progress.values().next()?;
        Some(match percent {
            Some(percent) => format!("{title} {percent}%"),
            None => title.clone(),
        })
    }

    /// Make sense of one message from the server.
    pub fn handle(&mut self, message: Value) -> Event {
        let id = message.get("id").cloned();
        let method = message.get("method").and_then(Value::as_str).map(str::to_string);
        match (id, method) {
            // A request from the server. The ones that need an answer to keep
            // it going get the least that satisfies them.
            (Some(id), Some(method)) => {
                // An edit is the one request that is not answered here: the
                // editor has to make it first, and only it can say whether it
                // could be made.
                if method == "workspace/applyEdit" {
                    return Event::ApplyEdit { id, edit: workspace_edit(&message["params"]["edit"]) };
                }
                if method == "workspace/inlayHint/refresh" {
                    self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": null }));
                    return Event::HintsStale;
                }
                let result = match method.as_str() {
                    "workspace/configuration" => {
                        let items = message["params"]["items"].as_array().map_or(0, Vec::len);
                        Value::Array(vec![Value::Null; items])
                    }
                    _ => Value::Null,
                };
                self.write(&json!({ "jsonrpc": "2.0", "id": id, "result": result }));
                Event::Nothing
            }
            (Some(id), None) => self.response(id, message),
            (None, Some(method)) => self.notification(&method, &message["params"]),
            (None, None) => Event::Nothing,
        }
    }

    fn response(&mut self, id: Value, message: Value) -> Event {
        let Some(request) = id.as_u64().and_then(|id| self.pending.remove(&id)) else {
            return Event::Nothing;
        };
        let result = message.get("result").cloned().unwrap_or(Value::Null);
        match request {
            Request::Initialize => {
                if let Some(error) = message.get("error") {
                    self.state = State::Exited;
                    return Event::Say(format!("{} would not start: {}", self.name, error["message"]));
                }
                self.capabilities = result["capabilities"].clone();
                self.encoding = match self.capabilities["positionEncoding"].as_str() {
                    Some("utf-8") => Encoding::Utf8,
                    _ => Encoding::Utf16,
                };
                self.state = State::Ready;
                self.write(&json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }));
                for bytes in std::mem::take(&mut self.queued) {
                    let _ = self.writer.send(bytes);
                }
                Event::Ready
            }
            Request::Definition { .. } => Event::Definition { request, locations: locations(&result) },
            Request::Completion { .. } => Event::Completion { request, items: suggestions(&result) },
            Request::Hover { .. } => Event::Hover { request, markup: hover_markup(&result) },
            Request::Format { .. } => Event::Format { request, edits: text_edits(&result) },
            Request::Signature { .. } => Event::Signature { request, help: signature(&result) },
            Request::References { .. } => Event::References { request, locations: locations(&result) },
            Request::Rename { .. } => Event::Rename { request, edit: workspace_edit(&result) },
            Request::CodeAction { .. } => Event::CodeActions { request, actions: code_actions(&result) },
            Request::ResolveAction { .. } => Event::ResolvedAction { request, action: code_action(&result) },
            // Nothing comes back from a command worth acting on: what it does
            // arrives as a `workspace/applyEdit` of its own.
            Request::Execute => Event::Nothing,
            Request::WorkspaceSymbol { .. } => Event::WorkspaceSymbols { request, symbols: symbols(&result) },
            // Busy indexing, most often, which a server says as "content
            // modified": worth asking again once it has something to say.
            Request::InlayHint { .. } if message.get("error").is_some() => Event::HintsFailed { request },
            Request::InlayHint { .. } => Event::InlayHints { request, hints: inlay_hints(&result) },
        }
    }

    fn notification(&mut self, method: &str, params: &Value) -> Event {
        match method {
            "textDocument/publishDiagnostics" => {
                let Some(path) = params["uri"].as_str().and_then(path_of) else {
                    return Event::Nothing;
                };
                let diagnostics = params["diagnostics"]
                    .as_array()
                    .map(|list| list.iter().filter_map(diagnostic).collect())
                    .unwrap_or_default();
                Event::Diagnostics { path, diagnostics }
            }
            "$/progress" => {
                let token = params["token"].to_string();
                let value = &params["value"];
                let percent = value["percentage"].as_u64();
                match value["kind"].as_str() {
                    Some("begin") => {
                        let title = value["title"].as_str().unwrap_or("working").to_string();
                        self.progress.insert(token, (title, percent));
                    }
                    Some("report") => {
                        if let Some(entry) = self.progress.get_mut(&token) {
                            entry.1 = percent.or(entry.1);
                        }
                    }
                    Some("end") => {
                        self.progress.remove(&token);
                    }
                    _ => {}
                }
                Event::Nothing
            }
            // Type 1 is an error. The rest is chatter nobody asked for.
            "window/showMessage" if params["type"].as_u64() == Some(1) => {
                Event::Say(format!("{}: {}", self.name, params["message"].as_str().unwrap_or("")))
            }
            _ => Event::Nothing,
        }
    }

    pub fn exited(&mut self) {
        self.state = State::Exited;
        self.progress.clear();
        self.pending.clear();
    }
}

impl Drop for Client {
    /// A server outliving the editor would go on indexing for nobody.
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn frame(message: &Value) -> Vec<u8> {
    let body = message.to_string();
    let mut bytes = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body.as_bytes());
    bytes
}

/// One message's body: headers up to a blank line, then as many bytes as
/// `Content-Length` said. `None` at the end of the stream.
pub fn read_frame(reader: &mut impl BufRead) -> std::io::Result<Option<Vec<u8>>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim_end();
        if line.is_empty() {
            // A blank line before any header is noise between messages.
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.eq_ignore_ascii_case("content-length")
        {
            length = value.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length.unwrap_or(0)];
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

fn position(value: &Value) -> Option<(u32, u32)> {
    Some((value["line"].as_u64()? as u32, value["character"].as_u64()? as u32))
}

fn diagnostic(value: &Value) -> Option<RawDiagnostic> {
    Some(RawDiagnostic {
        start: position(&value["range"]["start"])?,
        end: position(&value["range"]["end"])?,
        severity: Severity::from_number(value["severity"].as_u64()),
        message: value["message"].as_str()?.to_string(),
        raw: value.clone(),
    })
}

/// A definition answer in any of the three shapes the protocol allows: one
/// location, a list of them, or a list of links.
fn locations(result: &Value) -> Vec<Location> {
    let one = |value: &Value| -> Option<Location> {
        // A link names both the whole definition and the name inside it; the
        // name is where the cursor belongs.
        let (uri, range) = match value.get("targetUri") {
            Some(uri) => (uri, &value["targetSelectionRange"]),
            None => (&value["uri"], &value["range"]),
        };
        Some(Location { path: path_of(uri.as_str()?)?, position: position(&range["start"])? })
    };
    match result {
        Value::Array(list) => list.iter().filter_map(one).collect(),
        Value::Object(_) => one(result).into_iter().collect(),
        _ => Vec::new(),
    }
}

/// What a server offers, in either shape the protocol allows: a bare list, or
/// a list with a flag saying it is only the start of one.
///
/// `sortText` is the server saying what order it meant - pyright puts the
/// members of the thing before the dot above everything else that way - so it
/// is what these are sorted by, falling back to the label.
fn suggestions(result: &Value) -> Vec<Suggestion> {
    let list = match result.get("items") {
        Some(items) => items.as_array(),
        None => result.as_array(),
    };
    let Some(list) = list else {
        return Vec::new();
    };
    let mut items: Vec<(String, Suggestion)> = list
        .iter()
        .filter_map(|item| {
            let label = item["label"].as_str()?.trim();
            // `insertText` is what to type when it differs from what to show -
            // except in snippet form, which is a template with holes in it and
            // not something to paste into a buffer. The label is the honest
            // fallback, cut at the bracket where a server has written a
            // signature into it.
            let snippet = item["insertTextFormat"].as_u64() == Some(2);
            let text = match item["insertText"].as_str() {
                Some(text) if !snippet => text.trim(),
                _ => label.split(['(', '<', ' ']).next().unwrap_or(label),
            };
            if text.is_empty() {
                return None;
            }
            let sort = item["sortText"].as_str().unwrap_or(label).to_string();
            Some((sort, Suggestion { text: text.to_string(), kind: kind_of(item["kind"].as_u64()) }))
        })
        .collect();
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items.into_iter().map(|(_, item)| item).collect()
}

/// What a formatting answer asks for: a list of replacements, or nothing when
/// the server has left the file as it is.
fn text_edits(result: &Value) -> Vec<TextEdit> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|edit| {
            Some(TextEdit {
                start: position(&edit["range"]["start"])?,
                end: position(&edit["range"]["end"])?,
                text: edit["newText"].as_str()?.to_string(),
            })
        })
        .collect()
}

/// What a server wants changed, in either shape the protocol allows: a
/// `changes` map of uri to edits, or `documentChanges`, which is the same list
/// with a document version attached to each file - and may also carry creates,
/// renames and deletions of whole files, which are skipped. A rename that
/// needs a file moved is one jack will not do quietly.
fn workspace_edit(result: &Value) -> WorkspaceEdit {
    let mut changes: Vec<(PathBuf, Vec<TextEdit>)> = Vec::new();
    let mut add = |uri: Option<&str>, edits: &Value| {
        let Some(path) = uri.and_then(path_of) else {
            return;
        };
        let edits = text_edits(edits);
        if edits.is_empty() {
            return;
        }
        match changes.iter_mut().find(|(known, _)| *known == path) {
            Some((_, known)) => known.extend(edits),
            None => changes.push((path, edits)),
        }
    };

    if let Some(map) = result["changes"].as_object() {
        for (uri, edits) in map {
            add(Some(uri.as_str()), edits);
        }
    }
    if let Some(list) = result["documentChanges"].as_array() {
        for change in list {
            // A `kind` is a create, rename or delete of a file rather than an
            // edit to one, and has no `edits` to take.
            if change.get("kind").is_some() {
                continue;
            }
            add(change["textDocument"]["uri"].as_str(), &change["edits"]);
        }
    }
    changes.sort_by(|a, b| a.0.cmp(&b.0));
    WorkspaceEdit { changes }
}

/// What a server offers to do, out of the list it answered with. A list may
/// hold bare commands as well as code actions - the older shape - and both
/// are a title and something to do.
fn code_actions(result: &Value) -> Vec<CodeAction> {
    result.as_array().map(|list| list.iter().filter_map(code_action).collect()).unwrap_or_default()
}

fn code_action(value: &Value) -> Option<CodeAction> {
    let title = value["title"].as_str()?.to_string();
    // A bare command has its name in `command` as a string; a code action's
    // `command` is an object of one, when it has one at all.
    let command = match &value["command"] {
        Value::Object(_) => Some(value["command"].clone()),
        Value::String(_) => Some(value.clone()),
        _ => None,
    };
    let edit = value.get("edit").map(workspace_edit).filter(|edit| !edit.is_empty());
    Some(CodeAction { title, edit, command, raw: value.clone() })
}

/// A hover answer's text, in any of the shapes the protocol has collected over
/// the years: the markup object it has now, the string it used to be, the
/// `{language, value}` pair it used to be before that, and a list of any of
/// those.
fn hover_markup(result: &Value) -> Option<String> {
    fn one(value: &Value) -> Option<String> {
        match value {
            Value::String(text) => Some(text.clone()),
            // `kind` is markdown or plaintext; both are rendered the same way,
            // which is to say down to lines of text.
            Value::Object(_) => value["value"].as_str().map(str::to_string),
            _ => None,
        }
    }
    let contents = result.get("contents")?;
    let text = match contents {
        Value::Array(list) => {
            list.iter().filter_map(one).collect::<Vec<String>>().join("\n\n")
        }
        other => one(other)?,
    };
    (!text.trim().is_empty()).then_some(text)
}

/// The signature the cursor is in, out of however many the server sent.
///
/// `activeParameter` may be on the signature or on the answer as a whole, and
/// the newer of the two wins where a server sends both. A parameter names
/// itself either as offsets into the label or as the text of it - the offsets
/// being the only one that works when the same name appears twice.
fn signature(result: &Value) -> Option<Signature> {
    let signatures = result.get("signatures")?.as_array()?;
    let count = signatures.len();
    let index = (result["activeSignature"].as_u64().unwrap_or(0) as usize).min(count.saturating_sub(1));
    let chosen = signatures.get(index)?;
    let label = chosen["label"].as_str()?.to_string();

    let active = chosen["activeParameter"]
        .as_u64()
        .or_else(|| result["activeParameter"].as_u64())
        .map(|active| active as usize);
    let parameter = active.and_then(|active| chosen["parameters"].as_array()?.get(active).cloned());
    let range = parameter.and_then(|parameter| match &parameter["label"] {
        Value::Array(pair) => {
            let start = pair.first()?.as_u64()? as usize;
            let end = pair.get(1)?.as_u64()? as usize;
            // The offsets are counted the way the server counts columns.
            Some(units_to_chars(&label, start)..units_to_chars(&label, end))
        }
        Value::String(text) => {
            let at = label.find(text.as_str())?;
            let start = label[..at].chars().count();
            Some(start..start + text.chars().count())
        }
        _ => None,
    });

    Some(Signature { label, active: range, index, count })
}

/// A UTF-16 offset into `text` as a char offset, which is what a range of it
/// has to be to be drawn. Past the end clamps to the end.
fn units_to_chars(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (chars, c) in text.chars().enumerate() {
        if seen >= units {
            return chars;
        }
        seen += c.len_utf16();
    }
    text.chars().count()
}

/// The answer to `textDocument/inlayHint`. A label is a string or a list of
/// parts to be joined; padding is a space on the side the server asks for,
/// so `x: i32` reads as that and not `x:i32`.
fn inlay_hints(result: &Value) -> Vec<RawHint> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|value| {
            let position = position(&value["position"])?;
            let text = match &value["label"] {
                Value::String(text) => text.clone(),
                Value::Array(parts) => parts.iter().filter_map(|part| part["value"].as_str()).collect(),
                _ => return None,
            };
            // One line of it: a hint is drawn inside a line.
            let text = text.lines().next().unwrap_or_default().to_string();
            if text.is_empty() {
                return None;
            }
            let left = if value["paddingLeft"].as_bool() == Some(true) { " " } else { "" };
            let right = if value["paddingRight"].as_bool() == Some(true) { " " } else { "" };
            Some(RawHint { position, label: format!("{left}{text}{right}") })
        })
        .collect()
}

/// The answer to `workspace/symbol`, in either shape: `SymbolInformation`,
/// which always has a range, or `WorkspaceSymbol`, whose location may be only
/// a file. A file alone is taken to mean its top.
fn symbols(result: &Value) -> Vec<Symbol> {
    let Some(list) = result.as_array() else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|value| {
            let location = &value["location"];
            let path = path_of(location["uri"].as_str()?)?;
            let position = position(&location["range"]["start"]).unwrap_or((0, 0));
            Some(Symbol {
                name: value["name"].as_str()?.to_string(),
                kind: symbol_kind(value["kind"].as_u64()),
                container: value["containerName"].as_str().filter(|name| !name.is_empty()).map(str::to_string),
                location: Location { path, position },
            })
        })
        .collect()
}

/// `SymbolKind`, in a word.
fn symbol_kind(number: Option<u64>) -> &'static str {
    match number.unwrap_or(0) {
        1 => "file",
        2..=4 => "mod",
        5 => "class",
        6 => "method",
        7 | 8 => "field",
        9 => "new",
        10 => "enum",
        11 => "trait",
        12 => "fn",
        13 => "var",
        14 => "const",
        22 => "variant",
        23 => "struct",
        26 => "type",
        _ => "",
    }
}

/// `CompletionItemKind`, in the words the popup already uses for what the
/// grammar finds. The numbers are the protocol's, and the ones that would say
/// nothing useful in three letters are left unnamed.
fn kind_of(number: Option<u64>) -> Option<&'static str> {
    Some(match number? {
        2 | 3 => "fn",
        4 => "new",
        5 => "field",
        6 | 10 => "var",
        7 | 8 | 22 => "type",
        9 => "mod",
        14 => "kw",
        21 => "const",
        _ => return None,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Cursor;

    /// What was written, as the JSON of each message.
    pub fn sent(written: &std::sync::mpsc::Receiver<Vec<u8>>) -> Vec<Value> {
        written
            .try_iter()
            .map(|bytes| {
                let body = read_frame(&mut Cursor::new(bytes)).unwrap().unwrap();
                serde_json::from_slice(&body).unwrap()
            })
            .collect()
    }

    #[test]
    fn nothing_but_initialize_goes_out_until_the_server_has_answered_it() {
        let (mut client, written) = Client::detached("fake");
        client.initialize();
        client.notify("textDocument/didOpen", json!({ "n": 1 }));
        let first = sent(&written);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["method"], "initialize");
        assert!(!client.request("textDocument/definition", "definitionProvider", json!({}), Request::Initialize));

        let answer = json!({ "jsonrpc": "2.0", "id": 1, "result": {
            "capabilities": { "positionEncoding": "utf-8", "definitionProvider": true },
        }});
        assert_eq!(client.handle(answer), Event::Ready);
        assert_eq!(client.encoding, Encoding::Utf8);
        let methods: Vec<Value> = sent(&written).into_iter().map(|m| m["method"].clone()).collect();
        assert_eq!(methods, ["initialized", "textDocument/didOpen"], "then the held notification");

        // A capability it has is asked; one it does not have is not.
        let request = Request::Definition { view: 0, head: 0, edits: 0 };
        assert!(client.request("textDocument/definition", "definitionProvider", json!({}), request));
        assert!(!client.request("textDocument/hover", "hoverProvider", json!({}), request));
    }

    #[test]
    fn requests_from_the_server_get_an_answer() {
        let (mut client, written) = Client::detached("fake");
        let asked = json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/configuration",
            "params": { "items": [{}, {}] } });
        assert_eq!(client.handle(asked), Event::Nothing);
        let answer = &sent(&written)[0];
        assert_eq!(answer["id"], 7);
        assert_eq!(answer["result"], json!([null, null]));
    }

    #[test]
    fn progress_is_what_the_server_is_busy_with_until_it_ends() {
        let (mut client, _written) = Client::detached("fake");
        let progress = |kind: &str, extra: Value| {
            let mut value = json!({ "kind": kind });
            value.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            json!({ "jsonrpc": "2.0", "method": "$/progress", "params": { "token": "t", "value": value } })
        };
        client.handle(progress("begin", json!({ "title": "Indexing" })));
        assert_eq!(client.busy().as_deref(), Some("Indexing"));
        client.handle(progress("report", json!({ "percentage": 40 })));
        assert_eq!(client.busy().as_deref(), Some("Indexing 40%"));
        client.handle(progress("end", json!({})));
        assert_eq!(client.busy(), None);
    }

    #[test]
    fn frames_are_read_by_their_length() {
        let a = r#"{"id":1}"#;
        let b = r#"{"method":"é"}"#;
        let stream = format!(
            "Content-Length: {}\r\nContent-Type: x\r\n\r\n{a}Content-Length: {}\r\n\r\n{b}",
            a.len(),
            b.len()
        );
        let mut reader = Cursor::new(stream.into_bytes());
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), a.as_bytes());
        assert_eq!(read_frame(&mut reader).unwrap().unwrap(), b.as_bytes());
        assert_eq!(read_frame(&mut reader).unwrap(), None);

        // And what goes out reads back in.
        let sent = frame(&json!({ "text": "żółw 🐢" }));
        let body = read_frame(&mut Cursor::new(sent)).unwrap().unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&body).unwrap()["text"], "żółw 🐢");
    }

    #[test]
    fn positions_count_in_the_servers_units() {
        let rope = Rope::from_str("ab\nż🐢x\n");
        let x = 3 + 2;
        assert_eq!(to_position(&rope, x, Encoding::Utf8), (1, 2 + 4));
        assert_eq!(to_position(&rope, x, Encoding::Utf16), (1, 1 + 2));
        for encoding in [Encoding::Utf8, Encoding::Utf16] {
            let (line, character) = to_position(&rope, x, encoding);
            assert_eq!(from_position(&rope, line, character, encoding), x);
        }
        // Past the end of the line is its end; past the end of the file, the end.
        assert_eq!(from_position(&rope, 0, 99, Encoding::Utf16), 2);
        assert_eq!(from_position(&rope, 9, 0, Encoding::Utf16), rope.len_chars());
    }

    #[test]
    fn uris_carry_paths_both_ways() {
        let path = PathBuf::from("/tmp/a dir/żółw.rs");
        let encoded = encode("/tmp/a dir/żółw.rs");
        assert!(encoded.starts_with("file:///tmp/a%20dir/%C5%BC"), "{encoded}");
        assert_eq!(path_of(&encoded), Some(path));
        assert_eq!(path_of("untitled:1"), None);
    }

    #[test]
    fn a_definition_answer_is_read_in_every_shape() {
        let location = json!({ "uri": "file:///a.rs", "range": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 9 } } });
        let expected = Location { path: "/a.rs".into(), position: (3, 4) };
        assert_eq!(locations(&location), std::slice::from_ref(&expected));
        assert_eq!(locations(&json!([location])), [expected]);

        let link = json!([{
            "targetUri": "file:///b.rs",
            "targetRange": { "start": { "line": 1, "character": 0 }, "end": { "line": 5, "character": 1 } },
            "targetSelectionRange": { "start": { "line": 1, "character": 3 }, "end": { "line": 1, "character": 6 } },
        }]);
        assert_eq!(locations(&link), [Location { path: "/b.rs".into(), position: (1, 3) }]);
        assert!(locations(&Value::Null).is_empty());
    }

    #[test]
    fn diagnostics_are_read_with_a_severity_whether_given_or_not() {
        let value = json!({
            "range": { "start": { "line": 0, "character": 1 }, "end": { "line": 0, "character": 4 } },
            "severity": 2,
            "message": "unused",
        });
        let read = diagnostic(&value).unwrap();
        assert_eq!(read.severity, Severity::Warning);
        assert_eq!((read.start, read.end), ((0, 1), (0, 4)));

        let mut unlabelled = value;
        unlabelled.as_object_mut().unwrap().remove("severity");
        assert_eq!(diagnostic(&unlabelled).unwrap().severity, Severity::Error);
    }

    #[test]
    fn a_hover_answer_is_read_in_every_shape_the_protocol_has_had() {
        let markup = json!({ "contents": { "kind": "markdown", "value": "```rust\nfn f()\n```" } });
        assert_eq!(hover_markup(&markup).as_deref(), Some("```rust\nfn f()\n```"));
        // The string it used to be, and the pair it used to be before that.
        assert_eq!(hover_markup(&json!({ "contents": "plain words" })).as_deref(), Some("plain words"));
        let pair = json!({ "contents": { "language": "rust", "value": "fn f()" } });
        assert_eq!(hover_markup(&pair).as_deref(), Some("fn f()"));
        // A list of them, joined into paragraphs.
        let list = json!({ "contents": ["one", { "value": "two" }] });
        assert_eq!(hover_markup(&list).as_deref(), Some("one\n\ntwo"));
        // Nothing to say says nothing, rather than an empty box.
        assert_eq!(hover_markup(&json!({ "contents": "  " })), None);
        assert_eq!(hover_markup(&Value::Null), None);
    }

    #[test]
    fn a_signature_marks_the_parameter_being_typed() {
        let help = json!({
            "signatures": [{
                "label": "f(a: int, b: str) -> None",
                "parameters": [{ "label": [2, 8] }, { "label": [10, 16] }],
            }],
            "activeSignature": 0,
            "activeParameter": 1,
        });
        let read = signature(&help).expect("a signature");
        assert_eq!(read.label, "f(a: int, b: str) -> None");
        let active = read.active.clone().expect("a parameter");
        assert_eq!(&read.label[active], "b: str");
        assert_eq!((read.index, read.count), (0, 1));
    }

    #[test]
    fn the_signature_the_cursor_is_in_is_the_one_read() {
        let one = |label: &str| json!({ "label": label, "parameters": [{ "label": "a" }] });
        let help = json!({
            "signatures": [one("f(a: int)"), one("f(a: str)")],
            "activeSignature": 1,
            "activeParameter": 0,
        });
        let read = signature(&help).expect("a signature");
        assert_eq!(read.label, "f(a: str)");
        assert_eq!((read.index, read.count), (1, 2));
        // A parameter named by its text rather than by offsets is found in it.
        assert_eq!(read.active, Some(2..3));

        // A server that says nothing, and one that points past what it sent.
        assert_eq!(signature(&json!({ "signatures": [] })), None);
        let past = json!({ "signatures": [one("f(a: int)")], "activeParameter": 7 });
        assert_eq!(signature(&past).expect("still a signature").active, None);
    }

    #[test]
    fn parameter_offsets_are_counted_the_way_the_protocol_counts_columns() {
        // Two chars, four UTF-16 units: the offsets after them are not the
        // columns they are at.
        let label = "f(🙂🙂, b)";
        assert_eq!(units_to_chars(label, 2), 2);
        assert_eq!(units_to_chars(label, 6), 4, "past both faces");
        assert_eq!(units_to_chars(label, 99), label.chars().count());
    }

    #[test]
    fn a_root_is_the_nearest_marker_above_the_file() {
        let dir = std::env::temp_dir().join(format!("jack_lsp_root_{}", std::process::id()));
        let nested = dir.join("crate/src/deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(dir.join("crate/Cargo.toml"), "").unwrap();
        let spec = &SERVERS[0];
        assert_eq!(root_for(spec, &nested.join("x.rs")), dir.join("crate"));
        // Nothing marked: the file's own directory.
        let bare = dir.join("bare");
        std::fs::create_dir_all(&bare).unwrap();
        assert_eq!(root_for(spec, &bare.join("x.rs")), bare);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn inlay_hints_join_their_parts_and_pad_where_asked() {
        let hints = inlay_hints(&json!([
            { "position": { "line": 0, "character": 5 }, "label": ": i32", "kind": 1 },
            { "position": { "line": 1, "character": 2 }, "label": [{ "value": "count" }, { "value": ":" }], "paddingRight": true },
            { "position": { "line": 2, "character": 0 }, "label": "" },
        ]));
        assert_eq!(hints, [
            RawHint { position: (0, 5), label: ": i32".into() },
            RawHint { position: (1, 2), label: "count: ".into() },
        ]);
    }

    #[test]
    fn a_server_asking_for_hints_again_is_answered_and_heard() {
        let (mut client, written) = Client::detached("fake");
        let asked = json!({ "jsonrpc": "2.0", "id": 3, "method": "workspace/inlayHint/refresh" });
        assert_eq!(client.handle(asked), Event::HintsStale);
        assert_eq!(sent(&written)[0]["id"], 3);
    }
}

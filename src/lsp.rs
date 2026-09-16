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
}

/// A place in a file, as a definition answer has it.
#[derive(Clone, Debug, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub position: (u32, u32),
}

/// What a request was for, so the answer can be taken to the right place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Initialize,
    /// `gd`: the buffer and cursor it was asked from, and how many edits the
    /// buffer had had, so an answer that arrives after you have moved on is
    /// dropped rather than yanking you back.
    Definition { view: usize, head: usize, edits: u64 },
}

/// What a message from a server comes to.
#[derive(Debug, PartialEq)]
pub enum Event {
    Nothing,
    Ready,
    Diagnostics { path: PathBuf, diagnostics: Vec<RawDiagnostic> },
    Definition { request: Request, locations: Vec<Location> },
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
                },
                "window": { "workDoneProgress": true },
                "workspace": { "configuration": true, "workspaceFolders": true },
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
        assert_eq!(locations(&location), [expected.clone()]);
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
}

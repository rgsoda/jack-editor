use anyhow::{Context, Result};
use ropey::Rope;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::Path;
use std::rc::Rc;
use streaming_iterator::StreamingIterator;
use tree_sitter::{
    InputEdit, Language, Node, Parser, Point, Query, QueryCursor, TextProvider, Tree,
};

use crate::buffer::Edit;
use crate::screen::Style;
use crate::theme::Theme;

/// How deep injections may nest: Rust in a macro body in a macro body. A limit
/// keeps a self-injecting grammar from recursing forever.
const MAX_DEPTH: usize = 3;

/// A language we know how to highlight.
pub struct LanguageConfig {
    pub name: &'static str,
    extensions: &'static [&'static str],
    language: fn() -> Language,
    /// The highlight queries, in the order the earliest pattern should win.
    /// A list because a language can be another language plus its own: C++'s
    /// query is the C++ half only, and means nothing without C's under it.
    highlights: &'static [&'static str],
    injections: &'static str,
    /// Written here rather than shipped by the grammar crates, which have no
    /// indent queries: what indents, and what comes back out.
    indents: &'static str,
    /// Where names are bound and how far the bindings reach, for `gd`. Some
    /// grammars ship one; Rust's does not, so ours is in `queries/`.
    locals: &'static str,
    /// The definitions a file makes - functions, types, methods. Every grammar
    /// crate ships this one, for `ctags`-style indexes; `gd` wants the same
    /// thing.
    tags: &'static str,
}

fn python_language() -> Language {
    tree_sitter_python::LANGUAGE.into()
}

fn toml_language() -> Language {
    tree_sitter_toml_ng::LANGUAGE.into()
}

fn go_language() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

fn java_language() -> Language {
    tree_sitter_java::LANGUAGE.into()
}

fn c_language() -> Language {
    tree_sitter_c::LANGUAGE.into()
}

fn cpp_language() -> Language {
    tree_sitter_cpp::LANGUAGE.into()
}

fn rust_language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

fn html_language() -> Language {
    tree_sitter_html::LANGUAGE.into()
}

fn javascript_language() -> Language {
    tree_sitter_javascript::LANGUAGE.into()
}

/// `name` is what an injection query refers to, so it has to match the names
/// other grammars use: HTML injects "javascript" and "css".
static LANGUAGES: &[LanguageConfig] = &[
    LanguageConfig {
        name: "rust",
        extensions: &["rs"],
        language: rust_language,
        highlights: &[tree_sitter_rust::HIGHLIGHTS_QUERY],
        injections: tree_sitter_rust::INJECTIONS_QUERY,
        indents: include_str!("../queries/rust/indents.scm"),
        locals: include_str!("../queries/rust/locals.scm"),
        tags: tree_sitter_rust::TAGS_QUERY,
    },
    LanguageConfig {
        name: "html",
        extensions: &["html", "htm"],
        language: html_language,
        highlights: &[tree_sitter_html::HIGHLIGHTS_QUERY],
        injections: tree_sitter_html::INJECTIONS_QUERY,
        indents: include_str!("../queries/html/indents.scm"),
        // A markup language binds no names and defines nothing: `gd` falls
        // back to looking for the word.
        locals: "",
        tags: "",
    },
    LanguageConfig {
        name: "javascript",
        extensions: &["js", "mjs", "cjs"],
        language: javascript_language,
        highlights: &[tree_sitter_javascript::HIGHLIGHT_QUERY],
        injections: tree_sitter_javascript::INJECTIONS_QUERY,
        indents: include_str!("../queries/javascript/indents.scm"),
        locals: tree_sitter_javascript::LOCALS_QUERY,
        tags: tree_sitter_javascript::TAGS_QUERY,
    },
    LanguageConfig {
        name: "python",
        extensions: &["py", "pyi"],
        language: python_language,
        highlights: &[tree_sitter_python::HIGHLIGHTS_QUERY],
        injections: "",
        indents: include_str!("../queries/python/indents.scm"),
        locals: "",
        tags: tree_sitter_python::TAGS_QUERY,
    },
    LanguageConfig {
        name: "toml",
        extensions: &["toml"],
        language: toml_language,
        highlights: &[tree_sitter_toml_ng::HIGHLIGHTS_QUERY],
        injections: "",
        indents: include_str!("../queries/toml/indents.scm"),
        // Nothing to bind and nothing to define: a key is not a definition
        // you can go to, it is the thing itself.
        locals: "",
        tags: "",
    },
    LanguageConfig {
        name: "go",
        extensions: &["go"],
        language: go_language,
        highlights: &[tree_sitter_go::HIGHLIGHTS_QUERY],
        injections: "",
        indents: include_str!("../queries/go/indents.scm"),
        locals: "",
        tags: tree_sitter_go::TAGS_QUERY,
    },
    LanguageConfig {
        name: "java",
        extensions: &["java"],
        language: java_language,
        highlights: &[tree_sitter_java::HIGHLIGHTS_QUERY],
        injections: "",
        indents: include_str!("../queries/java/indents.scm"),
        locals: "",
        tags: tree_sitter_java::TAGS_QUERY,
    },
    LanguageConfig {
        name: "c",
        extensions: &["c", "h"],
        language: c_language,
        highlights: &[tree_sitter_c::HIGHLIGHT_QUERY],
        injections: "",
        indents: include_str!("../queries/c/indents.scm"),
        locals: "",
        tags: tree_sitter_c::TAGS_QUERY,
    },
    LanguageConfig {
        name: "cpp",
        extensions: &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
        language: cpp_language,
        // C++ first, then C: the earliest pattern wins, and the C++ query is
        // only the half that C does not already say.
        highlights: &[tree_sitter_cpp::HIGHLIGHT_QUERY, tree_sitter_c::HIGHLIGHT_QUERY],
        injections: "",
        indents: include_str!("../queries/cpp/indents.scm"),
        locals: "",
        tags: tree_sitter_cpp::TAGS_QUERY,
    },
];

pub fn language_for_path(path: Option<&Path>) -> Option<&'static LanguageConfig> {
    let extension = path?.extension()?.to_str()?;
    LANGUAGES.iter().find(|l| l.extensions.contains(&extension))
}

fn language_by_name(name: &str) -> Option<&'static LanguageConfig> {
    LANGUAGES.iter().find(|l| l.name == name)
}

/// Everything about a language that is expensive to build, so it is compiled
/// once and shared by every layer using that language.
struct Compiled {
    language: Language,
    highlights: Query,
    injections: Option<Query>,
    indents: Option<Query>,
    locals: Option<Query>,
    tags: Option<Query>,
    /// Highlight capture index to style, resolved against the theme up front.
    capture_styles: Vec<Option<Style>>,
    /// Indices of the `@injection.content` and `@injection.language` captures.
    content_capture: Option<u32>,
    language_capture: Option<u32>,
    /// Indices of the `@indent` and `@outdent` captures.
    indent_capture: Option<u32>,
    outdent_capture: Option<u32>,
    /// Indices of the `@local.scope` and `@local.definition` captures, and of
    /// the tags query's `@name`.
    scope_capture: Option<u32>,
    definition_capture: Option<u32>,
    name_capture: Option<u32>,
}

fn compile(config: &LanguageConfig, theme: &Theme) -> Result<Rc<Compiled>> {
    let language = (config.language)();
    let source = config.highlights.join("\n");
    let highlights = Query::new(&language, &source)
        .with_context(|| format!("compiling {} highlight query", config.name))?;
    let capture_styles = highlights
        .capture_names()
        .iter()
        .map(|name| theme.has(name).then(|| theme.style(name)))
        .collect();

    let injections = match config.injections.trim().is_empty() {
        true => None,
        false => Some(
            Query::new(&language, config.injections)
                .with_context(|| format!("compiling {} injection query", config.name))?,
        ),
    };
    let capture = |query: &Query, name: &str| query.capture_index_for_name(name);
    let (content_capture, language_capture) = match &injections {
        Some(query) => (
            capture(query, "injection.content"),
            capture(query, "injection.language"),
        ),
        None => (None, None),
    };

    let indents = match config.indents.trim().is_empty() {
        true => None,
        false => Some(
            Query::new(&language, config.indents)
                .with_context(|| format!("compiling {} indent query", config.name))?,
        ),
    };
    let (indent_capture, outdent_capture) = match &indents {
        Some(query) => (capture(query, "indent"), capture(query, "outdent")),
        None => (None, None),
    };

    let compile_query = |source: &'static str, what: &str| -> Result<Option<Query>> {
        match source.trim().is_empty() {
            true => Ok(None),
            false => Ok(Some(
                Query::new(&language, source)
                    .with_context(|| format!("compiling {} {what} query", config.name))?,
            )),
        }
    };
    let locals = compile_query(config.locals, "locals")?;
    let tags = compile_query(config.tags, "tags")?;
    let (scope_capture, definition_capture) = match &locals {
        Some(query) => (capture(query, "local.scope"), capture(query, "local.definition")),
        None => (None, None),
    };
    let name_capture = tags.as_ref().and_then(|query| capture(query, "name"));

    Ok(Rc::new(Compiled {
        language,
        highlights,
        injections,
        indents,
        locals,
        tags,
        capture_styles,
        content_capture,
        language_capture,
        indent_capture,
        outdent_capture,
        scope_capture,
        definition_capture,
        name_capture,
    }))
}

/// What a tags query calls a definition, in the words a one-line list has room
/// for. The names come from `tags.scm`, which every grammar spells the same
/// way because they are all built for the same index.
fn short_kind(kind: &str) -> &'static str {
    match kind {
        "function" => "fn",
        "method" => "method",
        "class" | "struct" | "type" => "type",
        "interface" | "trait" => "trait",
        "module" => "mod",
        "macro" => "macro",
        "constant" => "const",
        "field" => "field",
        _ => "def",
    }
}

/// Whether a node's text is exactly `name`. Borrowed from the rope where it
/// can be: a name is one line, but the query runs over the whole file.
fn text_is(rope: &Rope, node: Node, name: &str) -> bool {
    let slice = rope.byte_slice(node.byte_range());
    if slice.len_bytes() != name.len() {
        return false;
    }
    match slice.as_str() {
        Some(text) => text == name,
        None => slice == name,
    }
}

/// What a highlight capture says the thing it named is, in the words the
/// completion popup has room for. Captures that name syntax rather than a
/// thing - punctuation, keywords, comments - have no kind and are not offered.
fn kind_for(capture: &str) -> Option<&'static str> {
    let kind = match capture.split('.').next()? {
        "function" | "method" => "fn",
        "type" | "constructor" => "type",
        "constant" => "const",
        "variable" => match capture.starts_with("variable.parameter") {
            true => "param",
            false => "var",
        },
        "property" | "field" => "field",
        "module" | "namespace" => "mod",
        "attribute" => "attr",
        "label" => "label",
        _ => return None,
    };
    Some(kind)
}

/// The parts of a highlight pass that do not change as injections recurse.
struct Frame<'a> {
    rope: &'a Rope,
    range: &'a Range<usize>,
    theme: &'a Theme,
}

pub struct Syntax {
    root: Rc<Compiled>,
    parser: Parser,
    tree: Tree,
    /// Compiled languages, including ones we failed to find, so an unknown
    /// injection language is looked up once rather than on every frame.
    compiled: RefCell<HashMap<String, Option<Rc<Compiled>>>>,
    /// Reused for injected regions, which are parsed on demand.
    scratch: RefCell<Parser>,
}

impl Syntax {
    pub fn new(config: &'static LanguageConfig, rope: &Rope, theme: &Theme) -> Result<Self> {
        let root = compile(config, theme)?;
        let mut parser = Parser::new();
        parser
            .set_language(&root.language)
            .with_context(|| format!("setting language {}", config.name))?;
        let tree = parse(&mut parser, rope, None).context("parsing buffer")?;

        let mut compiled = HashMap::new();
        compiled.insert(config.name.to_string(), Some(root.clone()));

        Ok(Syntax {
            root,
            parser,
            tree,
            compiled: RefCell::new(compiled),
            scratch: RefCell::new(Parser::new()),
        })
    }

    /// Patch the tree with the edits that were just applied, then reparse.
    /// Tree-sitter reuses every subtree the edits did not touch, so this costs
    /// roughly the size of the change rather than the size of the file.
    pub fn edit(&mut self, edits: &[Edit], rope: &Rope) {
        for edit in edits {
            self.tree.edit(&InputEdit {
                start_byte: edit.start_byte,
                old_end_byte: edit.old_end_byte,
                new_end_byte: edit.new_end_byte,
                start_position: point(edit.start_point),
                old_end_position: point(edit.old_end_point),
                new_end_position: point(edit.new_end_point),
            });
        }

        if let Some(tree) = parse(&mut self.parser, rope, Some(&self.tree)) {
            self.tree = tree;
        }
    }

    /// Styles for one byte range, normally just the visible rows.
    pub fn highlights(&self, rope: &Rope, range: Range<usize>, theme: &Theme) -> Highlights {
        let mut styles = vec![None; range.len()];
        let frame = Frame { rope, range: &range, theme };
        self.paint(&frame, &self.root, self.tree.root_node(), &mut styles, 0);
        Highlights { start: range.start, styles }
    }

    /// Identifier-shaped words the highlight query can name, with what kind of
    /// thing each is. Only the root language: an injected region's names are
    /// not what you are typing when you ask for a completion.
    pub fn identifiers(&self, rope: &Rope, range: Range<usize>) -> Vec<(Range<usize>, &'static str)> {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);

        let mut found = Vec::new();
        let names = self.root.highlights.capture_names();
        let mut matches =
            cursor.captures(&self.root.highlights, self.tree.root_node(), RopeProvider(rope));
        while let Some((m, index)) = matches.next() {
            let capture = m.captures()[*index];
            let Some(kind) = kind_for(names[capture.index as usize]) else {
                continue;
            };
            found.push((capture.node.byte_range(), kind));
        }
        found
    }

    /// Where `name` is defined, as a byte offset, read from the cursor at
    /// `at`. `local` is `gd`, which looks for a binding in scope before it
    /// looks at what the file defines; `gD` skips straight to the second.
    ///
    /// Two tiers, and neither of them is a language server: the first knows
    /// about `let`, parameters and match arms, the second about functions,
    /// types and methods. Neither looks at another file, and neither knows
    /// about types - `gd` on a method call finds a method with that name, not
    /// the one for the receiver's type. That is where a real index begins, and
    /// this is what is worth having before one.
    pub fn definition(&self, rope: &Rope, at: usize, name: &str, local: bool) -> Option<usize> {
        // A binding can only be in scope from inside the item that holds it,
        // so the first tier reads that item and not the file. It is what makes
        // `gd` on a local instant in a file where reading the whole tree is
        // tens of milliseconds.
        if local
            && let Some(item) = self.enclosing_item(at)
            && let Some(found) = self.local_definition(rope, at, name, item)
        {
            return Some(found);
        }
        if let Some(found) = self.tagged_definition(rope, at, name) {
            return Some(found);
        }
        // Left over: a binding at the top level of the file, which is to say a
        // `const` or a `static`. No tags query names those, and they are the
        // one kind of binding the first tier cannot have seen.
        match local {
            true => self.local_definition(rope, at, name, 0..rope.len_bytes()),
            false => None,
        }
    }

    /// Everything the file defines, in the order it defines it: the name, what
    /// kind of thing it is, and where it starts. The symbol picker's list.
    ///
    /// The same tags query `gd` reads, asked for all of it rather than for one
    /// name. `@definition.function` and friends give the kind; the `@name`
    /// capture gives the name and the place to jump to.
    pub fn definitions(&self, rope: &Rope) -> Vec<(String, &'static str, usize)> {
        let Some(query) = self.root.tags.as_ref() else {
            return Vec::new();
        };
        let Some(capture_index) = self.root.name_capture else {
            return Vec::new();
        };
        let names = query.capture_names();

        let mut found = Vec::new();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), RopeProvider(rope));
        while let Some(m) = matches.next() {
            let captures = m.captures();
            let Some(kind) = captures
                .iter()
                .find_map(|c| names[c.index as usize].strip_prefix("definition."))
            else {
                continue;
            };
            let Some(node) = captures.iter().find(|c| c.index == capture_index).map(|c| c.node)
            else {
                continue;
            };
            found.push((
                rope.byte_slice(node.byte_range()).to_string(),
                short_kind(kind),
                node.start_byte(),
            ));
        }
        // Query order is not file order, and a list of what a file holds is
        // only readable in the order it holds it.
        found.sort_by_key(|(_, _, start)| *start);
        found.dedup_by_key(|(_, _, start)| *start);
        found
    }

    /// The byte range of the top-level item holding `at` - the function,
    /// `impl` or `mod` that a binding in scope has to be inside.
    fn enclosing_item(&self, at: usize) -> Option<Range<usize>> {
        let root = self.tree.root_node();
        let end = (at + 1).min(root.end_byte());
        let mut node = root.descendant_for_byte_range(at, end)?;
        while let Some(parent) = node.parent() {
            if parent.id() == root.id() {
                return Some(node.byte_range());
            }
            node = parent;
        }
        None
    }

    /// A binding of `name` in scope at `at`: the innermost one wins, which is
    /// what makes a shadowed name resolve to the shadow and a parameter lose
    /// to a `let` of the same name further in.
    fn local_definition(
        &self,
        rope: &Rope,
        at: usize,
        name: &str,
        range: Range<usize>,
    ) -> Option<usize> {
        let query = self.root.locals.as_ref()?;
        let (scope, definition) = (self.root.scope_capture?, self.root.definition_capture?);

        let mut scopes: HashSet<usize> = HashSet::new();
        let mut found: Vec<(usize, usize)> = Vec::new();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range);
        let mut matches = cursor.captures(query, self.tree.root_node(), RopeProvider(rope));
        while let Some((m, index)) = matches.next() {
            let capture = m.captures()[*index];
            if capture.index == scope {
                scopes.insert(capture.node.id());
            } else if capture.index == definition && text_is(rope, capture.node, name) {
                found.push((capture.node.id(), capture.node.start_byte()));
            }
        }
        if found.is_empty() {
            return None;
        }

        // The scopes around the cursor, innermost first. The file itself is
        // the last of them, for grammars whose locals query does not name the
        // root - JavaScript's does not.
        let root = self.tree.root_node();
        let end = (at + 1).min(root.end_byte());
        let mut chain: Vec<usize> = Vec::new();
        let mut node = root.descendant_for_byte_range(at, end);
        while let Some(current) = node {
            if scopes.contains(&current.id()) {
                chain.push(current.id());
            }
            node = current.parent();
        }
        chain.push(root.id());

        for scope in chain {
            let mut best: Option<usize> = None;
            for &(id, start) in &found {
                if self.scope_of(id, start, &scopes) != Some(scope) {
                    continue;
                }
                // The last binding before the cursor, because a rebinding
                // shadows the one above it. A binding *after* the cursor only
                // counts when there is none before - `gd` on a name used above
                // its `let` should still find it.
                let better = match best {
                    None => true,
                    Some(current) => match (current < at, start < at) {
                        (true, true) => start > current,
                        (false, false) => start < current,
                        (had, _) => !had,
                    },
                };
                if better {
                    best = Some(start);
                }
            }
            if best.is_some() {
                return best;
            }
        }
        None
    }

    /// Which scope a binding belongs to: the nearest one above it.
    fn scope_of(&self, id: usize, start: usize, scopes: &HashSet<usize>) -> Option<usize> {
        let root = self.tree.root_node();
        let end = (start + 1).min(root.end_byte());
        let mut node = root.descendant_for_byte_range(start, end);
        // Walk up to the captured node itself first, then on to its scope.
        while let Some(current) = node {
            if current.id() != id && scopes.contains(&current.id()) {
                return Some(current.id());
            }
            node = current.parent();
        }
        Some(root.id())
    }

    /// What the file itself defines, from the tags query every grammar crate
    /// ships. Nearest to the cursor wins, so a method defined in the impl you
    /// are reading beats one of the same name further off.
    ///
    /// A tags query names call sites as well as definitions - it is built for
    /// an index that cross-references both - so a match only counts when it
    /// also carries a `@definition.*` capture. Without that check `gd` lands
    /// on the call it was pressed over.
    fn tagged_definition(&self, rope: &Rope, at: usize, name: &str) -> Option<usize> {
        let query = self.root.tags.as_ref()?;
        let capture_index = self.root.name_capture?;
        let names = query.capture_names();

        let mut best: Option<usize> = None;
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, self.tree.root_node(), RopeProvider(rope));
        while let Some(m) = matches.next() {
            let captures = m.captures();
            if !captures.iter().any(|c| names[c.index as usize].starts_with("definition")) {
                continue;
            }
            let Some(node) = captures
                .iter()
                .find(|c| c.index == capture_index)
                .map(|c| c.node)
                .filter(|node| text_is(rope, *node, name))
            else {
                continue;
            };
            let start = node.start_byte();
            if best.is_none_or(|current| at.abs_diff(start) < at.abs_diff(current)) {
                best = Some(start);
            }
        }
        best
    }

    /// Whether the byte sits inside a comment or a string. Completion suggests
    /// itself while you type, and prose is the one place where offering to
    /// finish every word in the file is noise rather than help.
    ///
    /// By node kind rather than by query, so it works for every grammar we
    /// have without another file each: every one of them spells these
    /// `line_comment`, `block_comment`, `string_literal`, `template_string`.
    pub fn in_comment_or_string(&self, byte: usize) -> bool {
        let end = (byte + 1).min(self.tree.root_node().end_byte());
        let mut node = self.tree.root_node().descendant_for_byte_range(byte, end);
        while let Some(current) = node {
            let kind = current.kind();
            if kind.contains("comment") || kind.contains("string") || kind.contains("char_literal")
            {
                return true;
            }
            node = current.parent();
        }
        false
    }

    /// Whether the byte sits anywhere inside a node tree-sitter could not
    /// make sense of.
    fn inside_error(&self, byte: usize) -> bool {
        let mut node = self.tree.root_node().descendant_for_byte_range(byte, byte);
        while let Some(current) = node {
            if current.is_error() {
                return true;
            }
            node = current.parent();
        }
        false
    }

    pub fn has_indent_rules(&self) -> bool {
        self.root.indents.is_some()
    }

    /// How many steps of indentation the line covering `line` (a byte range)
    /// deserves, reading the tree at `at`. `None` when the grammar has nothing
    /// to say: no indent query, or a tree too broken here to trust.
    ///
    /// Every `@indent` ancestor that started on an earlier line is a step; an
    /// `@outdent` node starting on this line takes one back, which is what
    /// puts a closing brace under the thing it closes.
    pub fn indent_level(&self, rope: &Rope, at: usize, line: Range<usize>) -> Option<usize> {
        let query = self.root.indents.as_ref()?;
        let (indent, outdent) = (self.root.indent_capture?, self.root.outdent_capture);

        // One query run, restricted to the line: tree-sitter returns every
        // match that *overlaps* that range, which is exactly the enclosing
        // blocks plus whatever starts on the line itself.
        let mut indents: Vec<usize> = Vec::new();
        let mut outdents: Vec<usize> = Vec::new();
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(line);
        let mut matches = cursor.captures(query, self.tree.root_node(), RopeProvider(rope));
        while let Some((m, index)) = matches.next() {
            let capture = m.captures()[*index];
            if capture.index == indent {
                indents.push(capture.node.id());
            } else if Some(capture.index) == outdent {
                outdents.push(capture.node.id());
            }
        }

        // What decides this line's indent is the context before it, so that is
        // what has to have parsed. A half-typed block is an ERROR node with
        // the `{` loose inside it rather than a block at all - and that is
        // exactly the moment you are asking.
        let before = rope
            .bytes_at(at)
            .reversed()
            .position(|b| !b.is_ascii_whitespace())
            .map(|back| at - back - 1);
        if self.inside_error(at) || before.is_some_and(|byte| self.inside_error(byte)) {
            return None;
        }

        let row = rope.byte_to_line(at);
        // One byte wide, not zero: an empty range on a token boundary picks
        // the node that *contains* it, and the closing brace we need to see is
        // the one that starts there.
        let end = (at + 1).min(rope.len_bytes());
        let mut node = self.tree.root_node().descendant_for_byte_range(at, end)?;
        // Counted separately and subtracted at the end: the walk meets the
        // closing brace before the blocks that put it there, so taking one off
        // as we go would take it off nothing.
        let (mut steps, mut back) = (0usize, 0usize);
        loop {
            let starts_here = node.start_position().row == row;
            if !starts_here && indents.contains(&node.id()) {
                steps += 1;
            }
            if starts_here && outdents.contains(&node.id()) {
                back += 1;
            }
            match node.parent() {
                Some(parent) => node = parent,
                None => break,
            }
        }
        Some(steps.saturating_sub(back))
    }

    fn paint(
        &self,
        frame: &Frame,
        compiled: &Compiled,
        node: Node,
        styles: &mut [Option<Style>],
        depth: usize,
    ) {
        let (rope, range, theme) = (frame.rope, frame.range, frame.theme);
        self.paint_captures(compiled, node, rope, range, styles);

        if depth >= MAX_DEPTH {
            return;
        }
        for (language, ranges) in self.injections(compiled, node, rope, range) {
            let Some(child) = self.compiled_for(&language, theme) else {
                continue;
            };

            let tree = {
                let mut parser = self.scratch.borrow_mut();
                if parser.set_language(&child.language).is_err()
                    || parser.set_included_ranges(&ranges).is_err()
                {
                    continue;
                }
                let tree = parse(&mut parser, rope, None);
                // Leave the scratch parser unrestricted for the next caller.
                let _ = parser.set_included_ranges(&[]);
                tree
            };

            // An injected layer paints over its host, so the inner language's
            // idea of a token wins inside the injected region.
            if let Some(tree) = tree {
                self.paint(frame, &child, tree.root_node(), styles, depth + 1);
            }
        }
    }

    fn paint_captures(
        &self,
        compiled: &Compiled,
        node: Node,
        rope: &Rope,
        range: &Range<usize>,
        styles: &mut [Option<Style>],
    ) {
        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());

        let mut captures: Vec<(usize, usize, usize, usize)> = Vec::new();
        let mut matches = cursor.captures(&compiled.highlights, node, RopeProvider(rope));
        let mut order = 0;
        while let Some((m, index)) = matches.next() {
            let capture = m.captures()[*index];
            captures.push((
                capture.node.start_byte(),
                capture.node.end_byte(),
                capture.index as usize,
                order,
            ));
            order += 1;
        }

        // Paint widest first so a nested capture overrides its container; among
        // equal spans paint later query patterns first, so the earliest pattern
        // in the query wins, which is what highlight queries assume.
        captures.sort_by_key(|&(start, end, _, order)| (Reverse(end - start), Reverse(order)));

        for (start, end, capture, _) in captures {
            let Some(style) = compiled.capture_styles[capture] else {
                continue;
            };
            if end <= range.start || start >= range.end {
                continue;
            }
            let lo = start.max(range.start) - range.start;
            let hi = end.min(range.end) - range.start;
            for slot in &mut styles[lo..hi] {
                *slot = Some(style);
            }
        }
    }

    /// Injected regions of `node` that overlap `range`, as (language, ranges).
    fn injections(
        &self,
        compiled: &Compiled,
        node: Node,
        rope: &Rope,
        range: &Range<usize>,
    ) -> Vec<(String, Vec<tree_sitter::Range>)> {
        let (Some(query), Some(content_capture)) = (&compiled.injections, compiled.content_capture)
        else {
            return Vec::new();
        };

        let mut cursor = QueryCursor::new();
        cursor.set_byte_range(range.clone());

        let mut found = Vec::new();
        let mut matches = cursor.matches(query, node, RopeProvider(rope));
        while let Some(m) = matches.next() {
            let properties = query.property_settings(m.pattern_index);
            let include_children = properties
                .iter()
                .any(|p| &*p.key == "injection.include-children");

            // The language is either fixed by the pattern or read out of the
            // document, as in a fenced code block that names its language.
            let language = properties
                .iter()
                .find(|p| &*p.key == "injection.language")
                .and_then(|p| p.value.as_ref().map(|v| v.to_string()))
                .or_else(|| {
                    let capture = compiled.language_capture?;
                    let node = m.nodes_for_capture_index(capture).next()?;
                    Some(rope.byte_slice(node.byte_range()).to_string())
                });
            let Some(language) = language else { continue };

            for node in m.nodes_for_capture_index(content_capture) {
                let ranges = content_ranges(node, include_children);
                if ranges
                    .iter()
                    .any(|r| r.end_byte > range.start && r.start_byte < range.end)
                {
                    found.push((language.clone(), ranges));
                }
            }
        }
        found
    }

    fn compiled_for(&self, name: &str, theme: &Theme) -> Option<Rc<Compiled>> {
        if let Some(entry) = self.compiled.borrow().get(name) {
            return entry.clone();
        }
        let entry = language_by_name(name).and_then(|config| compile(config, theme).ok());
        self.compiled
            .borrow_mut()
            .insert(name.to_string(), entry.clone());
        entry
    }

}

/// The byte ranges an injection covers. Without `include-children`, the named
/// children are holes: a fenced code block injects the code but not the fence.
fn content_ranges(node: Node, include_children: bool) -> Vec<tree_sitter::Range> {
    if include_children {
        return vec![node.range()];
    }

    let mut ranges = Vec::new();
    let mut start_byte = node.start_byte();
    let mut start_point = node.start_position();
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        if child.start_byte() > start_byte {
            ranges.push(tree_sitter::Range {
                start_byte,
                end_byte: child.start_byte(),
                start_point,
                end_point: child.start_position(),
            });
        }
        start_byte = child.end_byte();
        start_point = child.end_position();
    }
    if node.end_byte() > start_byte {
        ranges.push(tree_sitter::Range {
            start_byte,
            end_byte: node.end_byte(),
            start_point,
            end_point: node.end_position(),
        });
    }
    ranges
}

/// Styles for a byte range, indexed by absolute byte offset.
pub struct Highlights {
    start: usize,
    styles: Vec<Option<Style>>,
}

impl Highlights {
    pub fn none() -> Self {
        Highlights { start: 0, styles: Vec::new() }
    }

    pub fn style_at(&self, byte: usize) -> Option<Style> {
        self.styles.get(byte.checked_sub(self.start)?).copied().flatten()
    }
}

/// Hands tree-sitter rope chunks so query predicates never copy the document.
struct RopeProvider<'a>(&'a Rope);

impl<'a> TextProvider<&'a [u8]> for RopeProvider<'a> {
    type I = std::iter::Map<ropey::iter::Chunks<'a>, fn(&str) -> &[u8]>;

    fn text(&mut self, node: Node) -> Self::I {
        self.0.byte_slice(node.byte_range()).chunks().map(str::as_bytes)
    }
}

fn parse(parser: &mut Parser, rope: &Rope, old: Option<&Tree>) -> Option<Tree> {
    let mut callback = |byte: usize, _: Point| -> &[u8] {
        if byte >= rope.len_bytes() {
            return &[];
        }
        let (chunk, chunk_start, _, _) = rope.chunk_at_byte(byte);
        &chunk.as_bytes()[byte - chunk_start..]
    };
    parser.parse_with_options(&mut callback, old, None)
}

fn point((row, column): (usize, usize)) -> Point {
    Point { row, column }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Document;
    use crossterm::style::Color;

    struct Fixture {
        doc: Document,
        syntax: Syntax,
        theme: Theme,
    }

    impl Fixture {
        fn new(text: &str) -> Self {
            Fixture::with_language("rust", text)
        }

        fn with_language(language: &str, text: &str) -> Self {
            let mut doc = Document::scratch();
            doc.text = Rope::from_str(text);
            let theme = Theme::built_in();
            let config = language_by_name(language).expect("unregistered language");
            let syntax = Syntax::new(config, &doc.text, &theme).unwrap();
            Fixture { doc, syntax, theme }
        }

        fn color_of(&self, needle: &str) -> Option<Color> {
            let text = self.doc.text.to_string();
            let at = text.find(needle).expect("needle not in document");
            self.syntax
                .highlights(&self.doc.text, 0..self.doc.len_bytes(), &self.theme)
                .style_at(at)
                .and_then(|style| style.fg)
        }

        fn insert(&mut self, char_pos: usize, text: &str) {
            let edit = self.doc.replace(char_pos, 0, text);
            self.syntax.edit(&[edit], &self.doc.text);
        }
    }

    #[test]
    fn keywords_types_and_strings_get_their_colors() {
        let f = Fixture::new("fn main() { let s: String = \"hi\"; }\n");
        assert_eq!(f.color_of("fn"), Some(Color::Magenta));
        assert_eq!(f.color_of("String"), Some(Color::Yellow));
        assert_eq!(f.color_of("\"hi\""), Some(Color::Green));
    }

    #[test]
    fn comments_are_dimmed_and_win_over_the_code_inside_them() {
        let f = Fixture::new("// fn main\nfn main() {}\n");
        assert_eq!(f.color_of("// fn"), Some(Color::DarkGrey));
        assert_eq!(f.color_of("fn main() {}"), Some(Color::Magenta));
    }

    #[test]
    fn every_language_compiles_every_query_it_ships() {
        // A query is only checked when it is compiled, and a node name that
        // the grammar does not have is a typo the test suite should find
        // rather than the first person to open such a file.
        let theme = Theme::built_in();
        for config in LANGUAGES {
            compile(config, &theme)
                .unwrap_or_else(|err| panic!("{}: {err:#}", config.name));
        }
    }

    #[test]
    fn every_language_highlights_and_indents_something() {
        // One shape per language: a word that has to be coloured, and a line
        // the grammar has to want indented one step. Between them they say the
        // highlight query matched and the indent query named a real node.
        let cases: &[(&str, &str, &str)] = &[
            ("rust", "fn main() {\n    let x = 1;\n}\n", "fn"),
            ("javascript", "function f() {\n    let x = 1;\n}\n", "function"),
            ("python", "def f():\n    x = 1\n", "def"),
            ("toml", "key = [\n    1,\n]\n", "key"),
            ("go", "func main() {\n    x := 1\n}\n", "func"),
            ("java", "class A {\n    int x = 1;\n}\n", "class"),
            ("c", "int main(void) {\n    int x = 1;\n}\n", "int"),
            ("cpp", "namespace n {\n    int x = 1;\n}\n", "namespace"),
        ];
        for (language, text, word) in cases {
            let f = Fixture::with_language(language, text);
            assert!(f.color_of(word).is_some(), "{language}: {word} is coloured");

            // The second line, which every snippet has indented by one step.
            let start = f.doc.text.line_to_byte(1);
            let end = f.doc.text.line_to_byte(2);
            let level = f.syntax.indent_level(&f.doc.text, start, start..end);
            assert_eq!(level, Some(1), "{language}: one step on line two");
        }
    }

    #[test]
    fn cpp_gets_cs_highlighting_as_well_as_its_own() {
        // The C++ query is only the half C does not already say, so a plain C
        // keyword in a C++ file is the check that both halves are in.
        let f = Fixture::with_language("cpp", "int main() { return 0; }\n");
        assert!(f.color_of("return").is_some(), "C's keywords are in");
        let f = Fixture::with_language("cpp", "template <class T> T id(T x) { return x; }\n");
        assert!(f.color_of("template").is_some(), "and C++'s own");
    }

    #[test]
    fn highlights_survive_an_edit_that_shifts_later_lines() {
        let mut f = Fixture::new("fn main() {}\n");
        f.insert(0, "// note\n");
        assert_eq!(f.color_of("// note"), Some(Color::DarkGrey));
        assert_eq!(f.color_of("fn"), Some(Color::Magenta));
        assert_eq!(f.color_of("main"), Some(Color::Blue));
    }

    #[test]
    fn reparsing_picks_up_a_newly_opened_string() {
        let mut f = Fixture::new("fn main() { let s = xy; }\n");
        assert_ne!(f.color_of("xy"), Some(Color::Green));

        let at = f.doc.text.to_string().find("xy").unwrap();
        f.insert(at, "\"");
        f.insert(at + 3, "\"");
        assert_eq!(f.color_of("\"xy\""), Some(Color::Green));
    }

    #[test]
    fn extensions_map_to_languages() {
        let name = |path: &str| language_for_path(Some(Path::new(path))).map(|c| c.name);
        assert_eq!(name("main.rs"), Some("rust"));
        assert_eq!(name("index.html"), Some("html"));
        assert_eq!(name("app.mjs"), Some("javascript"));
        assert_eq!(name("notes.txt"), None);
        assert!(language_for_path(None).is_none());
    }

    #[test]
    fn every_registered_language_compiles() {
        // A grammar whose query does not compile would otherwise only show up
        // the first time someone opens that kind of file.
        let theme = Theme::built_in();
        for config in LANGUAGES {
            compile(config, &theme)
                .unwrap_or_else(|err| panic!("{} failed to compile: {err:#}", config.name));
        }
    }

    #[test]
    fn a_language_is_compiled_once_and_shared() {
        // Compiling a highlight query costs milliseconds, and injections are
        // resolved on every frame. A page with many <script> tags must compile
        // JavaScript once, not once per tag per frame.
        let f = Fixture::with_language("html", "<script>const a = 1;</script>\n");
        let first = f.syntax.compiled_for("javascript", &f.theme).unwrap();
        let second = f.syntax.compiled_for("javascript", &f.theme).unwrap();
        assert!(Rc::ptr_eq(&first, &second));
    }

    #[test]
    fn html_highlights_its_own_tags() {
        let f = Fixture::with_language("html", "<div class=\"a\">hi</div>\n");
        assert_eq!(f.color_of("div"), Some(Color::Blue));
        assert_eq!(f.color_of("class"), Some(Color::DarkYellow));
    }

    #[test]
    fn javascript_inside_a_script_tag_is_highlighted_as_javascript() {
        // The real cross-language case: HTML's injection query hands the
        // script body to a different grammar entirely.
        let f = Fixture::with_language(
            "html",
            "<body>\n<script>\nconst total = 1;\n</script>\n</body>\n",
        );
        assert_eq!(f.color_of("const"), Some(Color::Magenta));
        assert_eq!(f.color_of("1;"), Some(Color::Cyan));
        // The surrounding HTML still highlights as HTML.
        assert_eq!(f.color_of("body"), Some(Color::Blue));
    }

    #[test]
    fn an_injection_into_an_unregistered_language_is_skipped() {
        // HTML injects "css" into <style>, and we have no CSS grammar. The
        // page around it must still highlight.
        let f = Fixture::with_language(
            "html",
            "<style>\n.a { color: red; }\n</style>\n<div>hi</div>\n",
        );
        assert_eq!(f.color_of("div"), Some(Color::Blue));
        assert_eq!(f.color_of("style"), Some(Color::Blue));
    }

    #[test]
    fn a_tagged_template_injects_the_language_named_in_the_document() {
        // JavaScript's injection query reads the language from an
        // @injection.language capture rather than fixing it in the pattern.
        let f = Fixture::with_language("javascript", "const page = html`<div>hi</div>`;\n");
        assert_eq!(f.color_of("div"), Some(Color::Blue));
        assert_eq!(f.color_of("const"), Some(Color::Magenta));
    }

    #[test]
    fn a_macro_body_is_highlighted_as_real_code() {
        // The root grammar parses a macro's token tree as loose tokens, so it
        // sees `new` as a bare identifier and leaves it unstyled. Only the
        // injected Rust layer parses `Foo::new(1)` as a call.
        let f = Fixture::new("fn main() { let v = vec![Foo::new(1)]; }\n");
        assert_eq!(f.color_of("new"), Some(Color::Blue));
    }

    #[test]
    fn injections_nest() {
        // A macro inside a macro: the inner body needs a second layer.
        let f = Fixture::new("fn main() { outer!(inner!(Foo::new(1))); }\n");
        assert_eq!(f.color_of("new"), Some(Color::Blue));
    }

    #[test]
    fn nesting_past_the_depth_limit_terminates() {
        // Rust injects Rust, so without a limit this recurses forever.
        let mut text = String::from("fn main() { ");
        for _ in 0..8 {
            text.push_str("m!(");
        }
        text.push('x');
        for _ in 0..8 {
            text.push(')');
        }
        text.push_str("; }\n");

        let f = Fixture::new(&text);
        assert_eq!(f.color_of("fn"), Some(Color::Magenta));
    }

    #[test]
    fn an_unknown_injection_language_is_skipped() {
        let f = Fixture::new("fn main() {}\n");
        assert!(f.syntax.compiled_for("cobol", &f.theme).is_none());
        // Looked up once and remembered, so it costs nothing on later frames.
        assert!(f.syntax.compiled_for("cobol", &f.theme).is_none());
        assert!(f.syntax.compiled_for("rust", &f.theme).is_some());
    }

    #[test]
    fn content_ranges_can_exclude_the_children() {
        let text = "fn a() {}\n\nfn b() {}\n";
        let f = Fixture::new(text);
        let root = f.syntax.tree.root_node();

        // With children included it is one range covering everything.
        let whole = content_ranges(root, true);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].start_byte, 0);

        // Without, the named children are holes, so only the whitespace
        // between the two functions is left.
        let gaps = content_ranges(root, false);
        let covered: String = gaps
            .iter()
            .map(|r| &text[r.start_byte..r.end_byte])
            .collect();
        assert!(!gaps.is_empty());
        assert!(
            covered.chars().all(char::is_whitespace),
            "expected only the gaps between items, got {covered:?}"
        );
    }
}



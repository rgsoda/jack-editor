use anyhow::{Context, Result};
use ropey::Rope;
use std::cell::RefCell;
use std::cmp::Reverse;
use std::collections::HashMap;
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
    highlights: &'static str,
    injections: &'static str,
    /// Written here rather than shipped by the grammar crates, which have no
    /// indent queries: what indents, and what comes back out.
    indents: &'static str,
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
        highlights: tree_sitter_rust::HIGHLIGHTS_QUERY,
        injections: tree_sitter_rust::INJECTIONS_QUERY,
        indents: include_str!("../queries/rust/indents.scm"),
    },
    LanguageConfig {
        name: "html",
        extensions: &["html", "htm"],
        language: html_language,
        highlights: tree_sitter_html::HIGHLIGHTS_QUERY,
        injections: tree_sitter_html::INJECTIONS_QUERY,
        indents: include_str!("../queries/html/indents.scm"),
    },
    LanguageConfig {
        name: "javascript",
        extensions: &["js", "mjs", "cjs"],
        language: javascript_language,
        highlights: tree_sitter_javascript::HIGHLIGHT_QUERY,
        injections: tree_sitter_javascript::INJECTIONS_QUERY,
        indents: include_str!("../queries/javascript/indents.scm"),
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
    /// Highlight capture index to style, resolved against the theme up front.
    capture_styles: Vec<Option<Style>>,
    /// Indices of the `@injection.content` and `@injection.language` captures.
    content_capture: Option<u32>,
    language_capture: Option<u32>,
    /// Indices of the `@indent` and `@outdent` captures.
    indent_capture: Option<u32>,
    outdent_capture: Option<u32>,
}

fn compile(config: &LanguageConfig, theme: &Theme) -> Result<Rc<Compiled>> {
    let language = (config.language)();
    let highlights = Query::new(&language, config.highlights)
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

    Ok(Rc::new(Compiled {
        language,
        highlights,
        injections,
        indents,
        capture_styles,
        content_capture,
        language_capture,
        indent_capture,
        outdent_capture,
    }))
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



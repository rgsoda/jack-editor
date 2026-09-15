use crate::buffer::Document;

/// A region named by its shape rather than by a motion: the word you are on,
/// the string you are inside, the block the cursor is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Object {
    /// Letters, digits and underscores, or the run of punctuation or
    /// whitespace under the cursor.
    Word,
    /// Anything but whitespace, which is vim's `W`.
    BigWord,
    /// The text between two of the same quote character, on one line.
    Quote(char),
    /// The text between a pair of brackets, across as many lines as it takes.
    Pair(char, char),
    /// Lines up to the next blank one.
    Paragraph,
}

/// Which object a key names. `b` and `B` are vim's aliases for the round and
/// curly pairs; either bracket of a pair names it, so `di(` and `di)` agree.
pub fn from_key(key: char) -> Option<Object> {
    Some(match key {
        'w' => Object::Word,
        'W' => Object::BigWord,
        '"' | '\'' | '`' => Object::Quote(key),
        '(' | ')' | 'b' => Object::Pair('(', ')'),
        '{' | '}' | 'B' => Object::Pair('{', '}'),
        '[' | ']' => Object::Pair('[', ']'),
        '<' | '>' => Object::Pair('<', '>'),
        'p' => Object::Paragraph,
        _ => return None,
    })
}

/// The object's char range, `[start, end)`. `around` is `a` rather than `i`:
/// the delimiters, or the whitespace after a word.
pub fn resolve(doc: &Document, at: usize, object: Object, around: bool) -> Option<(usize, usize)> {
    match object {
        Object::Word => word(doc, at, around, word_class),
        Object::BigWord => word(doc, at, around, big_word_class),
        Object::Quote(quote) => quoted(doc, at, quote, around),
        Object::Pair(open, close) => pair(doc, at, open, close, around),
        Object::Paragraph => paragraph(doc, at, around),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Class {
    Word,
    Space,
    Punctuation,
}

fn word_class(c: char) -> Class {
    match c {
        c if c.is_whitespace() => Class::Space,
        c if c.is_alphanumeric() || c == '_' => Class::Word,
        _ => Class::Punctuation,
    }
}

/// `W` knows only two kinds of character: space, and not space.
fn big_word_class(c: char) -> Class {
    match c.is_whitespace() {
        true => Class::Space,
        false => Class::Word,
    }
}

/// A word never crosses a line, so everything here works within one.
fn word(doc: &Document, at: usize, around: bool, class: fn(char) -> Class) -> Option<(usize, usize)> {
    let (line, column) = doc.coords(at);
    let text = doc.line_str(line);
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() || column >= chars.len() {
        return None;
    }

    let base = doc.line_to_char(line);
    let wanted = class(chars[column]);
    let start = (0..=column).rev().take_while(|&i| class(chars[i]) == wanted).last()?;
    let end = (column..chars.len()).take_while(|&i| class(chars[i]) == wanted).last()? + 1;
    if !around {
        return Some((base + start, base + end));
    }

    // `aw` takes the whitespace after the word, or the whitespace before it
    // when there is none after - so deleting the last word on a line does not
    // leave a dangling space.
    let after = (end..chars.len()).take_while(|&i| class(chars[i]) == Class::Space).count();
    if after > 0 {
        return Some((base + start, base + end + after));
    }
    let before = (0..start).rev().take_while(|&i| class(chars[i]) == Class::Space).count();
    Some((base + start - before, base + end))
}

fn quoted(doc: &Document, at: usize, quote: char, around: bool) -> Option<(usize, usize)> {
    let (line, column) = doc.coords(at);
    let text = doc.line_str(line);
    let base = doc.line_to_char(line);

    // Quotes pair up left to right across the line, as vim does: the first
    // with the second, the third with the fourth.
    let positions: Vec<usize> = text
        .chars()
        .enumerate()
        .filter(|&(_, c)| c == quote)
        .map(|(i, _)| i)
        .collect();

    for window in positions.chunks(2) {
        let [open, close] = window else { continue };
        // The pair the cursor is in, or the next one along the line - `ci"`
        // from the start of a line finds the string on it.
        if column <= *close {
            return match around {
                true => Some((base + open, base + close + 1)),
                false => Some((base + open + 1, base + close)),
            };
        }
    }
    None
}

fn pair(doc: &Document, at: usize, open: char, close: char, around: bool) -> Option<(usize, usize)> {
    let total = doc.len_chars();
    if total == 0 {
        return None;
    }

    // Back to the innermost unclosed opening bracket, counting the pairs that
    // close on the way so a nested one is stepped over.
    let mut depth = 0usize;
    let mut start = None;
    let mut i = at.min(total - 1);
    loop {
        let c = doc.text.char(i);
        if c == close && i != at {
            depth += 1;
        } else if c == open {
            match depth {
                0 => {
                    start = Some(i);
                    break;
                }
                _ => depth -= 1,
            }
        }
        match i {
            0 => break,
            _ => i -= 1,
        }
    }
    let start = start?;

    let mut depth = 0usize;
    let mut end = None;
    for i in start..total {
        let c = doc.text.char(i);
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                end = Some(i);
                break;
            }
        }
    }
    let end = end?;

    match around {
        true => Some((start, end + 1)),
        // An empty pair has nothing inside, and selecting nothing is not a
        // failure: `di()` on `()` deletes nothing and says nothing.
        false => Some((start + 1, end)),
    }
}

fn paragraph(doc: &Document, at: usize, around: bool) -> Option<(usize, usize)> {
    let total = doc.len_lines();
    let (line, _) = doc.coords(at);
    let blank = |line: usize| doc.line_str(line).trim().is_empty();
    let wanted = blank(line);

    let first = (0..=line).rev().take_while(|&l| blank(l) == wanted).last()?;
    let mut last = (line..total).take_while(|&l| blank(l) == wanted).last()?;

    if around {
        // A paragraph takes the blank lines after it, or the ones before when
        // there are none after.
        let after = (last + 1..total).take_while(|&l| blank(l) != wanted).count();
        match after {
            0 => {
                let before = (0..first).rev().take_while(|&l| blank(l) != wanted).count();
                return Some((line_start(doc, first - before), line_end(doc, last)));
            }
            _ => last += after,
        }
    }
    Some((line_start(doc, first), line_end(doc, last)))
}

fn line_start(doc: &Document, line: usize) -> usize {
    doc.line_to_char(line)
}

/// Up to the start of the next line, so the newline belongs to this one.
fn line_end(doc: &Document, line: usize) -> usize {
    match line + 1 < doc.len_lines() {
        true => doc.line_to_char(line + 1),
        false => doc.len_chars(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn doc(text: &str) -> Document {
        let mut doc = Document::scratch();
        doc.text = Rope::from_str(text);
        doc
    }

    /// The object as text, which is what every one of these is really about.
    fn text_of(text: &str, at: usize, key: char, around: bool) -> Option<String> {
        let doc = doc(text);
        let object = from_key(key)?;
        let (start, end) = resolve(&doc, at, object, around)?;
        Some(doc.slice_str(start, end))
    }

    #[test]
    fn inner_word_is_the_word_under_the_cursor() {
        let line = "let x = foo_bar(1);";
        for at in 8..15 {
            assert_eq!(text_of(line, at, 'w', false).as_deref(), Some("foo_bar"));
        }
    }

    #[test]
    fn a_word_takes_the_space_after_it() {
        assert_eq!(text_of("one two three", 4, 'w', true).as_deref(), Some("two "));
    }

    #[test]
    fn a_word_takes_the_space_before_it_when_there_is_none_after() {
        assert_eq!(text_of("one two", 4, 'w', true).as_deref(), Some(" two"));
    }

    #[test]
    fn a_word_stops_at_punctuation() {
        // `w` sees three objects in `foo.bar`; `W` sees one.
        assert_eq!(text_of("foo.bar baz", 1, 'w', false).as_deref(), Some("foo"));
        assert_eq!(text_of("foo.bar baz", 3, 'w', false).as_deref(), Some("."));
        assert_eq!(text_of("foo.bar baz", 1, 'W', false).as_deref(), Some("foo.bar"));
    }

    #[test]
    fn a_word_on_whitespace_is_the_whitespace() {
        assert_eq!(text_of("one   two", 4, 'w', false).as_deref(), Some("   "));
    }

    #[test]
    fn quotes_pair_left_to_right_along_the_line() {
        let line = r#"say("hi", 'x')"#;
        assert_eq!(text_of(line, 5, '"', false).as_deref(), Some("hi"));
        assert_eq!(text_of(line, 5, '"', true).as_deref(), Some("\"hi\""));
        assert_eq!(text_of(line, 11, '\'', false).as_deref(), Some("x"));
    }

    #[test]
    fn a_quote_is_found_further_along_the_line() {
        // `ci"` with the cursor before the string still changes the string.
        assert_eq!(text_of(r#"let s = "hi";"#, 0, '"', false).as_deref(), Some("hi"));
    }

    #[test]
    fn a_pair_is_the_innermost_one_around_the_cursor() {
        let line = "f(g(x), y)";
        assert_eq!(text_of(line, 4, '(', false).as_deref(), Some("x"));
        assert_eq!(text_of(line, 8, '(', false).as_deref(), Some("g(x), y"));
        assert_eq!(text_of(line, 8, ')', true).as_deref(), Some("(g(x), y)"));
    }

    #[test]
    fn a_pair_spans_lines() {
        let text = "fn main() {\n    body;\n}\n";
        assert_eq!(text_of(text, 16, '{', false).as_deref(), Some("\n    body;\n"));
    }

    #[test]
    fn an_empty_pair_has_nothing_inside_it() {
        assert_eq!(text_of("()", 1, '(', false).as_deref(), Some(""));
        assert_eq!(text_of("()", 1, '(', true).as_deref(), Some("()"));
    }

    #[test]
    fn a_pair_the_cursor_is_not_inside_is_no_object() {
        assert_eq!(text_of("a (b) c", 6, '(', false), None);
    }

    #[test]
    fn brackets_have_aliases() {
        assert_eq!(from_key('b'), from_key(')'));
        assert_eq!(from_key('B'), from_key('}'));
        assert_eq!(from_key('z'), None);
    }

    #[test]
    fn a_paragraph_runs_to_the_blank_line() {
        let text = "one\ntwo\n\nthree\n";
        assert_eq!(text_of(text, 0, 'p', false).as_deref(), Some("one\ntwo\n"));
        assert_eq!(text_of(text, 0, 'p', true).as_deref(), Some("one\ntwo\n\n"));
        // On a blank line the object is the run of blank lines.
        assert_eq!(text_of(text, 8, 'p', false).as_deref(), Some("\n"));
    }
}

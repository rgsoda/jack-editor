//! `gq` - a paragraph, rewrapped to fit.
//!
//! Prose in a source file is nearly always prose in a comment, so the thing
//! that matters is the bit at the front of the line: the indentation and the
//! `//` or the `#`. Reflowing text that loses those turns a comment into a
//! syntax error, so the prefix is worked out first, taken off every line,
//! and put back on every line of the answer.
//!
//! Pure, and the editor only decides which lines to hand it.

/// The markers a comment can start with. Two or three characters at most, and
/// no letters: a line starting with a word is prose, not a prefix.
const MARKERS: [&str; 8] = ["///", "//!", "//", "#", "--", ";;", ";", "*"];

/// The part of a line that gets repeated on every line of the answer: the
/// indentation, and the comment marker with the space after it.
///
/// `None` when the line has no marker, in which case the indentation on its
/// own is the prefix.
fn marked(line: &str) -> Option<&str> {
    let indent = line.len() - line.trim_start().len();
    let rest = &line[indent..];
    let marker = MARKERS.iter().find(|marker| rest.starts_with(**marker))?;
    let after = &rest[marker.len()..];
    // `//comment` is a marker too, but `//` glued to a word is what the
    // prefix is, and the space after it is kept where there was one.
    let space = after.len() - after.trim_start_matches(' ').len();
    Some(&line[..indent + marker.len() + space])
}

/// What every line of this paragraph begins with. The first line decides, and
/// the rest have to agree: a paragraph whose lines start differently is one
/// where the prefix is only the indentation they share.
fn prefix<'a>(lines: &[&'a str]) -> &'a str {
    let Some(first) = lines.first() else {
        return "";
    };
    let indent = &first[..first.len() - first.trim_start().len()];
    let Some(marked) = marked(first) else {
        return indent;
    };
    match lines[1..].iter().all(|line| line.starts_with(marked.trim_end()) || line.trim().is_empty())
    {
        true => marked,
        false => indent,
    }
}

/// A line with the prefix taken off. A paragraph can agree about its marker
/// without agreeing about the space after it, so the marker comes off even
/// when the space is not there.
fn body<'a>(line: &'a str, prefix: &str) -> &'a str {
    line.strip_prefix(prefix)
        .or_else(|| line.strip_prefix(prefix.trim_end()))
        .unwrap_or_else(|| line.trim_start())
}

/// One paragraph, wrapped greedily: as many words on a line as fit, and a
/// word longer than the width on a line of its own rather than broken.
fn wrap(lines: &[&str], width: usize) -> Vec<String> {
    let prefix = prefix(lines);
    let words: Vec<&str> = lines
        .iter()
        .flat_map(|line| body(line, prefix).split_whitespace())
        .collect();
    if words.is_empty() {
        return lines.iter().map(|line| line.to_string()).collect();
    }
    // A width that leaves no room for a word at all would wrap after every
    // one of them; one word per line is the least useless answer.
    let room = width.saturating_sub(prefix.chars().count()).max(1);

    let mut out = Vec::new();
    let mut line = String::new();
    for word in words {
        let length = line.chars().count();
        if !line.is_empty() && length + 1 + word.chars().count() > room {
            out.push(format!("{prefix}{line}"));
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(format!("{prefix}{line}"));
    }
    out
}

/// The lines, rewrapped. Blank lines separate one paragraph from the next and
/// are kept exactly where they were: they are how a person said "these belong
/// together and those do not", and reflowing is not the moment to argue.
pub fn reflow(lines: &[String], width: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    for line in lines {
        // A comment marker with nothing after it - the blank line of a
        // comment block - separates paragraphs the same way a blank line does.
        let empty = line.trim().is_empty()
            || marked(line).is_some_and(|prefix| line[prefix.len()..].trim().is_empty());
        if empty {
            if !paragraph.is_empty() {
                out.extend(wrap(&paragraph, width));
                paragraph.clear();
            }
            out.push(line.clone());
            continue;
        }
        paragraph.push(line);
    }
    if !paragraph.is_empty() {
        out.extend(wrap(&paragraph, width));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn a_long_line_becomes_as_many_lines_as_it_takes() {
        let got = reflow(&lines("one two three four five six seven"), 14);
        assert_eq!(got, lines("one two three\nfour five six\nseven"));
        // And short lines join back up.
        assert_eq!(reflow(&lines("one\ntwo\nthree"), 20), lines("one two three"));
    }

    #[test]
    fn a_comment_stays_a_comment() {
        let text = "    // the quick brown fox jumped over the lazy dog";
        let got = reflow(&lines(text), 32);
        assert_eq!(got, lines("    // the quick brown fox\n    // jumped over the lazy dog"));
        // Every line of the answer carries the marker, or the next build
        // fails, which is the whole reason this is not just word wrapping.
        assert!(got.iter().all(|line| line.starts_with("    // ")), "{got:?}");
    }

    #[test]
    fn blank_lines_are_where_one_paragraph_stops_and_the_next_starts() {
        let text = "aaa bbb ccc\n\nddd eee fff";
        assert_eq!(reflow(&lines(text), 7), lines("aaa bbb\nccc\n\nddd eee\nfff"));
        // The blank line of a comment block counts too: `//` with nothing
        // after it is a paragraph break, not a word.
        let comment = "// aaa bbb\n//\n// ccc ddd";
        assert_eq!(reflow(&lines(comment), 40), lines("// aaa bbb\n//\n// ccc ddd"));
    }

    #[test]
    fn a_word_longer_than_the_line_gets_a_line_of_its_own() {
        let got = reflow(&lines("a supercalifragilistic b"), 8);
        assert_eq!(got, lines("a\nsupercalifragilistic\nb"));
    }

    #[test]
    fn indentation_survives_and_decides_the_room_there_is_left() {
        let got = reflow(&lines("        one two three four"), 20);
        assert_eq!(got, lines("        one two\n        three four"));
    }
}

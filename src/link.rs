//! `gx`: the link under the cursor, handed to whatever opens links here.
//!
//! Finding where a URL ends is the whole of it, and the awkward part is that
//! prose puts punctuation against them: *see https://example.com/x.* ends in a
//! full stop that is not part of the address, and *(https://example.com/x)* is
//! wrapped in brackets that are not either - unless the address has brackets
//! of its own, which wikipedia's do.

/// Characters a URL may be made of. Deliberately not "everything that is not a
/// space": a quote or a backtick next to a URL in prose or in code is a quote,
/// never part of the address.
fn linky(ch: char) -> bool {
    !ch.is_whitespace() && !matches!(ch, '"' | '\'' | '`' | '<' | '>' | '|' | '\\' | '^')
}

/// What jack is willing to hand to an opener. A scheme, or the `www.` that
/// everyone writes without one.
fn is_link(text: &str) -> bool {
    const SCHEMES: [&str; 6] = ["http://", "https://", "mailto:", "ftp://", "file://", "git@"];
    SCHEMES.iter().any(|scheme| text.starts_with(scheme)) || text.starts_with("www.")
}

/// The link the cursor is in, else the first one to its right on the line.
///
/// Char offsets, like everything else the editor counts.
pub fn find(line: &str, column: usize) -> Option<(usize, usize, String)> {
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if !linky(chars[at]) {
            at += 1;
            continue;
        }
        let start = at;
        let mut end = at;
        while end < chars.len() && linky(chars[end]) {
            end += 1;
        }
        at = end;
        if column >= end {
            continue;
        }
        let word: String = chars[start..end].iter().collect();
        let (dropped, trimmed) = trim(&word);
        if !is_link(trimmed) {
            continue;
        }
        // The cursor may be in the punctuation that was trimmed off, which is
        // not in the link - but it is next to it, and meant it.
        let start = start + dropped;
        return Some((start, start + trimmed.chars().count(), trimmed.to_string()));
    }
    None
}

/// The punctuation prose leaves around a URL, taken off: how many chars came
/// off the front, and what is left.
///
/// The front first, so that the brackets round `(https://example.com/x)` are
/// not counted as brackets the link opened. A closing bracket *is* kept when
/// the link opened one, so a wikipedia address with `(disambiguation)` in it
/// survives being written inside brackets.
fn trim(word: &str) -> (usize, &str) {
    // The brackets are ASCII, so what came off the front is as many chars as
    // it is bytes.
    let rest = word.trim_start_matches(['(', '[', '{']);
    let front = word.len() - rest.len();
    let mut end = rest.len();
    while let Some(last) = rest[..end].chars().next_back() {
        let balanced = |open: char, shut: char| {
            rest[..end].matches(open).count() >= rest[..end].matches(shut).count()
        };
        let drop = match last {
            '.' | ',' | ';' | ':' | '!' | '?' => true,
            ')' => !balanced('(', ')'),
            ']' => !balanced('[', ']'),
            '}' => !balanced('{', '}'),
            _ => false,
        };
        if !drop {
            break;
        }
        end -= last.len_utf8();
    }
    (front, &rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(line: &str, column: usize) -> Option<String> {
        find(line, column).map(|(_, _, link)| link)
    }

    #[test]
    fn a_link_is_found_under_the_cursor_or_to_its_right() {
        let line = "see https://example.com/a for it";
        assert_eq!(found(line, 0).as_deref(), Some("https://example.com/a"));
        assert_eq!(found(line, 10).as_deref(), Some("https://example.com/a"));
        assert_eq!(found(line, 30), None, "past it, and nothing after");
        assert_eq!(found("no link here", 0), None);
        // Where it is, in chars, so the editor can put the cursor on it.
        assert_eq!(find(line, 0).map(|(start, end, _)| (start, end)), Some((4, 25)));
    }

    #[test]
    fn the_punctuation_prose_leaves_against_it_is_not_part_of_it() {
        assert_eq!(found("see https://example.com/x.", 0).as_deref(), Some("https://example.com/x"));
        assert_eq!(found("(https://example.com/x)", 0).as_deref(), Some("https://example.com/x"));
        assert_eq!(found("see https://example.com/x, and", 0).as_deref(), Some("https://example.com/x"));
        assert_eq!(found("<https://example.com/x>", 0).as_deref(), Some("https://example.com/x"));
        // Unless the link opened the bracket itself.
        assert_eq!(
            found("https://en.wikipedia.org/wiki/Jack_(disambiguation)", 0).as_deref(),
            Some("https://en.wikipedia.org/wiki/Jack_(disambiguation)")
        );
    }

    #[test]
    fn what_is_not_a_link_is_left_alone() {
        assert_eq!(found("src/editor.rs:1247", 0), None, "a place, not a link");
        assert_eq!(found("./configure", 0), None);
        assert_eq!(found("mailto:someone@example.com", 0).as_deref(), Some("mailto:someone@example.com"));
        assert_eq!(found("www.example.com", 0).as_deref(), Some("www.example.com"));
        assert_eq!(found("git@github.com:rgsoda/jack-editor.git", 0).as_deref(), Some("git@github.com:rgsoda/jack-editor.git"));
        // A quote next to it is a quote.
        assert_eq!(found("\"https://example.com/x\"", 0).as_deref(), Some("https://example.com/x"));
    }

    #[test]
    fn the_second_link_on_a_line_is_reachable() {
        let line = "https://a.example/1 https://b.example/2";
        assert_eq!(found(line, 0).as_deref(), Some("https://a.example/1"));
        assert_eq!(found(line, 25).as_deref(), Some("https://b.example/2"));
    }
}

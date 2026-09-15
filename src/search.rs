use regex::{Regex, RegexBuilder};

use crate::buffer::Document;

/// Where a match is, in characters, and whether finding it meant going round
/// the end of the buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub start: usize,
    pub end: usize,
    pub wrapped: bool,
}

/// The last search: its pattern, compiled, and whether its matches are still
/// being highlighted. One search is shared by every buffer, as in vim, so `n`
/// keeps working after switching files.
#[derive(Default)]
pub struct Search {
    pub pattern: String,
    regex: Option<Regex>,
    /// True while matches are painted, until `esc` in normal mode.
    pub highlight: bool,
    /// Which way the last search went, so `n` repeats it and `N` reverses it.
    pub backward: bool,
}

impl Search {
    /// Compile a pattern, with the same smart case as the grep picker: an
    /// all-lower-case pattern matches either case, a capital means it.
    pub fn set_pattern(&mut self, pattern: &str) -> Result<(), String> {
        self.pattern = pattern.to_string();
        if pattern.is_empty() {
            self.regex = None;
            return Ok(());
        }
        let insensitive = !pattern.chars().any(char::is_uppercase);
        match RegexBuilder::new(pattern).case_insensitive(insensitive).build() {
            Ok(regex) => {
                self.regex = Some(regex);
                Ok(())
            }
            Err(err) => {
                self.regex = None;
                Err(brief(&err.to_string()))
            }
        }
    }

    pub fn is_set(&self) -> bool {
        self.regex.is_some()
    }

    /// The next match strictly after `from`, or before it when `backward`.
    /// Wraps round the buffer, as `n` does, and says when it did.
    pub fn find(&self, doc: &Document, from: usize, backward: bool) -> Option<Hit> {
        let regex = self.regex.as_ref()?;
        let total = doc.len_lines();
        let (line, _) = doc.coords(from.min(doc.len_chars()));

        // Every line once, starting at the cursor's, then round to it again.
        let order: Vec<usize> = match backward {
            false => (line..total).chain(0..=line.min(total - 1)).collect(),
            true => (0..=line).rev().chain((line..total).rev()).collect(),
        };

        for (step, line) in order.into_iter().enumerate() {
            let text = doc.line_str(line);
            let base = doc.line_to_char(line);
            let mut hits: Vec<(usize, usize)> = regex
                .find_iter(&text)
                .map(|m| {
                    let start = base + text[..m.start()].chars().count();
                    let end = base + text[..m.end()].chars().count();
                    (start, end)
                })
                // An empty match would let `n` sit still forever.
                .filter(|(start, end)| end > start)
                .collect();
            if backward {
                hits.reverse();
            }

            for (start, end) in hits {
                let past = match backward {
                    false => start > from,
                    true => start < from,
                };
                // Only the first line searched contains the cursor; after that
                // every match on the line counts.
                if step > 0 || past {
                    // A match that is not past the cursor is one we came round
                    // the buffer to reach - including the one under it, which
                    // is the only match in a buffer that has just one.
                    let wrapped = match backward {
                        false => start <= from,
                        true => start >= from,
                    };
                    return Some(Hit { start, end, wrapped });
                }
            }
        }
        None
    }

    /// Every match on the given lines, for painting the viewport.
    pub fn matches_in_lines(&self, doc: &Document, first: usize, last: usize) -> Vec<(usize, usize)> {
        let Some(regex) = self.regex.as_ref() else {
            return Vec::new();
        };
        let mut found = Vec::new();
        for line in first..last.min(doc.len_lines()) {
            let text = doc.line_str(line);
            let base = doc.line_to_char(line);
            for m in regex.find_iter(&text) {
                if m.end() > m.start() {
                    found.push((
                        base + text[..m.start()].chars().count(),
                        base + text[..m.end()].chars().count(),
                    ));
                }
            }
        }
        found
    }
}

/// Regex errors are a caret diagram with the complaint at the bottom.
fn brief(text: &str) -> String {
    for line in text.lines().rev() {
        if let Some(reason) = line.trim().strip_prefix("error: ") {
            return reason.to_string();
        }
    }
    text.lines().next().unwrap_or(text).trim().to_string()
}

/// Escape a word so it is matched literally, with boundaries either side -
/// what `*` means by "this word".
pub fn word_pattern(word: &str) -> String {
    format!(r"\b{}\b", regex::escape(word))
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

    fn search(pattern: &str) -> Search {
        let mut search = Search::default();
        search.set_pattern(pattern).unwrap();
        search
    }

    #[test]
    fn a_search_finds_the_next_match_after_the_cursor() {
        let doc = doc("one two one two\n");
        let s = search("two");
        let hit = s.find(&doc, 0, false).unwrap();
        assert_eq!((hit.start, hit.end), (4, 7));
        assert!(!hit.wrapped);

        let hit = s.find(&doc, 4, false).unwrap();
        assert_eq!((hit.start, hit.end), (12, 15));
    }

    #[test]
    fn a_search_wraps_and_says_so() {
        let doc = doc("alpha\nbeta\n");
        let s = search("alpha");
        let hit = s.find(&doc, 7, false).unwrap();
        assert_eq!(hit.start, 0);
        assert!(hit.wrapped);
    }

    #[test]
    fn a_backward_search_finds_the_previous_match() {
        let doc = doc("one two one two\n");
        let s = search("one");
        let hit = s.find(&doc, 8, true).unwrap();
        assert_eq!(hit.start, 0);
        assert!(!hit.wrapped);

        // Before the first match there is only the last one, round the end.
        let hit = s.find(&doc, 0, true).unwrap();
        assert_eq!(hit.start, 8);
        assert!(hit.wrapped);
    }

    #[test]
    fn a_search_crosses_lines() {
        let doc = doc("one\ntwo\nthree\n");
        let s = search("three");
        let hit = s.find(&doc, 0, false).unwrap();
        assert_eq!(hit.start, 8);
    }

    #[test]
    fn a_pattern_is_a_regex_with_smart_case() {
        let doc = doc("Cat cat CAT\n");
        assert_eq!(search("cat").matches_in_lines(&doc, 0, 1).len(), 3);
        assert_eq!(search("Cat").matches_in_lines(&doc, 0, 1).len(), 1);
        assert_eq!(search(r"c.t").matches_in_lines(&doc, 0, 1).len(), 3);
    }

    #[test]
    fn a_pattern_that_is_not_a_regex_is_reported() {
        let mut s = Search::default();
        let err = s.set_pattern("fn (").unwrap_err();
        assert!(err.contains("unclosed group"), "{err}");
        // And nothing is left compiled to search with.
        assert!(!s.is_set());
    }

    #[test]
    fn an_empty_match_cannot_trap_the_cursor() {
        let doc = doc("aaa\n");
        // `a*` matches the empty string everywhere; only the real ones count.
        let s = search("a*");
        let hit = s.find(&doc, 0, false).unwrap();
        assert!(hit.end > hit.start);
    }

    #[test]
    fn matches_are_only_gathered_for_the_lines_asked_for() {
        let doc = doc("hit\nhit\nhit\nhit\n");
        let s = search("hit");
        assert_eq!(s.matches_in_lines(&doc, 1, 3).len(), 2);
        assert_eq!(s.matches_in_lines(&doc, 0, 99).len(), 4);
    }

    #[test]
    fn the_word_pattern_matches_whole_words_only() {
        let doc = doc("cat category cat\n");
        let s = search(&word_pattern("cat"));
        assert_eq!(s.matches_in_lines(&doc, 0, 1).len(), 2);
    }
}

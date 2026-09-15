use std::collections::HashMap;

use crate::view::View;

/// How much of the buffer a completion looks at, in chars either side of the
/// cursor. A whole file is the normal case; the window is what keeps a
/// generated 10MB one from costing a visible pause.
///
/// Two sizes, because the two tiers cost wildly different amounts: scanning
/// 400k chars for words takes about a millisecond, while running the highlight
/// query over the same span takes forty. The grammar therefore gets a tenth of
/// the window - still a few thousand lines, and the words tier covers the rest.
const WORD_WINDOW: usize = 200_000;
const NAME_WINDOW: usize = 20_000;

/// The longest list worth offering. Past this you are better off typing.
const MAX_ITEMS: usize = 200;

/// The shortest word worth remembering as a candidate.
const MIN_LEN: usize = 2;

/// One thing you could be typing, and what the grammar thinks it is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Candidate {
    pub text: String,
    /// `fn`, `type`, `var` - or `None` for a word that was only ever seen as
    /// text, which is all there is to go on without a grammar.
    pub kind: Option<&'static str>,
}

/// The popup: the word being completed, and what could finish it.
pub struct Completion {
    /// Char index where the word being completed starts. Accepting replaces
    /// from here to the cursor.
    pub start: usize,
    prefix: String,
    /// Everything the buffer offered when the popup opened, ranked once.
    all: Vec<Candidate>,
    /// Indices into `all` that still match the prefix.
    shown: Vec<usize>,
    selected: usize,
}

impl Completion {
    /// Gather the candidates for the word ending at `at`. `None` when there is
    /// no word there, or nothing in the buffer that could finish it.
    pub fn new(view: &View, at: usize, backward: bool) -> Option<Completion> {
        let start = word_start(view, at);
        let prefix = view.doc.slice_str(start, at);
        let all = candidates(view, at);

        let mut completion = Completion {
            start,
            prefix,
            all,
            shown: Vec::new(),
            selected: 0,
        };
        completion.filter(backward);
        match completion.shown.is_empty() {
            true => None,
            false => Some(completion),
        }
    }

    /// Re-filter after the buffer changed under the popup - a character typed,
    /// or one deleted. False when nothing matches any more and the popup has
    /// outlived its usefulness.
    pub fn update(&mut self, view: &View, at: usize) -> bool {
        if at < self.start {
            return false;
        }
        self.prefix = view.doc.slice_str(self.start, at);
        self.filter(false);
        !self.shown.is_empty()
    }

    fn filter(&mut self, backward: bool) {
        let prefix = self.prefix.clone();
        self.shown = (0..self.all.len())
            // A candidate that is already the whole prefix has nothing to add.
            .filter(|&i| self.all[i].text != prefix && matches(&self.all[i].text, &prefix))
            .take(MAX_ITEMS)
            .collect();
        self.selected = match backward {
            true => self.shown.len().saturating_sub(1),
            false => 0,
        };
    }

    /// Move the selection, wrapping - which is what makes `^n` `^n` `^n` a way
    /// to walk the list rather than something that stops at the bottom.
    pub fn step(&mut self, forward: bool) {
        if self.shown.is_empty() {
            return;
        }
        self.selected = match forward {
            true => (self.selected + 1) % self.shown.len(),
            false => (self.selected + self.shown.len() - 1) % self.shown.len(),
        };
    }

    pub fn items(&self) -> impl Iterator<Item = &Candidate> {
        self.shown.iter().map(|&i| &self.all[i])
    }

    pub fn len(&self) -> usize {
        self.shown.len()
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn selected_text(&self) -> &str {
        &self.all[self.shown[self.selected]].text
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

/// Prefix matching, case-insensitively only when the prefix is all lower case -
/// the same smart-case rule search uses.
fn matches(text: &str, prefix: &str) -> bool {
    if text.len() < prefix.len() {
        return false;
    }
    match prefix.chars().any(|c| c.is_uppercase()) {
        true => text.starts_with(prefix),
        false => text
            .chars()
            .zip(prefix.chars())
            .all(|(t, p)| t.to_lowercase().eq(p.to_lowercase())),
    }
}

pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Where the word ending at `at` begins.
fn word_start(view: &View, at: usize) -> usize {
    let mut start = at;
    while start > 0 && is_word(view.doc.text.char(start - 1)) {
        start -= 1;
    }
    start
}

/// Everything in the buffer that could finish a word, best first.
///
/// Two tiers. Every word in the buffer is a candidate, because the thing you
/// are half way through typing is usually a few lines up and no grammar is
/// needed to see it. On top of that, what the grammar *names* - functions,
/// types, fields - is marked with its kind and ranked first, so a real
/// identifier beats a word that only ever appeared in a comment.
fn candidates(view: &View, at: usize) -> Vec<Candidate> {
    let chars = view.doc.len_chars();
    let from = at.saturating_sub(WORD_WINDOW);
    let to = (at + WORD_WINDOW).min(chars);

    // Word -> (kind, how far the nearest occurrence is from the cursor).
    let mut seen: HashMap<String, (Option<&'static str>, usize)> = HashMap::new();

    let mut word = String::new();
    let mut word_start = from;
    let record = |word: &mut String, start: usize, seen: &mut HashMap<_, _>| {
        if word.len() >= MIN_LEN && word.starts_with(|c: char| c.is_alphabetic() || c == '_') {
            let distance = at.abs_diff(start);
            let entry = seen.entry(std::mem::take(word)).or_insert((None, distance));
            entry.1 = entry.1.min(distance);
        }
        word.clear();
    };

    for (offset, c) in view.doc.text.slice(from..to).chars().enumerate() {
        match is_word(c) {
            true => {
                if word.is_empty() {
                    word_start = from + offset;
                }
                word.push(c);
            }
            false => record(&mut word, word_start, &mut seen),
        }
    }
    record(&mut word, word_start, &mut seen);

    // The grammar's names, for the same window. A word the grammar named keeps
    // the first kind it was given: `push_str` is a function and a field
    // depending on the line, and the first one the query reached will do.
    let names = at.saturating_sub(NAME_WINDOW)..(at + NAME_WINDOW).min(chars);
    let bytes = view.doc.text.char_to_byte(names.start)..view.doc.text.char_to_byte(names.end);
    for (range, kind) in view.identifiers(bytes) {
        // Borrowed from the rope where it can be - one allocation per name is
        // most of the cost of asking the grammar at all.
        let slice = view.doc.text.byte_slice(range);
        let entry = match slice.as_str() {
            Some(text) => seen.get_mut(text),
            None => seen.get_mut(&slice.to_string()),
        };
        if let Some(entry) = entry
            && entry.0.is_none()
        {
            entry.0 = Some(kind);
        }
    }

    let mut candidates: Vec<(Option<&'static str>, usize, String)> = seen
        .into_iter()
        .map(|(text, (kind, distance))| (kind, distance, text))
        .collect();
    // Named things first, then whichever is nearest, then alphabetically so the
    // list does not reshuffle for reasons the eye cannot see.
    candidates.sort_by(|a, b| {
        a.0.is_none()
            .cmp(&b.0.is_none())
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    candidates
        .into_iter()
        .map(|(kind, _, text)| Candidate { text, kind })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buffer::Document;
    use crate::theme::Theme;
    use ropey::Rope;

    fn open(text: &str, path: Option<&str>) -> View {
        let doc = match path {
            Some(path) => Document::open(path).unwrap(),
            None => Document::scratch(),
        };
        let mut view = View::new(doc);
        view.doc.text = Rope::from_str(text);
        view.attach_syntax(&Theme::built_in());
        view
    }

    fn texts(completion: &Completion) -> Vec<&str> {
        completion.items().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn a_word_is_completed_from_the_words_in_the_buffer() {
        let text = "alphabet album\nal";
        let view = open(text, None);
        let completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();

        assert_eq!(completion.prefix(), "al");
        // `album` is nearer the cursor than `alphabet`.
        assert_eq!(texts(&completion), ["album", "alphabet"]);
    }

    #[test]
    fn the_word_being_typed_is_not_offered_to_finish_itself() {
        let view = open("alpha\nalpha", None);
        let completion = Completion::new(&view, view.doc.len_chars(), false);
        assert!(completion.is_none(), "nothing left to add");
    }

    #[test]
    fn short_words_and_numbers_are_not_candidates() {
        let view = open("a 42 42x ab\na", None);
        let completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();
        assert_eq!(texts(&completion), ["ab"]);
    }

    #[test]
    fn a_lower_case_prefix_matches_either_case() {
        let view = open("Widget widget\nwi", None);
        let completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();
        assert_eq!(texts(&completion), ["widget", "Widget"]);

        let view = open("Widget widget\nWi", None);
        let completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();
        assert_eq!(texts(&completion), ["Widget"]);
    }

    #[test]
    fn what_the_grammar_names_comes_first_and_says_what_it_is() {
        // `render_all` is a function; `render_later` only ever appears in a
        // comment, and is nearer the cursor - the kind still wins.
        let text = "fn render_all() {}\n// render_later\nre";
        let view = open(text, Some("demo.rs"));
        let completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();

        let items: Vec<_> = completion.items().collect();
        assert_eq!(items[0].text, "render_all");
        assert_eq!(items[0].kind, Some("fn"));
        assert_eq!(items[1].text, "render_later");
        assert_eq!(items[1].kind, None);
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let view = open("one once only\non", None);
        let mut completion = Completion::new(&view, view.doc.len_chars(), false).unwrap();
        assert_eq!(completion.selected(), 0);
        completion.step(false);
        assert_eq!(completion.selected(), completion.len() - 1);
        completion.step(true);
        assert_eq!(completion.selected(), 0);
    }

    #[test]
    fn opening_backwards_starts_at_the_bottom() {
        let view = open("one once only\non", None);
        let completion = Completion::new(&view, view.doc.len_chars(), true).unwrap();
        assert_eq!(completion.selected(), completion.len() - 1);
    }

    #[test]
    fn a_big_buffer_completes_between_keystrokes() {
        // One function per "file", repeated until it is megabytes: more than
        // anyone types into, and the window is what keeps it bounded.
        let mut text = String::new();
        for i in 0..20_000 {
            text.push_str(&format!(
                "fn render_{i}(widget: &Widget) -> String {{\n    widget.label.clone()\n}}\n"
            ));
        }
        let view = open(&text, Some("demo.rs"));
        let at = view.doc.len_chars() / 2;

        let start = std::time::Instant::now();
        let completion = Completion::new(&view, at, false).unwrap();
        let elapsed = start.elapsed();
        println!(
            "{} chars, {} candidates in {elapsed:?}",
            view.doc.len_chars(),
            completion.len()
        );

        // Debug builds are several times slower than the release binary the
        // keystroke actually lands in.
        assert!(elapsed.as_millis() < 500, "{elapsed:?}");
    }
}

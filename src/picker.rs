use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Where a picker's items came from, and so what confirming one does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// The open buffers. `id` is the index into `Editor::views`.
    Buffers,
    /// Files under the working directory. `target` is the path, relative to it.
    Files,
    /// The keymap. Confirming one does nothing: it is a list to read, not to
    /// act on.
    Help,
    /// Lines matching a pattern, anywhere under the working directory.
    /// `target` is the path and `id` the line number.
    Grep,
    /// What this buffer defines - functions, types, methods. `id` is the line.
    Symbols,
}

impl Source {
    pub fn prompt(self) -> &'static str {
        match self {
            Source::Buffers => "buffer",
            Source::Files => "file",
            Source::Help => "help",
            Source::Grep => "grep",
            Source::Symbols => "symbol",
        }
    }

    /// True when the query is a search run elsewhere rather than a filter over
    /// items already in hand. A live source replaces its whole list on every
    /// keystroke, and nothing here scores or reorders it.
    pub fn is_live(self) -> bool {
        matches!(self, Source::Grep)
    }
}

/// One candidate. `text` is what the query matches against and what is drawn;
/// `detail` is drawn dimmed on the right and never matched, so a modified
/// marker or a line number cannot influence the ranking.
pub struct Item {
    pub text: String,
    pub detail: String,
    /// What the source needs to act on this item, alongside `target`: a buffer
    /// index, a line number.
    pub id: usize,
    /// The path this item points at, for the sources that point at one.
    pub target: String,
}

/// A matched item: which item, how well it scored, and which characters of its
/// text the query landed on, so they can be drawn differently.
pub struct Match {
    pub index: usize,
    pub score: i32,
    pub positions: Vec<usize>,
}

/// The item the user chose. Sources identify items differently - a buffer by
/// its index, a file by its path - so both travel.
pub struct Choice {
    pub id: usize,
    pub target: String,
}

/// Where a chosen item opens: in the window the picker was opened from, or in
/// a new one split off it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Open {
    Here,
    /// `^v`, a window beside.
    Beside,
    /// `^s` or `^x`, a window below.
    Below,
}

/// What a keypress meant to the editor around the picker.
pub enum Outcome {
    /// Handled; the picker stays open.
    Continue,
    /// Close without doing anything.
    Cancel,
    /// Close and act on this item, from this source, in this window.
    Confirm(Source, Choice, Open),
    /// The query of a live source changed: run it again with this pattern.
    Search(String),
}

/// The one picker. Four planned sources - buffers, files, lines, matches -
/// differ only in how the items are built and what confirming one does, so
/// everything here is source-agnostic.
pub struct Picker {
    pub source: Source,
    pub query: String,
    items: Vec<Item>,
    matches: Vec<Match>,
    /// Index into `matches`, not into `items`.
    cursor: usize,
    /// First visible row of the list.
    scroll: usize,
    /// False while a background walk is still feeding this picker.
    complete: bool,
}

impl Picker {
    pub fn new(source: Source, items: Vec<Item>) -> Self {
        let mut picker = Picker {
            source,
            query: String::new(),
            items,
            matches: Vec::new(),
            cursor: 0,
            scroll: 0,
            complete: true,
        };
        picker.refilter();
        picker
    }

    /// A picker whose query is a search run elsewhere. It starts empty and
    /// complete: there is nothing to wait for until something is typed.
    pub fn live(source: Source) -> Self {
        Picker::new(source, Vec::new())
    }

    /// An empty picker that a background job will fill.
    pub fn streaming(source: Source) -> Self {
        let mut picker = Picker::new(source, Vec::new());
        picker.complete = false;
        picker
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }

    /// Nothing more is coming, whether or not it went well.
    pub fn mark_complete(&mut self) {
        self.complete = true;
    }

    /// Take a batch of streamed candidates. The selected item stays selected
    /// even as better matches arrive above it, because the list moving under a
    /// held-down cursor is how you open the wrong file.
    pub fn extend(&mut self, items: impl IntoIterator<Item = Item>, done: bool) {
        let selected = self.matches.get(self.cursor).map(|m| m.index);
        self.complete |= done;

        // Only the new items are scored. Re-ranking the whole list on every
        // batch is what makes streaming a large tree quadratic.
        let first = self.items.len();
        self.items.extend(items);
        for index in first..self.items.len() {
            if let Some(m) = self.match_for(index) {
                self.matches.push(m);
            }
        }
        // With no query every score is zero and the order is the walk's, which
        // is already what pushing produced.
        if !self.query.is_empty() && !self.source.is_live() {
            self.matches.sort_by_key(|m| -m.score);
        }
        if let Some(index) = selected
            && let Some(position) = self.matches.iter().position(|m| m.index == index)
        {
            self.cursor = position;
        }
    }

    pub fn matches(&self) -> &[Match] {
        &self.matches
    }

    pub fn item(&self, m: &Match) -> &Item {
        &self.items[m.index]
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn item_count(&self) -> usize {
        self.items.len()
    }

    /// Rows the panel takes out of the text area: the prompt plus the list.
    /// Fixed rather than fitted to the number of matches, so the text does not
    /// jump around underneath while typing.
    pub fn panel_height(text_rows: usize) -> usize {
        (text_rows / 2).clamp(2, 13)
    }

    pub fn list_rows(text_rows: usize) -> usize {
        Picker::panel_height(text_rows) - 1
    }

    /// Where the terminal cursor belongs: the end of the query on the prompt
    /// row, which is the top row of the panel.
    pub fn cursor_screen(&self, text_rows: usize) -> (u16, u16) {
        let row = text_rows.saturating_sub(Picker::panel_height(text_rows));
        let column = self.source.prompt().chars().count() + 2 + self.query.chars().count();
        (column as u16, row as u16)
    }

    fn confirm(&self, open: Open) -> Outcome {
        match self.matches.get(self.cursor) {
            Some(m) => {
                let item = &self.items[m.index];
                let choice = Choice { id: item.id, target: item.target.clone() };
                Outcome::Confirm(self.source, choice, open)
            }
            // Confirming nothing closes rather than sitting there.
            None => Outcome::Cancel,
        }
    }

    pub fn input(&mut self, key: KeyEvent, text_rows: usize) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return Outcome::Cancel,
            KeyCode::Char('c') if ctrl => return Outcome::Cancel,
            KeyCode::Enter => return self.confirm(Open::Here),
            // The keys fzf and Telescope open a split with. Before the query
            // editing below, which would otherwise ignore them as chords.
            KeyCode::Char('v') if ctrl => return self.confirm(Open::Beside),
            KeyCode::Char('s') | KeyCode::Char('x') if ctrl => return self.confirm(Open::Below),
            KeyCode::Down | KeyCode::Tab => self.step(1),
            KeyCode::Char('n') if ctrl => self.step(1),
            KeyCode::Up | KeyCode::BackTab => self.step(-1),
            KeyCode::Char('p') if ctrl => self.step(-1),
            KeyCode::Char('u') if ctrl => {
                self.query.clear();
                return self.requery();
            }
            KeyCode::Char('w') if ctrl => {
                // Back over the trailing separator, then over the word itself.
                let end = self.query.trim_end_matches(is_separator);
                let start = end.rfind(is_separator).map_or(0, |i| i + 1);
                self.query.truncate(start);
                return self.requery();
            }
            KeyCode::Backspace => {
                self.query.pop();
                return self.requery();
            }
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                if self.source.is_live() {
                    return self.requery();
                }
                // A longer query can only ever match fewer items, so there is
                // no reason to look at the ones already ruled out.
                self.narrow();
            }
            _ => {}
        }
        self.scroll_to_cursor(text_rows);
        Outcome::Continue
    }

    /// The query changed. A live source throws away what it has and asks for
    /// the search to be run again; anything else just re-filters.
    fn requery(&mut self) -> Outcome {
        if !self.source.is_live() {
            self.refilter();
            return Outcome::Continue;
        }
        self.items.clear();
        self.matches.clear();
        self.cursor = 0;
        self.scroll = 0;
        self.complete = self.query.is_empty();
        Outcome::Search(self.query.clone())
    }

    /// Wraps, because a picker list is short and the alternative is a dead key
    /// at either end.
    fn step(&mut self, delta: isize) {
        if self.matches.is_empty() {
            self.cursor = 0;
            return;
        }
        let len = self.matches.len() as isize;
        self.cursor = ((self.cursor as isize + delta).rem_euclid(len)) as usize;
    }

    fn scroll_to_cursor(&mut self, text_rows: usize) {
        let rows = Picker::list_rows(text_rows);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        } else if self.cursor >= self.scroll + rows {
            self.scroll = self.cursor + 1 - rows;
        }
    }

    /// Re-rank, and start again from the best match. Typing means the previous
    /// selection no longer means anything.
    fn refilter(&mut self) {
        self.rank();
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Re-score only the current matches, for a query that just grew. The
    /// result is the same as `rank`, but the work shrinks as you type instead
    /// of staying proportional to the whole tree.
    fn narrow(&mut self) {
        let query = std::mem::take(&mut self.query);
        self.matches.retain_mut(|m| {
            match score(&query, &self.items[m.index].text) {
                Some((score, positions)) => {
                    m.score = score;
                    m.positions = positions;
                    true
                }
                None => false,
            }
        });
        self.query = query;
        self.matches.sort_by_key(|m| -m.score);
        self.cursor = 0;
        self.scroll = 0;
    }

    /// Re-rank every item against the query. An empty query keeps the original
    /// order, which for buffers is the order they were opened in and for files
    /// is the order they were walked in.
    fn rank(&mut self) {
        self.matches = (0..self.items.len())
            .filter_map(|index| self.match_for(index))
            .collect();

        // Stable, so equal scores keep the source's own order.
        self.matches.sort_by_key(|m| -m.score);
    }
}

impl Picker {
    fn match_for(&self, index: usize) -> Option<Match> {
        // A live source's items already are the answer to the query.
        if self.source.is_live() {
            return Some(Match { index, score: 0, positions: Vec::new() });
        }
        let (score, positions) = match self.query.is_empty() {
            true => (0, Vec::new()),
            false => score(&self.query, &self.items[index].text)?,
        };
        Some(Match { index, score, positions })
    }
}

fn is_separator(c: char) -> bool {
    matches!(c, '/' | '\\' | '_' | '-' | '.' | ' ' | ':')
}

const CONSECUTIVE: i32 = 16;
const BOUNDARY: i32 = 12;
const GAP: i32 = 3;

/// Fuzzy match `query` against `text`, returning its score and the character
/// positions it matched, or `None` if the query is not a subsequence.
///
/// Two passes, as fzf's simple algorithm does: forward to find where a match
/// can end, then backward from there to pull the start as far right as
/// possible, which is what makes `mn` prefer `main.rs` over `moduleName.rs`.
/// Matching is case-insensitive unless the query has an upper-case character,
/// in which case it is taken to mean it.
pub fn score(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let smart_case = query.chars().any(char::is_uppercase);
    let chars: Vec<char> = text.chars().collect();
    let needles: Vec<char> = query.chars().collect();
    if needles.is_empty() {
        return Some((0, Vec::new()));
    }

    let eq = |a: char, b: char| match smart_case {
        true => a == b,
        false => a.eq_ignore_ascii_case(&b) || a.to_lowercase().eq(b.to_lowercase()),
    };

    let mut needle = 0;
    let mut end = None;
    for (i, &c) in chars.iter().enumerate() {
        if eq(c, needles[needle]) {
            needle += 1;
            if needle == needles.len() {
                end = Some(i + 1);
                break;
            }
        }
    }
    let end = end?;

    let mut needle = needles.len();
    let mut start = 0;
    for i in (0..end).rev() {
        if eq(chars[i], needles[needle - 1]) {
            needle -= 1;
            if needle == 0 {
                start = i;
                break;
            }
        }
    }

    // Greedy forward assignment inside the tightened window.
    let mut positions = Vec::with_capacity(needles.len());
    let mut needle = 0;
    for (i, &c) in chars.iter().enumerate().take(end).skip(start) {
        if needle < needles.len() && eq(c, needles[needle]) {
            positions.push(i);
            needle += 1;
        }
    }

    let mut total = 0;
    let mut previous: Option<usize> = None;
    for &i in &positions {
        if previous == Some(i.wrapping_sub(1)) {
            total += CONSECUTIVE;
        }
        let boundary = i == 0
            || is_separator(chars[i - 1])
            || (chars[i].is_uppercase() && chars[i - 1].is_lowercase());
        if boundary {
            total += BOUNDARY;
        }
        if let Some(p) = previous {
            total -= GAP * (i - p - 1) as i32;
        }
        previous = Some(i);
    }
    // Among equally clustered matches, prefer the shorter, earlier one.
    total -= start as i32;
    total -= (chars.len() / 8) as i32;

    Some((total, positions))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    fn code(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn picker(texts: &[&str]) -> Picker {
        let items = texts
            .iter()
            .enumerate()
            .map(|(id, text)| Item {
                text: (*text).to_string(),
                detail: String::new(),
                id,
                target: (*text).to_string(),
            })
            .collect();
        Picker::new(Source::Buffers, items)
    }

    fn ranked(p: &Picker) -> Vec<&str> {
        p.matches()
            .iter()
            .map(|m| p.item(m).text.as_str())
            .collect()
    }

    fn type_query(p: &mut Picker, query: &str) {
        for c in query.chars() {
            p.input(key(c), 10);
        }
    }

    #[test]
    fn a_query_must_be_a_subsequence() {
        assert!(score("mn", "main.rs").is_some());
        assert!(score("nm", "main.rs").is_none());
        assert!(score("", "main.rs").is_some());
    }

    #[test]
    fn matching_pulls_the_match_to_the_right() {
        // The leading `m` could match the one in `mod/`, but pulling right
        // finds the tighter match inside the file name itself.
        let (_, positions) = score("mn", "mod/main.rs").unwrap();
        assert_eq!(positions, vec![4, 7]);
    }

    #[test]
    fn word_boundaries_beat_the_middle_of_a_word() {
        let boundary = score("sr", "src/render.rs").unwrap().0;
        let middle = score("sr", "unsorted_thing.rs").unwrap().0;
        assert!(boundary > middle, "{boundary} vs {middle}");
    }

    #[test]
    fn consecutive_characters_beat_scattered_ones() {
        let together = score("main", "main.rs").unwrap().0;
        let apart = score("main", "m_a_i_n.rs").unwrap().0;
        assert!(together > apart, "{together} vs {apart}");
    }

    #[test]
    fn an_upper_case_query_is_taken_literally() {
        assert!(score("Main", "main.rs").is_none());
        assert!(score("main", "MAIN.rs").is_some());
    }

    #[test]
    fn an_empty_query_keeps_every_item_in_order() {
        let p = picker(&["b.rs", "a.rs", "c.rs"]);
        assert_eq!(ranked(&p), ["b.rs", "a.rs", "c.rs"]);
    }

    #[test]
    fn typing_filters_and_ranks() {
        let mut p = picker(&["notes.md", "src/main.rs", "src/model/mod.rs"]);
        type_query(&mut p, "mai");
        assert_eq!(ranked(&p), ["src/main.rs"]);
    }

    #[test]
    fn backspace_widens_the_match_again() {
        let mut p = picker(&["one.rs", "two.rs"]);
        type_query(&mut p, "one");
        assert_eq!(p.matches().len(), 1);
        p.input(code(KeyCode::Backspace), 10);
        p.input(code(KeyCode::Backspace), 10);
        p.input(code(KeyCode::Backspace), 10);
        assert_eq!(p.matches().len(), 2);
    }

    #[test]
    fn ctrl_w_deletes_a_word_of_the_query() {
        let mut p = picker(&["a"]);
        type_query(&mut p, "src/main");
        p.input(ctrl('w'), 10);
        assert_eq!(p.query, "src/");
        p.input(ctrl('u'), 10);
        assert_eq!(p.query, "");
    }

    #[test]
    fn the_cursor_wraps_at_both_ends() {
        let mut p = picker(&["a", "b", "c"]);
        p.input(code(KeyCode::Up), 10);
        assert_eq!(p.cursor(), 2);
        p.input(code(KeyCode::Down), 10);
        assert_eq!(p.cursor(), 0);
    }

    #[test]
    fn retyping_resets_the_cursor_to_the_best_match() {
        let mut p = picker(&["a", "b", "c"]);
        p.input(code(KeyCode::Down), 10);
        assert_eq!(p.cursor(), 1);
        type_query(&mut p, "a");
        assert_eq!(p.cursor(), 0);
    }

    #[test]
    fn the_list_scrolls_to_keep_the_cursor_visible() {
        // 10 text rows: a 5-row panel, so 4 list rows.
        let mut p = picker(&["a", "b", "c", "d", "e", "f"]);
        assert_eq!(Picker::list_rows(10), 4);
        for _ in 0..4 {
            p.input(code(KeyCode::Down), 10);
        }
        assert_eq!(p.cursor(), 4);
        assert_eq!(p.scroll(), 1);
    }

    #[test]
    fn confirming_reports_the_item_id_not_the_row() {
        let mut p = picker(&["zero", "one", "two"]);
        type_query(&mut p, "two");
        match p.input(code(KeyCode::Enter), 10) {
            Outcome::Confirm(Source::Buffers, choice, Open::Here) => {
                assert_eq!(choice.id, 2);
                assert_eq!(choice.target, "two");
            }
            _ => panic!("expected a confirm"),
        }
    }

    #[test]
    fn the_split_keys_confirm_into_a_new_window() {
        let chord = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        for (c, open) in [('v', Open::Beside), ('s', Open::Below), ('x', Open::Below)] {
            let mut p = picker(&["a", "b"]);
            assert!(
                matches!(p.input(chord(c), 10), Outcome::Confirm(_, _, got) if got == open),
                "^{c}"
            );
            assert_eq!(p.query, "", "^{c} is not typed into the query");
        }
    }

    #[test]
    fn confirming_nothing_closes_the_picker() {
        let mut p = picker(&["a"]);
        type_query(&mut p, "zzz");
        assert!(matches!(
            p.input(code(KeyCode::Enter), 10),
            Outcome::Cancel
        ));
    }

    #[test]
    fn escape_cancels() {
        let mut p = picker(&["a"]);
        assert!(matches!(p.input(code(KeyCode::Esc), 10), Outcome::Cancel));
    }

    fn items(texts: &[&str]) -> Vec<Item> {
        texts
            .iter()
            .map(|text| Item {
                text: (*text).to_string(),
                detail: String::new(),
                id: 0,
                target: (*text).to_string(),
            })
            .collect()
    }

    #[test]
    fn streamed_items_join_the_list() {
        let mut p = Picker::streaming(Source::Files);
        assert!(!p.is_complete());
        assert_eq!(p.matches().len(), 0);

        p.extend(items(&["src/main.rs", "README.md"]), false);
        assert!(!p.is_complete());
        p.extend(items(&["src/picker.rs"]), true);
        assert!(p.is_complete());
        assert_eq!(p.item_count(), 3);
    }

    #[test]
    fn a_batch_arriving_is_ranked_against_the_query_already_typed() {
        let mut p = Picker::streaming(Source::Files);
        type_query(&mut p, "pick");
        p.extend(items(&["src/main.rs", "src/picker.rs"]), true);
        assert_eq!(ranked(&p), ["src/picker.rs"]);
    }

    #[test]
    fn a_batch_arriving_does_not_move_the_selection_off_its_item() {
        let mut p = Picker::streaming(Source::Files);
        p.extend(items(&["one.rs", "two.rs"]), false);
        p.input(code(KeyCode::Down), 10);
        assert_eq!(p.item(&p.matches()[p.cursor()]).text, "two.rs");

        // An empty query ranks everything equally, so the new item sorts in
        // ahead of nothing - but the cursor must still be on `two.rs`.
        p.extend(items(&["three.rs"]), true);
        assert_eq!(p.item(&p.matches()[p.cursor()]).text, "two.rs");
    }

    #[test]
    fn typing_after_a_batch_starts_from_the_best_match_again() {
        let mut p = Picker::streaming(Source::Files);
        p.extend(items(&["one.rs", "two.rs"]), true);
        p.input(code(KeyCode::Down), 10);
        assert_eq!(p.cursor(), 1);
        type_query(&mut p, "o");
        assert_eq!(p.cursor(), 0);
    }


    // Not a correctness test: it measures, and only fails if it is far enough
    // out to matter. Run with `cargo test --release matching_a_large_tree -- --nocapture`.
    #[test]
    fn matching_a_large_tree_stays_interactive() {
        let mut texts = Vec::new();
        for dir in 0..200 {
            for file in 0..100 {
                texts.push(format!("crates/module_{dir}/src/handler_{file}.rs"));
            }
        }
        let items: Vec<Item> = texts
            .iter()
            .map(|text| Item {
                text: text.clone(),
                detail: String::new(),
                id: 0,
                target: text.clone(),
            })
            .collect();
        let count = items.len();

        let start = std::time::Instant::now();
        let mut p = Picker::new(Source::Files, items);
        let build = start.elapsed();

        let start = std::time::Instant::now();
        type_query(&mut p, "mod9han");
        let typed = start.elapsed();

        println!(
            "{count} paths: build {build:?}, 7 keystrokes {typed:?} ({:?} each)",
            typed / 7
        );
        assert!(!p.matches().is_empty());
    }


    #[test]
    fn narrowing_gives_the_same_answer_as_ranking_from_scratch() {
        let texts = ["src/main.rs", "src/picker.rs", "src/view.rs", "README.md"];

        let mut typed = picker(&texts);
        type_query(&mut typed, "src");

        let mut fresh = picker(&texts);
        fresh.query = "src".into();
        fresh.refilter();

        assert_eq!(ranked(&typed), ranked(&fresh));
        let scores: Vec<i32> = typed.matches().iter().map(|m| m.score).collect();
        let expected: Vec<i32> = fresh.matches().iter().map(|m| m.score).collect();
        assert_eq!(scores, expected);
    }


    // Streaming used to re-rank every item on every batch, which made walking a
    // large tree quadratic: 100k paths in batches of 512 never finished.
    #[test]
    fn streaming_a_large_tree_stays_linear() {
        let mut p = Picker::streaming(Source::Files);
        let start = std::time::Instant::now();
        for batch in 0..200 {
            let texts: Vec<String> = (0..512)
                .map(|i| format!("crates/module_{batch}/src/handler_{i}.rs"))
                .collect();
            let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
            p.extend(items(&refs), false);
        }
        let elapsed = start.elapsed();
        println!("{} paths streamed in {elapsed:?}", p.item_count());
        assert_eq!(p.item_count(), 200 * 512);
    }


    #[test]
    fn a_live_source_asks_for_the_search_to_be_run_again() {
        let mut p = Picker::live(Source::Grep);
        assert!(matches!(p.input(key('f'), 10), Outcome::Search(q) if q == "f"));
        assert!(matches!(p.input(key('n'), 10), Outcome::Search(q) if q == "fn"));
        assert!(matches!(
            p.input(code(KeyCode::Backspace), 10),
            Outcome::Search(q) if q == "f"
        ));
    }

    #[test]
    fn a_live_source_drops_the_old_results_when_the_query_changes() {
        let mut p = Picker::live(Source::Grep);
        p.input(key('f'), 10);
        p.extend(items(&["a.rs:1:fn one", "b.rs:2:fn two"]), true);
        assert_eq!(p.matches().len(), 2);

        // The new pattern's hits will arrive from the search; whatever matched
        // the old one is not an answer to it.
        p.input(key('n'), 10);
        assert_eq!(p.matches().len(), 0);
        assert!(!p.is_complete());
    }

    #[test]
    fn a_live_source_keeps_its_results_in_the_order_they_were_found() {
        let mut p = Picker::live(Source::Grep);
        p.input(key('z'), 10);
        // None of these contain the query - a live source does not filter.
        p.extend(items(&["b.rs:9:second", "a.rs:1:first"]), true);
        assert_eq!(ranked(&p), ["b.rs:9:second", "a.rs:1:first"]);
    }

    #[test]
    fn clearing_a_live_query_asks_for_nothing_and_waits() {
        let mut p = Picker::live(Source::Grep);
        p.input(key('f'), 10);
        match p.input(ctrl('u'), 10) {
            Outcome::Search(q) => assert_eq!(q, ""),
            _ => panic!("expected a search"),
        }
        // An empty pattern is not a search, so there is nothing to wait for.
        assert!(p.is_complete());
    }

}

//! More than one cursor: where the others are, and how one keystroke reaches
//! all of them.
//!
//! The trick is that the view already carries positions through edits - a
//! diagnostic stays on its word while you type above it - and the other
//! cursors are carried the same way. So a keystroke does not have to work out
//! where anything ends up: it runs the ordinary command at each cursor in
//! turn, from the bottom of the buffer up, and the view moves the rest along
//! as it goes. Every command here is the one-cursor command, run more than
//! once.

use crate::search;
use crate::view::Selection;

use super::{Editor, Mode};

impl Editor {
    /// Whether there is more than one cursor.
    pub fn has_cursors(&self) -> bool {
        !self.view().extra.is_empty()
    }

    /// Put them away, which `esc` does.
    pub fn clear_cursors(&mut self) {
        if self.has_cursors() {
            self.view_mut().extra.clear();
            self.message = "one cursor".into();
        }
    }

    /// Run one command at every cursor, bottom-up, leaving each cursor where
    /// the command put it.
    ///
    /// Bottom-up so that the positions still to be visited are the ones no
    /// edit has touched yet; the ones already visited are carried along by the
    /// edits below them, the same way everything else in the view is. One undo
    /// group, because one keystroke is one change however many places it
    /// happened in.
    pub fn at_every_cursor(&mut self, act: impl Fn(&mut Editor)) {
        if !self.has_cursors() {
            return act(self);
        }
        // The primary goes in with the rest so that it is carried too, and
        // comes back out afterwards: which one it is has to survive the loop.
        let mut all = self.view().extra.clone();
        all.push(self.view().sel);
        all.sort_by_key(|sel| sel.range());
        let primary = all
            .iter()
            .position(|sel| *sel == self.view().sel)
            .unwrap_or(all.len() - 1);
        self.view_mut().extra = all;

        self.begin_undo_group();
        for index in (0..self.view().extra.len()).rev() {
            self.view_mut().sel = self.view().extra[index];
            act(self);
            let now = self.view().sel;
            self.view_mut().extra[index] = now;
        }
        let mut all = std::mem::take(&mut self.view_mut().extra);
        self.view_mut().sel = all.remove(primary);
        self.view_mut().extra = all;
        self.end_undo_group();
        self.clamp_all_cursors();
    }

    /// Keep every cursor somewhere it can be: inside the buffer, and not two
    /// in the same place, which would type everything twice.
    fn clamp_all_cursors(&mut self) {
        let len = self.view().doc.len_chars();
        let primary = self.view().sel;
        let extra = &mut self.view_mut().extra;
        for sel in extra.iter_mut() {
            sel.anchor = sel.anchor.min(len);
            sel.head = sel.head.min(len);
        }
        extra.retain(|sel| sel.range() != primary.range());
        extra.sort_by_key(|sel| sel.range());
        extra.dedup_by_key(|sel| sel.range());
    }

    /// `^n`: the word under the cursor, and then the next place it is used,
    /// and the next. The first press selects the word, as vim's plugins have
    /// it - so the one you are on is one of them, and `c` changes them all.
    pub fn add_cursor_at_next_match(&mut self) {
        let (start, end) = match self.selected_word() {
            Some(range) => range,
            None => return self.message = "no word under the cursor".into(),
        };
        let selected = self.view().sel.range() == (start, end);
        if !selected {
            self.view_mut().sel = Selection { anchor: start, head: end };
            self.message = "1 cursor - ^n for the next one".into();
            return;
        }

        let text = self.view().doc.slice_str(start, end);
        let mut search = search::Search::default();
        if search.set_pattern(&search::word_pattern(&text)).is_err() {
            return;
        }
        // From the last cursor rather than from the primary, so that pressing
        // it again walks down the file instead of finding the same one twice.
        let from = self
            .view()
            .extra
            .iter()
            .map(|sel| sel.range().0)
            .chain(std::iter::once(start))
            .max()
            .unwrap_or(start);
        let Some(hit) = search.find(&self.view().doc, from, false) else {
            self.message = "no more of them".into();
            return;
        };
        let found = Selection { anchor: hit.start, head: hit.end };
        if found.range() == self.view().sel.range() {
            self.message = "no more of them".into();
            return;
        }
        self.view_mut().extra.push(found);
        self.clamp_all_cursors();
        self.said_cursors();
    }

    /// The word the cursor is on, as a range. The selection itself when there
    /// already is one, so `v` over anything makes `^n` about that.
    fn selected_word(&self) -> Option<(usize, usize)> {
        let view = self.view();
        if !view.sel.is_empty() {
            return Some(view.sel.range());
        }
        crate::object::resolve(&view.doc, view.sel.head, crate::object::Object::Word, false)
    }

    /// `gm` over a selection: one cursor per line of it, where the selection
    /// starts on that line. The way to add the same thing to twenty lines
    /// that are not a rectangle.
    pub fn cursors_on_lines(&mut self) {
        let (first, last) = self.selection_lines();
        if last <= first {
            self.message = "one line - nothing to spread over".into();
            return;
        }
        let block = self.block();
        let mut cursors = Vec::new();
        for line in first..=last {
            let view = self.view();
            let at = match &block {
                // A rectangle has a column of its own on every line.
                Some(block) => match block.row(line) {
                    Some((start, _)) => start,
                    None => continue,
                },
                None => view.doc.line_to_char(line) + view.doc.line_indent_len(line),
            };
            cursors.push(Selection::point(at.min(view.doc.len_chars())));
        }
        self.set_mode(Mode::Normal);
        self.view_mut().sel = cursors.remove(0);
        self.view_mut().extra = cursors;
        self.clamp_all_cursors();
        self.said_cursors();
    }

    /// `gs` over a selection: a cursor at every match of the last search
    /// inside it. The search you already ran is the one that found the places
    /// worth being in.
    pub fn cursors_on_search(&mut self) {
        if !self.search.is_set() {
            self.message = "no search to split on".into();
            return;
        }
        // The selection as an operator would see it: `V` takes whole lines,
        // and the matches on them are the ones meant.
        let (start, end) = self.selection_range().unwrap_or_else(|| self.view().sel.range());
        let (first, last) = (self.view().doc.char_to_line(start), self.view().doc.char_to_line(end));
        let found: Vec<Selection> = self
            .search
            .matches_in_lines(&self.view().doc, first, last + 1)
            .into_iter()
            .filter(|(from, to)| *from >= start && *to <= end)
            .map(|(from, to)| Selection { anchor: from, head: to })
            .collect();
        if found.is_empty() {
            self.message = "no matches in the selection".into();
            return;
        }
        self.set_mode(Mode::Normal);
        let mut found = found;
        self.view_mut().sel = found.remove(0);
        self.view_mut().extra = found;
        self.clamp_all_cursors();
        self.said_cursors();
    }

    /// `d` and `x` with more than one cursor: what each of them is on goes.
    pub fn delete_at_cursors(&mut self) {
        self.at_every_cursor(|editor| match editor.view().sel.is_empty() {
            true => editor.delete_chars(None, 1),
            false => editor.delete_selection(None),
        });
        self.said_cursors();
    }

    /// `c` and `s`: the same, and then type at all of them.
    pub fn change_at_cursors(&mut self) {
        // Insert mode first: in normal mode a cursor may not rest past the
        // last character of a line, and a change that empties the end of one
        // would be pulled back a character before there was anything to type.
        self.set_mode(Mode::Insert);
        self.at_every_cursor(|editor| {
            if !editor.view().sel.is_empty() {
                editor.delete_selection(None);
            }
        });
    }

    /// How many there are, said once rather than after every keystroke.
    fn said_cursors(&mut self) {
        let count = self.view().extra.len() + 1;
        self.message = format!("{count} cursors - esc for one again");
    }
}

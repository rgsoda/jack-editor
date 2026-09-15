use crate::buffer::{Document, Edit};
use crate::editor::Selection;

/// One replacement: at `pos`, `removed` becomes `inserted`.
///
/// `pos` is a char index in the document as it was *before* the containing
/// transaction was applied, so a transaction is self-describing and can be
/// inverted without consulting the document.
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub pos: usize,
    pub removed: String,
    pub inserted: String,
}

impl Change {
    fn removed_len(&self) -> usize {
        self.removed.chars().count()
    }

    fn inserted_len(&self) -> usize {
        self.inserted.chars().count()
    }

    fn is_insert(&self) -> bool {
        self.removed.is_empty() && !self.inserted.is_empty()
    }

    fn is_delete(&self) -> bool {
        self.inserted.is_empty() && !self.removed.is_empty()
    }
}

/// One undo step: the edits, plus where the cursor was before and after.
/// Restoring the selection is what makes undo land somewhere useful.
#[derive(Clone, Debug)]
pub struct Transaction {
    /// Sorted ascending by `pos`, all in pre-transaction coordinates.
    pub changes: Vec<Change>,
    pub sel_before: Selection,
    pub sel_after: Selection,
}

impl Transaction {
    pub fn new(changes: Vec<Change>, sel_before: Selection, sel_after: Selection) -> Self {
        Transaction { changes, sel_before, sel_after }
    }

    /// Returns the applied edits in document coordinates, for tree-sitter.
    pub fn apply(&self, doc: &mut Document) -> Vec<Edit> {
        // Each change's pos is in pre-transaction coordinates, so walking
        // ascending means carrying the length shift from earlier changes.
        let mut shift: isize = 0;
        let mut edits = Vec::with_capacity(self.changes.len());
        for change in &self.changes {
            let pos = (change.pos as isize + shift) as usize;
            edits.push(doc.replace(pos, change.removed_len(), &change.inserted));
            shift += change.inserted_len() as isize - change.removed_len() as isize;
        }
        edits
    }

    /// The transaction that undoes this one, in post-transaction coordinates.
    pub fn invert(&self) -> Transaction {
        let mut shift: isize = 0;
        let changes = self
            .changes
            .iter()
            .map(|change| {
                let pos = (change.pos as isize + shift) as usize;
                shift += change.inserted_len() as isize - change.removed_len() as isize;
                Change {
                    pos,
                    removed: change.inserted.clone(),
                    inserted: change.removed.clone(),
                }
            })
            .collect();

        Transaction {
            changes,
            sel_before: self.sel_after,
            sel_after: self.sel_before,
        }
    }
}

#[derive(Default)]
pub struct History {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// Undo depth the document was last saved at. `None` means the saved state
    /// is no longer reachable by undoing.
    saved_depth: Option<usize>,
}

impl History {
    pub fn new() -> Self {
        History { undo: Vec::new(), redo: Vec::new(), saved_depth: Some(0) }
    }

    pub fn push(&mut self, tx: Transaction) {
        self.redo.clear();
        // A new edit after undoing strands any save point above us.
        if self.saved_depth.is_some_and(|depth| depth > self.undo.len()) {
            self.saved_depth = None;
        }

        // Merging into the transaction at the save point would change the
        // document without changing the depth, so `is_modified` would lie.
        if self.saved_depth != Some(self.undo.len())
            && let Some(last) = self.undo.last_mut()
            && coalesce(last, &tx)
        {
            return;
        }

        self.undo.push(tx);
    }

    /// Pops the last transaction and returns its inverse, ready to apply.
    pub fn undo(&mut self) -> Option<Transaction> {
        let tx = self.undo.pop()?;
        let inverse = tx.invert();
        self.redo.push(tx);
        Some(inverse)
    }

    pub fn redo(&mut self) -> Option<Transaction> {
        let tx = self.redo.pop()?;
        self.undo.push(tx.clone());
        Some(tx)
    }

    pub fn is_modified(&self) -> bool {
        self.saved_depth != Some(self.undo.len())
    }

    pub fn mark_saved(&mut self) {
        self.saved_depth = Some(self.undo.len());
    }
}

/// Fold `tx` into `last` if they are one continuous run of typing or deleting,
/// so undo steps back by a word-ish chunk rather than a keystroke.
fn coalesce(last: &mut Transaction, tx: &Transaction) -> bool {
    let (Some(prev), Some(next)) = (last.changes.first(), tx.changes.first()) else {
        return false;
    };
    if last.changes.len() != 1 || tx.changes.len() != 1 {
        return false;
    }
    // A line break is always an undo boundary.
    if prev.removed.contains('\n')
        || prev.inserted.contains('\n')
        || next.removed.contains('\n')
        || next.inserted.contains('\n')
    {
        return false;
    }

    if prev.is_insert() && next.is_insert() && next.pos == prev.pos + prev.inserted_len() {
        // Typing forward.
        last.changes[0].inserted.push_str(&next.inserted);
    } else if prev.is_delete() && next.is_delete() && next.pos + next.removed_len() == prev.pos {
        // Backspacing leftward.
        let mut removed = next.removed.clone();
        removed.push_str(&prev.removed);
        last.changes[0].removed = removed;
        last.changes[0].pos = next.pos;
    } else if prev.is_delete() && next.is_delete() && next.pos == prev.pos {
        // Deleting forward.
        last.changes[0].removed.push_str(&next.removed);
    } else {
        return false;
    }

    last.sel_after = tx.sel_after;
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;

    fn doc(text: &str) -> Document {
        let mut d = Document::scratch();
        d.text = Rope::from_str(text);
        d
    }

    fn change(pos: usize, removed: &str, inserted: &str) -> Change {
        Change { pos, removed: removed.into(), inserted: inserted.into() }
    }

    fn tx(changes: Vec<Change>) -> Transaction {
        Transaction::new(changes, Selection::point(0), Selection::point(0))
    }

    #[test]
    fn multiple_changes_apply_in_original_coordinates() {
        let mut d = doc("abcdef");
        tx(vec![change(1, "b", "XY"), change(4, "e", "")]).apply(&mut d);
        assert_eq!(d.text.to_string(), "aXYcdf");
    }

    #[test]
    fn inverting_a_multi_change_transaction_round_trips() {
        let original = "abcdef";
        let mut d = doc(original);
        let t = tx(vec![change(1, "b", "XY"), change(4, "e", "")]);
        t.apply(&mut d);
        t.invert().apply(&mut d);
        assert_eq!(d.text.to_string(), original);
    }

    #[test]
    fn inverting_swaps_the_selections() {
        let t = Transaction::new(vec![change(0, "", "x")], Selection::point(3), Selection::point(7));
        let inverse = t.invert();
        assert_eq!(inverse.sel_before.head, 7);
        assert_eq!(inverse.sel_after.head, 3);
    }

    #[test]
    fn a_new_edit_after_undoing_strands_the_save_point() {
        let mut h = History::new();
        h.push(tx(vec![change(0, "", "a")]));
        h.mark_saved();
        assert!(!h.is_modified());

        h.undo();
        assert!(h.is_modified());

        // Redo history is dropped, so the saved state is unreachable...
        h.push(tx(vec![change(0, "", "b")]));
        assert!(h.is_modified());
        // ...and stays unreachable even at the same depth.
        h.undo();
        assert!(h.is_modified());
    }

    #[test]
    fn undoing_back_to_the_save_point_clears_modified() {
        let mut h = History::new();
        h.push(tx(vec![change(0, "", "a")]));
        h.mark_saved();
        h.push(tx(vec![change(1, "", "b")]));
        assert!(h.is_modified());
        h.undo();
        assert!(!h.is_modified());
    }

    #[test]
    fn edits_never_merge_into_the_transaction_at_the_save_point() {
        let mut h = History::new();
        h.push(tx(vec![change(0, "", "a")]));
        h.mark_saved();
        // Contiguous typing, but merging here would leave is_modified false.
        h.push(tx(vec![change(1, "", "b")]));
        assert!(h.is_modified());
    }
}

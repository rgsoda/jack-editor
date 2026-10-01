//! What came out of the orchard, and what it was worth.
//!
//! The ledger is append-only: a picking is written once and never edited, and
//! a correction is another row saying so. Totals are worked out from the rows
//! rather than kept alongside them, because a total that can disagree with
//! its rows eventually does.

use std::collections::BTreeMap;
use std::fmt;

/// A row of the ledger. One picking, by one person, of one variety.
#[derive(Clone, Debug)]
pub struct Picking {
    pub day: Day,
    pub picker: String,
    pub variety: String,
    pub grams: u32,
    /// What it was sold for, in pence, when it has been sold. A picking that
    /// went into the kitchen instead has nothing here and never will.
    pub pence: Option<u32>,
}

/// A day of the season, counted from the first picking rather than from the
/// first of the month: the season is the unit anyone here thinks in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Day(pub u16);

impl fmt::Display for Day {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "day {}", self.0)
    }
}

impl Picking {
    pub fn new(day: u16, picker: &str, variety: &str, grams: u32) -> Self {
        Picking {
            day: Day(day),
            picker: picker.to_string(),
            variety: variety.to_string(),
            grams,
            pence: None,
        }
    }

    /// The same picking, sold. Takes `self` rather than borrowing it: a
    /// picking is sold once, and a method that could be called twice would
    /// need to decide what the second call meant.
    pub fn sold(mut self, pence: u32) -> Self {
        self.pence = Some(pence);
        self
    }

    /// Pence per kilogram, where there is a price and there is any weight.
    /// Dividing by a weight nobody recorded is the one case worth a `None`
    /// rather than a zero.
    pub fn rate(&self) -> Option<u32> {
        match (self.pence, self.grams) {
            (Some(pence), grams) if grams > 0 => Some(pence * 1000 / grams),
            _ => None,
        }
    }

    pub fn kept(&self) -> bool {
        self.pence.is_none()
    }
}

/// Every row, in the order they were written.
#[derive(Default)]
pub struct Ledger {
    rows: Vec<Picking>,
}

impl Ledger {
    pub fn push(&mut self, picking: Picking) {
        self.rows.push(picking);
    }

    pub fn rows(&self) -> &[Picking] {
        &self.rows
    }

    /// Grams by variety, for the board on the barn door.
    pub fn by_variety(&self) -> BTreeMap<&str, u32> {
        let mut totals = BTreeMap::new();
        for row in &self.rows {
            *totals.entry(row.variety.as_str()).or_insert(0) += row.grams;
        }
        totals
    }

    /// Grams by picker, and the same thing said twice over: the map is what
    /// the wages are worked out from, and the order is what gets read out.
    pub fn by_picker(&self) -> Vec<(&str, u32)> {
        let mut totals: BTreeMap<&str, u32> = BTreeMap::new();
        for row in &self.rows {
            *totals.entry(row.picker.as_str()).or_insert(0) += row.grams;
        }
        let mut ordered: Vec<(&str, u32)> = totals.into_iter().collect();
        ordered.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        ordered
    }

    /// What the season took, in pence. Pickings that were kept are not
    /// losses and are not counted either way.
    pub fn takings(&self) -> u32 {
        self.rows.iter().filter_map(|row| row.pence).sum()
    }

    /// The best rate anyone got for a variety, which is the number the
    /// arguments in the barn are actually about.
    pub fn best_rate(&self, variety: &str) -> Option<(&str, u32)> {
        let mut best: Option<(&str, u32)> = None;
        for row in &self.rows {
            if row.variety != variety {
                continue;
            }
            if let Some(rate) = row.rate() {
                match best {
                    Some((_, so_far)) if so_far >= rate => (),
                    _ => best = Some((row.picker.as_str(), rate)),
                }
            }
        }
        best
    }

    /// A run of days with nothing picked, which is either weather or nobody
    /// writing anything down. The ledger cannot tell the difference and does
    /// not pretend to.
    pub fn quiet_spells(&self) -> Vec<(Day, Day)> {
        let mut days: Vec<u16> = self.rows.iter().map(|row| row.day.0).collect();
        days.sort_unstable();
        days.dedup();

        let mut spells = Vec::new();
        for pair in days.windows(2) {
            if let [before, after] = pair {
                if after - before > 1 {
                    spells.push((Day(before + 1), Day(after - 1)));
                }
            }
        }
        spells
    }

    /// Everything, as it would go on the board: variety, weight, and what the
    /// kitchen took out of it.
    pub fn board(&self) -> String {
        let mut lines = Vec::new();
        for (variety, grams) in self.by_variety() {
            let kept: u32 = self
                .rows
                .iter()
                .filter(|row| row.variety == variety && row.kept())
                .map(|row| row.grams)
                .sum();
            match kept {
                0 => lines.push(format!("{variety}: {grams}g")),
                kept => lines.push(format!("{variety}: {grams}g, {kept}g to the kitchen")),
            }
        }
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rate_needs_a_price_and_a_weight() {
        let picking = Picking::new(3, "Rowan", "damson", 2000);
        assert_eq!(picking.rate(), None, "nothing sold has no rate");
        assert_eq!(picking.clone().sold(900).rate(), Some(450));
    }

    #[test]
    fn the_board_says_what_the_kitchen_took() {
        let mut ledger = Ledger::default();
        ledger.push(Picking::new(1, "Rowan", "bullace", 1200).sold(500));
        ledger.push(Picking::new(1, "Nell", "bullace", 300));
        assert_eq!(ledger.board(), "bullace: 1500g, 300g to the kitchen");
        assert_eq!(ledger.takings(), 500);
    }

    #[test]
    fn a_gap_in_the_days_is_a_quiet_spell() {
        let mut ledger = Ledger::default();
        ledger.push(Picking::new(1, "Rowan", "sloe", 80));
        ledger.push(Picking::new(5, "Rowan", "sloe", 120));
        assert_eq!(ledger.quiet_spells(), vec![(Day(2), Day(4))]);
    }
}

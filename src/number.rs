//! `^a` and `g-`: the number under the cursor, one more or one less.
//!
//! Finding it is the whole of the problem. The cursor is rarely *on* the digit
//! you mean - it is at the start of the line, or on the `x` of `x = 41` - so
//! the number wanted is the one the cursor is in, else the next one along the
//! line. Vim settled that rule decades ago and everyone's fingers know it.
//!
//! Chars rather than bytes, because the editor edits in chars and a line with
//! an é in it before the number would otherwise be off by one.

/// A number found on a line: where it is, what it is, and how it was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number {
    /// Char offsets into the line, the sign included when there was one.
    pub start: usize,
    pub end: usize,
    pub value: i64,
    /// The digits as they were written, for `007` to stay three wide.
    pub width: usize,
    pub padded: bool,
}

impl Number {
    /// The same number `by` further on, written the way it was written: a
    /// padded one keeps its width, and one that has outgrown its padding is
    /// allowed to.
    pub fn bumped(&self, by: i64) -> String {
        let value = self.value.saturating_add(by);
        let digits = value.unsigned_abs().to_string();
        let sign = if value < 0 { "-" } else { "" };
        match self.padded && digits.len() < self.width {
            true => format!("{sign}{}{digits}", "0".repeat(self.width - digits.len())),
            false => format!("{sign}{digits}"),
        }
    }
}

/// The number the cursor is in, else the first one to its right.
///
/// A `-` immediately in front is part of the number, as vim has it, so `-1`
/// becomes `0` rather than `-2`. With one difference: a `-` with a digit in
/// front of it is a subtraction rather than a sign, so `9-1` goes to `9-2` and
/// not to `90`. Vim gives `90` there, and it is the one place where matching
/// vim means silently changing what an expression says.
pub fn find(line: &str, column: usize) -> Option<Number> {
    let chars: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < chars.len() {
        if !chars[at].is_ascii_digit() {
            at += 1;
            continue;
        }
        // The whole run of digits, and the sign in front of it.
        let mut start = at;
        let mut end = at;
        while end < chars.len() && chars[end].is_ascii_digit() {
            end += 1;
        }
        // Past it already: keep looking.
        if column >= end {
            at = end;
            continue;
        }
        let digits: String = chars[start..end].iter().collect();
        let negative = start > 0
            && chars[start - 1] == '-'
            && (start < 2 || !chars[start - 2].is_ascii_digit());
        if negative {
            start -= 1;
        }
        let value: i64 = digits.parse().unwrap_or(i64::MAX);
        let value = match negative {
            true => -value,
            false => value,
        };
        return Some(Number {
            start,
            end,
            value,
            width: digits.len(),
            padded: digits.len() > 1 && digits.starts_with('0'),
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bumped(line: &str, column: usize, by: i64) -> Option<String> {
        let number = find(line, column)?;
        let mut after: String = line.chars().take(number.start).collect();
        after.push_str(&number.bumped(by));
        after.extend(line.chars().skip(number.end));
        Some(after)
    }

    #[test]
    fn the_number_is_the_one_the_cursor_is_in_or_the_next_one_along() {
        // On it, before it, in the middle of it.
        assert_eq!(bumped("x = 41", 0, 1).as_deref(), Some("x = 42"));
        assert_eq!(bumped("x = 41", 4, 1).as_deref(), Some("x = 42"));
        assert_eq!(bumped("x = 41", 5, 1).as_deref(), Some("x = 42"));
        // Past it: the next one, and nothing when there is no next one.
        assert_eq!(bumped("1 and 7", 2, 1).as_deref(), Some("1 and 8"));
        assert_eq!(bumped("x = 41", 6, 1), None);
        assert_eq!(bumped("no numbers here", 0, 1), None);
    }

    #[test]
    fn a_minus_in_front_belongs_to_the_number() {
        assert_eq!(bumped("-1", 0, 1).as_deref(), Some("0"));
        assert_eq!(bumped("x = -1", 0, -1).as_deref(), Some("x = -2"));
        assert_eq!(bumped("0", 0, -1).as_deref(), Some("-1"));
        // A digit before the minus means it is a subtraction, not a sign -
        // where vim would give `90` and quietly change what the line says.
        assert_eq!(bumped("9-1", 2, 1).as_deref(), Some("9-2"));
        assert_eq!(bumped("x-1", 2, 1).as_deref(), Some("x0"), "but a name in front is vim's");
    }

    #[test]
    fn padding_is_kept_until_the_number_outgrows_it() {
        assert_eq!(bumped("007", 0, 1).as_deref(), Some("008"));
        assert_eq!(bumped("007", 0, -8).as_deref(), Some("-001"));
        assert_eq!(bumped("099", 0, 1).as_deref(), Some("100"));
        assert_eq!(bumped("999", 0, 1).as_deref(), Some("1000"), "no padding to keep");
        assert_eq!(bumped("10", 0, -1).as_deref(), Some("9"), "9 was never padded");
    }

    #[test]
    fn a_count_is_how_far_and_the_ends_are_not_wrapped_round() {
        assert_eq!(bumped("x = 8", 0, 34).as_deref(), Some("x = 42"));
        let huge = format!("{}", i64::MAX);
        assert_eq!(bumped(&huge, 0, 1).as_deref(), Some(huge.as_str()), "stops rather than wraps");
    }

    #[test]
    fn columns_are_chars_so_what_is_in_front_can_be_anything() {
        assert_eq!(bumped("héllo 41", 0, 1).as_deref(), Some("héllo 42"));
        let found = find("héllo 41", 0).expect("the number");
        assert_eq!((found.start, found.end), (6, 8), "in chars, not bytes");
    }
}

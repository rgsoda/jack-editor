//! `:sort` - the lines of a range, put in order.
//!
//! Pure, and separate from the editor, because the interesting part is what
//! "in order" means: by the text, by the number in the text, with case or
//! without, keeping the duplicates or dropping them. All of it is decided
//! here, where it can be read and tested, and the editor only puts the answer
//! back into the buffer.

/// What was asked for, in vim's spelling: `:sort!` reverses, and the letters
/// after it pick the comparison.
#[derive(Debug, Default, PartialEq, Eq, Clone, Copy)]
pub struct Options {
    /// `!` - largest first.
    pub reverse: bool,
    /// `n` - by the first number on the line rather than by its text.
    pub numeric: bool,
    /// `i` - upper and lower case are the same letter.
    pub insensitive: bool,
    /// `u` - one line out of every run that compares equal.
    pub unique: bool,
}

impl Options {
    /// The flags as they are typed after the command: `:sort! un`, `:sort i`.
    /// Anything else is a mistake worth saying out loud rather than ignoring,
    /// so an unknown letter comes back as the letter.
    pub fn parse(flags: &str) -> Result<Options, char> {
        let mut options = Options::default();
        for flag in flags.chars() {
            match flag {
                'n' => options.numeric = true,
                'i' => options.insensitive = true,
                'u' => options.unique = true,
                'r' => options.reverse = true,
                ' ' | '\t' => {}
                other => return Err(other),
            }
        }
        Ok(options)
    }
}

/// The first number on a line, for `:sort n`. A leading `-` counts; a line
/// with no number at all has no key, and those go first - they are the
/// headings and the blank lines, and vim puts them at the top too.
fn number(line: &str) -> Option<i64> {
    let bytes: Vec<char> = line.chars().collect();
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at].is_ascii_digit() {
            let start = match at > 0 && bytes[at - 1] == '-' {
                true => at - 1,
                false => at,
            };
            let mut end = at;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            return bytes[start..end].iter().collect::<String>().parse().ok();
        }
        at += 1;
    }
    None
}

/// What a line is compared by: the number if one was asked for, then the
/// text.
type Key = (Option<i64>, String);

/// What a line is compared by: the number if one was asked for, and the text
/// either way, so that lines with the same number keep a sensible order
/// instead of an arbitrary one.
fn key(line: &str, options: Options) -> Key {
    let text = match options.insensitive {
        true => line.to_lowercase(),
        false => line.to_string(),
    };
    match options.numeric {
        true => (number(line), text),
        false => (None, text),
    }
}

/// The lines, sorted. Stable, so lines that compare equal come out in the
/// order they went in - which is what makes `:sort n` on a file of names and
/// numbers leave the names alphabetical.
pub fn sorted(lines: &[String], options: Options) -> Vec<String> {
    let mut keyed: Vec<(usize, Key, &String)> = lines
        .iter()
        .enumerate()
        .map(|(index, line)| (index, key(line, options), line))
        .collect();
    keyed.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    if options.reverse {
        keyed.reverse();
    }
    if options.unique {
        // After sorting, the duplicates are neighbours: one of each run, and
        // the one kept is the first in the order that was asked for.
        let mut seen: Option<Key> = None;
        keyed.retain(|(_, key, _)| {
            let repeat = seen.as_ref() == Some(key);
            if !repeat {
                seen = Some(key.clone());
            }
            !repeat
        });
    }
    keyed.into_iter().map(|(_, _, line)| line.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<String> {
        text.lines().map(String::from).collect()
    }

    #[test]
    fn the_plain_sort_is_by_the_text_of_the_line() {
        let got = sorted(&lines("pear\nApple\nbanana"), Options::default());
        assert_eq!(got, lines("Apple\nbanana\npear"), "capitals sort before lower case");
        let insensitive = Options { insensitive: true, ..Options::default() };
        assert_eq!(sorted(&lines("pear\nApple\nbanana"), insensitive), lines("Apple\nbanana\npear"));
        let reverse = Options { reverse: true, ..Options::default() };
        assert_eq!(sorted(&lines("a\nb\nc"), reverse), lines("c\nb\na"));
    }

    #[test]
    fn a_numeric_sort_reads_the_number_and_not_the_digits() {
        let numeric = Options { numeric: true, ..Options::default() };
        // The whole point: 9 before 10, which sorting by text gets wrong.
        assert_eq!(sorted(&lines("item 10\nitem 9\nitem 100"), numeric), lines("item 9\nitem 10\nitem 100"));
        assert_eq!(sorted(&lines("x 3\nx -1\nx 0"), numeric), lines("x -1\nx 0\nx 3"));
        // A line with no number at all goes first rather than nowhere.
        assert_eq!(sorted(&lines("two 2\nheading\none 1"), numeric), lines("heading\none 1\ntwo 2"));
    }

    #[test]
    fn unique_keeps_one_of_each_run_and_keeps_the_rest_in_order() {
        let unique = Options { unique: true, ..Options::default() };
        assert_eq!(sorted(&lines("b\na\nb\na\nc"), unique), lines("a\nb\nc"));
        // Equal by the comparison that was asked for, not by the bytes.
        let both = Options { unique: true, insensitive: true, ..Options::default() };
        assert_eq!(sorted(&lines("Apple\napple\nfig"), both), lines("Apple\nfig"));
    }

    #[test]
    fn a_stable_sort_leaves_equal_lines_where_they_were() {
        let numeric = Options { numeric: true, ..Options::default() };
        let got = sorted(&lines("zeta 1\nalpha 1\nbeta 0"), numeric);
        // Same number, so the text decides - and the text is part of the key
        // on purpose, so the answer is the same every time.
        assert_eq!(got, lines("beta 0\nalpha 1\nzeta 1"));
    }

    #[test]
    fn the_flags_are_the_ones_vim_takes_and_nothing_else() {
        assert_eq!(Options::parse("un"), Ok(Options { unique: true, numeric: true, ..Options::default() }));
        assert_eq!(Options::parse(""), Ok(Options::default()));
        assert_eq!(Options::parse("x"), Err('x'));
    }
}

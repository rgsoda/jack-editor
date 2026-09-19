//! `:s` - the substitute command's grammar.
//!
//! Parsing only: what lines, what pattern, what to put there and how. The
//! editing is `Editor::substitute`, which is where a document exists to be
//! edited. Kept apart because the grammar is the fiddly half - `:%s#a#b#gi` is
//! a range, a delimiter that is not `/`, a pattern, a replacement and two
//! flags, and none of that needs a buffer to test.

/// One end of a range: the ways vim lets you name a line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    /// A line number, as typed - one-based.
    Line(usize),
    /// `.`, the line the cursor is on.
    Current,
    /// `$`, the last line.
    Last,
    /// `'<` and `'>`, the ends of the last visual selection.
    VisualStart,
    VisualEnd,
}

/// Which lines the command runs over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lines {
    /// No range given: the current line, as vim has it.
    Current,
    /// `%`, the whole buffer.
    Whole,
    Range(Address, Address),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Flags {
    /// `g`: every match on a line, not just the first.
    pub global: bool,
    /// `i` and `I`: force case-insensitive or case-sensitive. Neither means
    /// the same smart case as `/`, where a capital in the pattern means it.
    pub insensitive: Option<bool>,
    /// `n`: count the matches and change nothing.
    pub count_only: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Substitute {
    pub lines: Lines,
    /// Empty means the last search pattern, which is what `:s//x/` is for.
    pub pattern: String,
    pub replacement: String,
    pub flags: Flags,
}

/// Read a `:` line as a substitute. `None` when it is not one at all - some
/// other command, for the usual dispatch to deal with - and `Err` when it is
/// one and is wrong, which is worth saying rather than passing on.
pub fn parse(line: &str) -> Option<Result<Substitute, String>> {
    let line = line.trim();
    let (lines, rest) = range(line);
    let rest = rest.strip_prefix('s')?;
    // `:s`, `:su`, `:sub`, `:substitute` - and nothing else that starts with s,
    // so `:set` is still `:set`.
    let rest = match rest.find(|c: char| !c.is_alphabetic()) {
        Some(at) => {
            if !"ubstitute".starts_with(&rest[..at]) {
                return None;
            }
            &rest[at..]
        }
        None => return "ubstitute".starts_with(rest).then_some(Err(needs_pattern())),
    };

    let mut chars = rest.chars();
    let Some(delimiter) = chars.next() else {
        return Some(Err(needs_pattern()));
    };
    // A delimiter is punctuation: `:s g` is not a substitute with a space for
    // a delimiter, it is a mistake worth naming.
    if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '\\' {
        return Some(Err(format!("not a delimiter: {delimiter}")));
    }

    let parts = split(chars.as_str(), delimiter);
    let [pattern, replacement, flags] = match parts.len() {
        // `:s/a` - vim allows the trailing delimiters to be left off.
        1 => [parts[0].clone(), String::new(), String::new()],
        2 => [parts[0].clone(), parts[1].clone(), String::new()],
        3 => [parts[0].clone(), parts[1].clone(), parts[2].clone()],
        _ => return Some(Err(format!("too many {delimiter}s"))),
    };

    let mut parsed = Flags::default();
    for flag in flags.trim().chars() {
        match flag {
            'g' => parsed.global = true,
            'i' => parsed.insensitive = Some(true),
            'I' => parsed.insensitive = Some(false),
            'n' => parsed.count_only = true,
            other => return Some(Err(format!("not a flag: {other}"))),
        }
    }

    Some(Ok(Substitute { lines, pattern, replacement, flags: parsed }))
}

/// `:g/pattern/command` - run one command on every line that matches, and
/// `:v` (or `:g!`) on every line that does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Global {
    /// No range given means the whole buffer, which is not the same default
    /// as `:s` has, so "none" has to survive the parse rather than becoming
    /// `Lines::Current` here.
    pub lines: Option<Lines>,
    pub invert: bool,
    /// Empty means the last search pattern, as it does for `:s`.
    pub pattern: String,
    /// The rest of the line, delimiters and all: `:g/x/s/a/b/g` is one
    /// substitute, not a pattern with two stray slashes in it.
    pub command: String,
}

/// Read a `:` line as a global. `None` when it is some other command, `Err`
/// when it is a global and is wrong.
pub fn parse_global(line: &str) -> Option<Result<Global, String>> {
    let line = line.trim();
    let (lines, rest) = range(line);
    let lines = (rest.len() != line.len()).then_some(lines);

    let end = rest.find(|c: char| !c.is_alphabetic()).unwrap_or(rest.len());
    let (name, rest) = rest.split_at(end);
    // `:g`, `:gl`, `:global`, and `:v`, `:vg`, `:vglobal` - but not `:vsplit`,
    // which is a longer word than the one that would have to be a prefix of.
    let mut invert = match name {
        "" => return None,
        _ if "global".starts_with(name) => false,
        _ if "vglobal".starts_with(name) => true,
        _ => return None,
    };
    let rest = match rest.strip_prefix('!') {
        // `:g!` is `:v`, and `:v!` is a double negative nobody means.
        Some(rest) if !invert => {
            invert = true;
            rest
        }
        Some(_) => return Some(Err("v! is g, which is not what you meant".into())),
        None => rest,
    };

    let mut chars = rest.chars();
    let Some(delimiter) = chars.next() else {
        return Some(Err(needs_command()));
    };
    if delimiter.is_alphanumeric() || delimiter.is_whitespace() || delimiter == '\\' {
        return Some(Err(format!("not a delimiter: {delimiter}")));
    }

    // Only the pattern is split off. Everything after the second delimiter is
    // a command line of its own and is handed on whole.
    let (pattern, command) = take_until(chars.as_str(), delimiter);
    let command = command.trim().to_string();
    if command.is_empty() {
        return Some(Err(needs_command()));
    }
    Some(Ok(Global { lines, invert, pattern, command }))
}

fn needs_command() -> String {
    "global needs a pattern and a command: :g/pattern/d".to_string()
}

/// Up to the first delimiter a backslash is not hiding, and the rest after it.
fn take_until(text: &str, delimiter: char) -> (String, &str) {
    let mut taken = String::new();
    let mut escaped = false;
    for (at, c) in text.char_indices() {
        if escaped {
            if c != delimiter {
                taken.push('\\');
            }
            taken.push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delimiter {
            return (taken, &text[at + c.len_utf8()..]);
        } else {
            taken.push(c);
        }
    }
    if escaped {
        taken.push('\\');
    }
    (taken, "")
}

fn needs_pattern() -> String {
    "substitute needs a pattern: :s/old/new/".to_string()
}

/// Split on a delimiter that a backslash can escape, keeping the escape in
/// place for the replacement to deal with: `\/` is a literal slash, and the
/// pattern still wants the backslash so the regex reads it as one too.
fn split(text: &str, delimiter: char) -> Vec<String> {
    let mut parts = vec![String::new()];
    let mut escaped = false;
    for c in text.chars() {
        if escaped {
            // An escaped delimiter is just the delimiter: the backslash was
            // there to hide it from this loop.
            if c != delimiter {
                parts.last_mut().expect("one part").push('\\');
            }
            parts.last_mut().expect("one part").push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == delimiter {
            parts.push(String::new());
        } else {
            parts.last_mut().expect("one part").push(c);
        }
    }
    if escaped {
        parts.last_mut().expect("one part").push('\\');
    }
    parts
}

/// The range in front of the command, and what is left after it. Public
/// because `:s` is no longer the only command that works over lines.
pub fn range(line: &str) -> (Lines, &str) {
    if let Some(rest) = line.strip_prefix('%') {
        return (Lines::Whole, rest);
    }
    let Some((first, rest)) = address(line) else {
        return (Lines::Current, line);
    };
    match rest.strip_prefix(',') {
        Some(rest) => match address(rest) {
            Some((second, rest)) => (Lines::Range(first, second), rest),
            // `:3,s/a/b/` - a comma with nothing after it means the current
            // line, which is what vim does with a missing address.
            None => (Lines::Range(first, Address::Current), rest),
        },
        None => (Lines::Range(first, first), rest),
    }
}

fn address(text: &str) -> Option<(Address, &str)> {
    let mut chars = text.char_indices();
    match chars.next()? {
        (_, '.') => Some((Address::Current, &text[1..])),
        (_, '$') => Some((Address::Last, &text[1..])),
        (_, '\'') => match chars.next() {
            Some((_, '<')) => Some((Address::VisualStart, &text[2..])),
            Some((_, '>')) => Some((Address::VisualEnd, &text[2..])),
            _ => None,
        },
        (_, c) if c.is_ascii_digit() => {
            let end = text.find(|c: char| !c.is_ascii_digit()).unwrap_or(text.len());
            let number = text[..end].parse().ok()?;
            Some((Address::Line(number), &text[end..]))
        }
        _ => None,
    }
}

/// Vim's replacement spellings in the regex crate's: `&` and `\0` are the whole
/// match, `\1` a group, and a literal `$` has to be doubled so it is not read
/// as a group itself.
pub fn replacement(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '$' => out.push_str("$$"),
            '&' => out.push_str("${0}"),
            '\\' => match chars.next() {
                Some(digit) if digit.is_ascii_digit() => {
                    out.push_str(&format!("${{{digit}}}"));
                }
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                // `\&`, `\\`, and anything else: the character itself, which
                // is the point of having escaped it.
                Some(other) => out.push(other),
                None => out.push('\\'),
            },
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(line: &str) -> Substitute {
        parse(line).expect("a substitute").expect("valid")
    }

    #[test]
    fn a_bare_substitute_is_this_line() {
        let s = ok("s/old/new/");
        assert_eq!(s.lines, Lines::Current);
        assert_eq!(s.pattern, "old");
        assert_eq!(s.replacement, "new");
        assert_eq!(s.flags, Flags::default());

        // The trailing delimiter is optional, and so is the replacement.
        assert_eq!(ok("s/old/new").replacement, "new");
        assert_eq!(ok("s/old").replacement, "");
        assert_eq!(ok("s/old/").replacement, "");
    }

    #[test]
    fn a_global_is_a_pattern_and_a_command() {
        let g = parse_global("g/x/d").expect("a global").expect("valid");
        assert_eq!(g.lines, None, "no range is the whole buffer, not this line");
        assert!(!g.invert);
        assert_eq!(g.pattern, "x");
        assert_eq!(g.command, "d");

        // Everything after the second delimiter is the command, whatever is
        // in it: `:g/x/s/a/b/g` is one substitute.
        assert_eq!(parse_global("g/x/s/a/b/g").unwrap().unwrap().command, "s/a/b/g");

        // `:v` and `:g!` are the same thing, and a range still comes first.
        let g = parse_global("1,5v#x#d").expect("a global").expect("valid");
        assert!(g.invert);
        assert_eq!(g.lines, Some(Lines::Range(Address::Line(1), Address::Line(5))));
        assert!(parse_global("g!/x/d").unwrap().unwrap().invert);
        assert_eq!(parse_global("%global/x/d").unwrap().unwrap().lines, Some(Lines::Whole));

        // An escaped delimiter belongs to the pattern.
        assert_eq!(parse_global(r"g/a\/b/d").unwrap().unwrap().pattern, "a/b");
    }

    #[test]
    fn what_is_not_a_global_is_left_for_someone_else() {
        // `:vsplit` starts with a v and is not a `:v`, and neither is `:gd`.
        assert!(parse_global("vsplit foo.rs").is_none());
        assert!(parse_global("gd").is_none());
        assert!(parse_global("set number").is_none());

        // A global with no command is a mistake worth naming.
        assert!(parse_global("g/x/").unwrap().is_err());
        assert!(parse_global("g").unwrap().is_err());
    }

    #[test]
    fn the_range_comes_first() {
        assert_eq!(ok("%s/a/b/").lines, Lines::Whole);
        assert_eq!(ok("3,7s/a/b/").lines, Lines::Range(Address::Line(3), Address::Line(7)));
        assert_eq!(ok(".,$s/a/b/").lines, Lines::Range(Address::Current, Address::Last));
        assert_eq!(ok("5s/a/b/").lines, Lines::Range(Address::Line(5), Address::Line(5)));
        assert_eq!(
            ok("'<,'>s/a/b/").lines,
            Lines::Range(Address::VisualStart, Address::VisualEnd)
        );
    }

    #[test]
    fn the_delimiter_is_whatever_follows_the_s() {
        let s = ok("%s#/usr/bin#/usr/local/bin#g");
        assert_eq!(s.pattern, "/usr/bin");
        assert_eq!(s.replacement, "/usr/local/bin");
        assert!(s.flags.global);

        // An escaped delimiter is a character in the pattern, not the end of
        // it - and the backslash goes, since it was only hiding the slash.
        let s = ok(r"s/a\/b/c/");
        assert_eq!(s.pattern, "a/b");

        // A backslash in front of anything else stays: the regex wants it.
        assert_eq!(ok(r"s/\d+/n/").pattern, r"\d+");
    }

    #[test]
    fn the_flags_are_read_and_the_wrong_ones_named() {
        let s = ok("%s/a/b/gi");
        assert_eq!(s.flags, Flags { global: true, insensitive: Some(true), count_only: false });
        assert_eq!(ok("%s/a/b/I").flags.insensitive, Some(false));
        assert!(ok("%s/a/b/n").flags.count_only);

        let error = parse("%s/a/b/q").expect("a substitute").expect_err("a complaint");
        assert!(error.contains('q'), "{error}");
    }

    #[test]
    fn what_is_not_a_substitute_is_left_alone() {
        // Every other command, including the ones that start with s.
        assert!(parse("set number").is_none());
        assert!(parse("w").is_none());
        assert!(parse("42").is_none());
        // And the long spellings are.
        assert_eq!(ok("substitute/a/b/").pattern, "a");
        assert_eq!(ok("sub/a/b/").pattern, "a");

        // `:s` on its own is a substitute that needs a pattern, not a mystery.
        assert!(parse("s").expect("a substitute").is_err());
        assert!(parse("%s").expect("a substitute").is_err());
    }

    #[test]
    fn a_replacement_speaks_vim_and_writes_regex() {
        assert_eq!(replacement("new"), "new");
        assert_eq!(replacement("[&]"), "[${0}]");
        assert_eq!(replacement(r"\1-\2"), "${1}-${2}");
        assert_eq!(replacement(r"\&"), "&");
        assert_eq!(replacement("$5"), "$$5");
        assert_eq!(replacement(r"a\tb"), "a\tb");
        assert_eq!(replacement(r"one\ntwo"), "one\ntwo");
    }
}

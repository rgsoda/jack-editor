use std::collections::HashMap;

/// Where text goes when no register is named.
pub const UNNAMED: char = '"';
/// The last yank, kept apart so a later delete does not clobber it.
pub const YANK: char = '0';
/// The system clipboard, as vim spells it. What `^c` writes and `^v` reads;
/// the editor syncs it with the session's clipboard either side of those.
pub const SYSTEM: char = '+';

/// What shape the text in a register is, which is what `p` has to know: the
/// same characters go back into the buffer three different ways.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kind {
    /// Characters, put inline where the cursor is.
    #[default]
    Char,
    /// Whole lines, put on a new line rather than inline - the difference
    /// between putting back a `dd` and a `dw`.
    Line,
    /// A rectangle, taken by `^b`: each line of it goes on a line of the
    /// buffer, at the cursor's column, however many lines that takes.
    Block,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegisterValue {
    pub text: String,
    pub kind: Kind,
}

impl RegisterValue {
    pub fn charwise(text: String) -> Self {
        RegisterValue { text, kind: Kind::Char }
    }

    /// Linewise text always ends with a newline, so putting it back cannot
    /// join two lines together.
    pub fn linewise(mut text: String) -> Self {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        RegisterValue { text, kind: Kind::Line }
    }

    /// A block, one row of the rectangle per line. Ragged rows are kept as
    /// they were taken; putting one pads them out again.
    pub fn block(text: String) -> Self {
        RegisterValue { text, kind: Kind::Block }
    }

    pub fn is_linewise(&self) -> bool {
        self.kind == Kind::Line
    }

    pub fn is_block(&self) -> bool {
        self.kind == Kind::Block
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Default)]
pub struct Registers {
    map: HashMap<char, RegisterValue>,
}

impl Registers {
    pub fn get(&self, name: Option<char>) -> RegisterValue {
        let name = name.map_or(UNNAMED, |c| c.to_ascii_lowercase());
        self.map.get(&name).cloned().unwrap_or_default()
    }

    /// Write one register and nothing else - the way the system clipboard is
    /// brought in, which is not a yank and should not disturb what is.
    pub fn set(&mut self, name: char, value: RegisterValue) {
        self.map.insert(name, value);
    }

    pub fn record_delete(&mut self, name: Option<char>, value: RegisterValue) {
        self.store(name, value);
    }

    /// A yank also lands in register `0`, so it is still there after the next
    /// delete has overwritten the unnamed register.
    pub fn record_yank(&mut self, name: Option<char>, value: RegisterValue) {
        let stored = self.store(name, value);
        if name.is_none() {
            self.map.insert(YANK, stored);
        }
    }

    /// Writes to the named register if there is one, and always to the unnamed
    /// register. An uppercase name appends instead of replacing.
    fn store(&mut self, name: Option<char>, value: RegisterValue) -> RegisterValue {
        let unnamed = match name {
            None => value,
            Some(name) if name.is_ascii_uppercase() => {
                let entry = self.map.entry(name.to_ascii_lowercase()).or_default();
                entry.text.push_str(&value.text);
                // Appending to a register keeps the wider shape: lines and
                // characters together are lines, which is what vim does.
                if value.kind == Kind::Line {
                    entry.kind = Kind::Line;
                }
                entry.clone()
            }
            Some(name) => {
                self.map.insert(name, value.clone());
                value
            }
        };
        self.map.insert(UNNAMED, unnamed.clone());
        unnamed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_delete_without_a_name_goes_to_the_unnamed_register() {
        let mut registers = Registers::default();
        registers.record_delete(None, RegisterValue::charwise("gone".into()));
        assert_eq!(registers.get(None).text, "gone");
    }

    #[test]
    fn a_named_delete_lands_in_both_registers() {
        let mut registers = Registers::default();
        registers.record_delete(Some('a'), RegisterValue::charwise("x".into()));
        assert_eq!(registers.get(Some('a')).text, "x");
        assert_eq!(registers.get(None).text, "x");
    }

    #[test]
    fn a_yank_survives_a_later_delete_in_register_zero() {
        let mut registers = Registers::default();
        registers.record_yank(None, RegisterValue::charwise("kept".into()));
        registers.record_delete(None, RegisterValue::charwise("junk".into()));

        assert_eq!(registers.get(None).text, "junk");
        assert_eq!(registers.get(Some(YANK)).text, "kept");
    }

    #[test]
    fn an_uppercase_name_appends() {
        let mut registers = Registers::default();
        registers.record_yank(Some('a'), RegisterValue::charwise("one ".into()));
        registers.record_yank(Some('A'), RegisterValue::charwise("two".into()));
        assert_eq!(registers.get(Some('a')).text, "one two");
        // The unnamed register sees the whole appended contents.
        assert_eq!(registers.get(None).text, "one two");
    }

    #[test]
    fn linewise_text_always_ends_with_a_newline() {
        let value = RegisterValue::linewise("no newline".into());
        assert_eq!(value.text, "no newline\n");
        assert!(value.is_linewise());

        // One already there is not doubled.
        assert_eq!(RegisterValue::linewise("has one\n".into()).text, "has one\n");
    }

    #[test]
    fn an_unset_register_reads_as_empty() {
        let registers = Registers::default();
        assert!(registers.get(Some('z')).is_empty());
        assert!(registers.get(None).is_empty());
    }
}

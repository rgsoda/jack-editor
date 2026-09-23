//! A window's keyboard, said the way the editor already listens.
//!
//! Everything from `keys.rs` down speaks `crossterm::event::KeyEvent`, and
//! that stays true here: this is the one place that knows a window sends
//! anything else. It is a translation and nothing more, so it can be tested
//! without a window to press keys in.
//!
//! A window is the better keyboard of the two, which shows up in what it can
//! say: a terminal cannot tell `^i` from `tab` or `^m` from `enter`, and
//! sends nothing at all for `ctrl-shift-p`. Those arrive here whole.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// What a key press means, as the editor would have heard it from a terminal.
/// `None` for the keys that are not text and not a command - a bare `shift`,
/// a dead key, anything the editor has no name for.
pub fn key_event(key: &Key, state: ModifiersState) -> Option<KeyEvent> {
    let mut modifiers = KeyModifiers::NONE;
    modifiers.set(KeyModifiers::CONTROL, state.control_key());
    modifiers.set(KeyModifiers::ALT, state.alt_key());
    modifiers.set(KeyModifiers::SUPER, state.super_key());

    let code = match key {
        Key::Named(named) => {
            // Shift belongs to the named keys: `shift-left` extends a
            // selection, where `A` is simply `A` and says nothing about how
            // it was typed.
            modifiers.set(KeyModifiers::SHIFT, state.shift_key());
            match named {
                NamedKey::Enter => KeyCode::Enter,
                NamedKey::Tab => KeyCode::Tab,
                NamedKey::Space => KeyCode::Char(' '),
                NamedKey::Backspace => KeyCode::Backspace,
                NamedKey::Escape => KeyCode::Esc,
                NamedKey::Delete => KeyCode::Delete,
                NamedKey::Insert => KeyCode::Insert,
                NamedKey::ArrowLeft => KeyCode::Left,
                NamedKey::ArrowRight => KeyCode::Right,
                NamedKey::ArrowUp => KeyCode::Up,
                NamedKey::ArrowDown => KeyCode::Down,
                NamedKey::Home => KeyCode::Home,
                NamedKey::End => KeyCode::End,
                NamedKey::PageUp => KeyCode::PageUp,
                NamedKey::PageDown => KeyCode::PageDown,
                NamedKey::F1 => KeyCode::F(1),
                NamedKey::F2 => KeyCode::F(2),
                NamedKey::F3 => KeyCode::F(3),
                NamedKey::F4 => KeyCode::F(4),
                NamedKey::F5 => KeyCode::F(5),
                NamedKey::F6 => KeyCode::F(6),
                NamedKey::F7 => KeyCode::F(7),
                NamedKey::F8 => KeyCode::F(8),
                NamedKey::F9 => KeyCode::F(9),
                NamedKey::F10 => KeyCode::F(10),
                NamedKey::F11 => KeyCode::F(11),
                NamedKey::F12 => KeyCode::F(12),
                _ => return None,
            }
        }
        Key::Character(text) => {
            let ch = text.chars().next()?;
            if text.chars().count() > 1 {
                return None;
            }
            // With control held, some keyboard layouts hand over the control
            // character itself - `^b` as byte 2. The editor wants the letter
            // and a modifier, which is what a terminal sends.
            let ch = match state.control_key() && (ch as u32) < 0x20 {
                true => char::from(ch as u8 + 0x60),
                false => ch,
            };
            KeyCode::Char(ch)
        }
        _ => return None,
    };
    Some(KeyEvent::new(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn character(text: &str, state: ModifiersState) -> Option<KeyEvent> {
        key_event(&Key::Character(text.into()), state)
    }

    #[test]
    fn typing_is_the_character_that_was_typed() {
        let key = character("x", ModifiersState::empty()).expect("a key");
        assert_eq!(key.code, KeyCode::Char('x'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);

        // Shift is in the character, not beside it: `A` is `A`, and saying
        // it was shifted as well would make `A` a different key from `A`.
        let key = character("A", ModifiersState::SHIFT).expect("a key");
        assert_eq!(key.code, KeyCode::Char('A'));
        assert_eq!(key.modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn control_arrives_as_the_letter_and_a_modifier() {
        let key = character("b", ModifiersState::CONTROL).expect("a key");
        assert_eq!(key.code, KeyCode::Char('b'));
        assert!(key.modifiers.contains(KeyModifiers::CONTROL));

        // And when the layout hands over the control character itself.
        let key = character("\u{2}", ModifiersState::CONTROL).expect("a key");
        assert_eq!(key.code, KeyCode::Char('b'));
    }

    #[test]
    fn the_named_keys_are_the_ones_the_editor_has_names_for() {
        let named = |key: NamedKey| key_event(&Key::Named(key), ModifiersState::empty()).map(|k| k.code);
        assert_eq!(named(NamedKey::Escape), Some(KeyCode::Esc));
        assert_eq!(named(NamedKey::Enter), Some(KeyCode::Enter));
        assert_eq!(named(NamedKey::Space), Some(KeyCode::Char(' ')));
        assert_eq!(named(NamedKey::ArrowDown), Some(KeyCode::Down));
        assert_eq!(named(NamedKey::F5), Some(KeyCode::F(5)));
        // A modifier on its own is not a key press.
        assert_eq!(named(NamedKey::Shift), None);
        assert_eq!(named(NamedKey::CapsLock), None);
    }

    #[test]
    fn shift_stays_on_the_named_keys_where_it_means_something() {
        let key = key_event(&Key::Named(NamedKey::ArrowRight), ModifiersState::SHIFT).expect("a key");
        assert_eq!(key.code, KeyCode::Right);
        assert!(key.modifiers.contains(KeyModifiers::SHIFT), "shift-right extends a selection");
    }

    #[test]
    fn a_window_can_say_what_a_terminal_cannot() {
        // `^i` and `tab` are one byte apart in a terminal and cannot be told
        // apart; here they are two keys.
        let tab = key_event(&Key::Named(NamedKey::Tab), ModifiersState::empty()).expect("a key");
        let ctrl_i = character("i", ModifiersState::CONTROL).expect("a key");
        assert_ne!(tab.code, ctrl_i.code);
    }
}

// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! What is typed and what the mouse does, translated into the bytes the server expects (xterm).

/// A key, already separated from the platform.
#[derive(Clone, Debug, PartialEq)]
pub enum Key {
    /// Typed text (a letter, a symbol, a composed character…).
    Text(String),
    Enter,
    Backspace,
    Tab,
    Escape,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    /// F1 to F12.
    F(u8),
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    /// The xterm modifier parameter (`1 + shift + 2·alt + 4·ctrl`), or `None` if there are none.
    fn param(self) -> Option<u8> {
        let n = self.shift as u8 + 2 * self.alt as u8 + 4 * self.ctrl as u8;
        (n > 0).then_some(n + 1)
    }
}

/// Bytes of a key. `app_cursor`: the program asked for arrow keys in application mode (vim, less…).
/// `None` if the key sends nothing.
pub fn encode_key(key: &Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    // `ESC [ 1 ; m X` with modifiers; without them, `ESC [ X` or, in application mode, `ESC O X`.
    let cursor = |c: char| {
        Some(match mods.param() {
            Some(m) => format!("\x1b[1;{m}{c}").into_bytes(),
            None if app_cursor => format!("\x1bO{c}").into_bytes(),
            None => format!("\x1b[{c}").into_bytes(),
        })
    };

    // `ESC [ n ~`, or `ESC [ n ; m ~` with modifiers.
    let tilde = |n: u8| {
        Some(match mods.param() {
            Some(m) => format!("\x1b[{n};{m}~").into_bytes(),
            None => format!("\x1b[{n}~").into_bytes(),
        })
    };

    let alt = |bytes: Vec<u8>| {
        if mods.alt {
            [b"\x1b".as_slice(), &bytes].concat()
        } else {
            bytes
        }
    };

    match key {
        Key::Text(text) if mods.ctrl => {
            let mut chars = text.chars();
            let c = chars.next()?;

            if chars.next().is_some() {
                return Some(alt(text.as_bytes().to_vec()));
            }

            let byte = match c.to_ascii_lowercase() {
                c @ 'a'..='z' => c as u8 - b'a' + 1,
                '@' | ' ' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '?' | '8' => 0x7f,
                _ => return Some(alt(text.as_bytes().to_vec())),
            };

            Some(alt(vec![byte]))
        }
        Key::Text(text) => Some(alt(text.as_bytes().to_vec())),
        Key::Enter => Some(alt(b"\r".to_vec())),
        Key::Backspace if mods.ctrl => Some(alt(vec![0x08])),
        Key::Backspace => Some(alt(vec![0x7f])),
        Key::Tab if mods.shift => Some(b"\x1b[Z".to_vec()),
        Key::Tab => Some(alt(b"\t".to_vec())),
        Key::Escape => Some(alt(b"\x1b".to_vec())),
        Key::Up => cursor('A'),
        Key::Down => cursor('B'),
        Key::Right => cursor('C'),
        Key::Left => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F(n @ 1..=4) => {
            let c = (b'P' + n - 1) as char;

            Some(match mods.param() {
                Some(m) => format!("\x1b[1;{m}{c}").into_bytes(),
                None => format!("\x1bO{c}").into_bytes(),
            })
        }
        Key::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][*n as usize - 5]),
        Key::F(_) => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
    WheelUp,
    WheelDown,
}

/// A mouse event for a program that asks for it (vim, htop…), in the SGR format
/// (`ESC [ < b ; x ; y M`) or, without it, in the classic X10 one. `line` and `column` start at 0.
/// `pressed`: press (or wheel); otherwise, release. `motion`: drag with a button pressed.
pub fn encode_mouse(
    button: MouseButton,
    pressed: bool,
    motion: bool,
    line: usize,
    column: usize,
    mods: Modifiers,
    sgr: bool,
) -> Option<Vec<u8>> {
    let mut code: u32 = match button {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
        MouseButton::WheelUp => 64,
        MouseButton::WheelDown => 65,
    };

    code += 4 * mods.shift as u32 + 8 * mods.alt as u32 + 16 * mods.ctrl as u32;

    if motion {
        code += 32;
    }

    let (x, y) = (column + 1, line + 1);

    if sgr {
        let end = if pressed { 'M' } else { 'm' };
        return Some(format!("\x1b[<{code};{x};{y}{end}").into_bytes());
    }

    // X10 does not know which button is released (always 3) and does not go past column 223.
    if !pressed {
        code = 3 + (code & !3);
    }

    if x > 223 || y > 223 {
        return None;
    }

    Some(vec![
        0x1b,
        b'[',
        b'M',
        32 + code as u8,
        32 + x as u8,
        32 + y as u8,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        shift: false,
        ctrl: false,
        alt: false,
    };

    fn key(k: Key, mods: Modifiers) -> Vec<u8> {
        encode_key(&k, mods, false).unwrap()
    }

    #[test]
    fn letters_and_control() {
        let ctrl = Modifiers { ctrl: true, ..NONE };
        let alt = Modifiers { alt: true, ..NONE };

        assert_eq!(key(Key::Text("a".into()), NONE), b"a");
        assert_eq!(key(Key::Text("é".into()), NONE), "é".as_bytes());
        assert_eq!(key(Key::Text("c".into()), ctrl), [3]);
        assert_eq!(key(Key::Text("C".into()), ctrl), [3]);
        assert_eq!(key(Key::Text("[".into()), ctrl), [0x1b]);
        assert_eq!(key(Key::Text("x".into()), alt), b"\x1bx");
        assert_eq!(key(Key::Enter, NONE), b"\r");
        assert_eq!(key(Key::Backspace, NONE), [0x7f]);
        assert_eq!(
            key(
                Key::Tab,
                Modifiers {
                    shift: true,
                    ..NONE
                }
            ),
            b"\x1b[Z"
        );
    }

    #[test]
    fn arrows_and_special_keys() {
        let ctrl = Modifiers { ctrl: true, ..NONE };

        assert_eq!(key(Key::Up, NONE), b"\x1b[A");
        assert_eq!(encode_key(&Key::Up, NONE, true).unwrap(), b"\x1bOA");
        assert_eq!(key(Key::Right, ctrl), b"\x1b[1;5C");
        assert_eq!(key(Key::Delete, NONE), b"\x1b[3~");
        assert_eq!(key(Key::PageUp, ctrl), b"\x1b[5;5~");
        assert_eq!(key(Key::F(1), NONE), b"\x1bOP");
        assert_eq!(key(Key::F(5), NONE), b"\x1b[15~");
        assert_eq!(key(Key::F(12), NONE), b"\x1b[24~");
        assert_eq!(encode_key(&Key::F(13), NONE, false), None);
    }

    #[test]
    fn mouse_sgr_and_x10() {
        assert_eq!(
            encode_mouse(MouseButton::Left, true, false, 0, 4, NONE, true).unwrap(),
            b"\x1b[<0;5;1M"
        );
        assert_eq!(
            encode_mouse(MouseButton::Left, false, false, 2, 0, NONE, true).unwrap(),
            b"\x1b[<0;1;3m"
        );
        assert_eq!(
            encode_mouse(MouseButton::WheelDown, true, false, 0, 0, NONE, true).unwrap(),
            b"\x1b[<65;1;1M"
        );
        assert_eq!(
            encode_mouse(MouseButton::Left, true, false, 0, 0, NONE, false).unwrap(),
            [0x1b, b'[', b'M', 32, 33, 33]
        );
        assert_eq!(
            encode_mouse(MouseButton::Left, true, false, 0, 300, NONE, false),
            None
        );
    }
}

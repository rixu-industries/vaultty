// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 José García "Ruxwez" (Rixu Industries)

//! Headless terminal emulator, built on `alacritty_terminal`: it receives the bytes from the server,
//! keeps the screen and the history, and translates what is typed into what the server expects. The
//! UI (desktop or mobile) only paints `Screen` and passes it keys, mouse and size.

mod input;
mod settings;

use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side as CellSide};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Config, Term, TermMode};
// `Handler` brings `clear_screen` to `Term`.
use alacritty_terminal::vte::ansi::{
    ClearMode, Color, CursorShape, Handler, NamedColor, Processor, Rgb,
};

pub use input::{Key, Modifiers, MouseButton, encode_key, encode_mouse};
pub use settings::Settings;

/// History lines kept per terminal.
const HISTORY: usize = 10_000;

/// Color RGB.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rgb8 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb8 {
    pub const fn hex(v: u32) -> Self {
        Self {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }
}

/// Terminal colors, from the theme: background, text, cursor, selection and the 16 ANSI colors (the 8
/// normal ones, then the bright ones). The 256-color cube and the grays are always xterm's.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Rgb8,
    pub foreground: Rgb8,
    pub cursor: Rgb8,
    pub selection: Rgb8,
    pub ansi: [Rgb8; 16],
}

/// A piece of a line with the same style.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// Column where it starts.
    pub column: usize,
    /// How many columns it spans (a wide character spans two).
    pub width: usize,
    pub text: String,
    pub fg: Rgb8,
    /// `None`: the terminal background.
    pub bg: Option<Rgb8>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

/// What needs painting now.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub columns: usize,
    pub lines: Vec<Vec<Run>>,
    /// Line and column of the cursor on the screen, if visible.
    pub cursor: Option<(usize, usize)>,
    /// The cursor is a bar or an underline (otherwise, a block).
    pub thin_cursor: bool,
}

/// What the emulator asks of its user while processing output.
#[derive(Default)]
struct Pending {
    /// Replies for the server (cursor position, terminal attributes…).
    replies: Vec<String>,
    /// Text the remote program wants to copy to the clipboard (OSC 52).
    clipboard: Option<String>,
}

#[derive(Clone)]
struct Listener(Arc<Mutex<Pending>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut pending = self.0.lock().unwrap();

        match event {
            Event::PtyWrite(text) => pending.replies.push(text),
            Event::ClipboardStore(_, text) => pending.clipboard = Some(text),
            _ => {}
        }
    }
}

struct Size {
    columns: usize,
    lines: usize,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        self.lines
    }

    fn screen_lines(&self) -> usize {
        self.lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// What `feed` returns: what to reply to the server and what to copy.
#[derive(Default, Debug, PartialEq)]
pub struct Output {
    pub reply: Vec<u8>,
    pub clipboard: Option<String>,
}

/// A terminal: screen, history, selection and modes.
pub struct Emulator {
    term: Term<Listener>,
    parser: Processor,
    pending: Arc<Mutex<Pending>>,
}

impl Emulator {
    pub fn new(columns: usize, lines: usize) -> Self {
        let pending = Arc::new(Mutex::new(Pending::default()));
        let config = Config {
            scrolling_history: HISTORY,
            ..Config::default()
        };
        let size = Size {
            columns: columns.max(2),
            lines: lines.max(1),
        };

        Self {
            term: Term::new(config, &size, Listener(pending.clone())),
            parser: Processor::new(),
            pending,
        }
    }

    pub fn columns(&self) -> usize {
        self.term.columns()
    }

    pub fn lines(&self) -> usize {
        self.term.screen_lines()
    }

    /// Processes what the server sends.
    pub fn feed(&mut self, bytes: &[u8]) -> Output {
        self.parser.advance(&mut self.term, bytes);

        let mut pending = self.pending.lock().unwrap();

        Output {
            reply: pending.replies.drain(..).collect::<String>().into_bytes(),
            clipboard: pending.clipboard.take(),
        }
    }

    /// Changes the size. Returns whether it changed (and the server must be notified).
    pub fn resize(&mut self, columns: usize, lines: usize) -> bool {
        let (columns, lines) = (columns.max(2), lines.max(1));

        if columns == self.columns() && lines == self.lines() {
            return false;
        }

        self.term.resize(Size { columns, lines });
        true
    }

    fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// The modes that change how keys and the mouse are encoded.
    pub fn app_cursor(&self) -> bool {
        self.mode().contains(TermMode::APP_CURSOR)
    }

    /// The remote program wants the mouse (vim, htop…).
    pub fn mouse_mode(&self) -> bool {
        self.mode().intersects(TermMode::MOUSE_MODE)
    }

    pub fn sgr_mouse(&self) -> bool {
        self.mode().contains(TermMode::SGR_MOUSE)
    }

    /// Alternate screen (vim, less…): the wheel sends arrow keys instead of scrolling the history.
    pub fn alt_screen(&self) -> bool {
        self.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Bytes for pasted text: wrapped in markers if the program asks for them, so it is not run line by line.
    pub fn paste(&self, text: &str) -> Vec<u8> {
        // Windows line breaks are sent as a single Enter.
        let text = text.replace("\r\n", "\r").replace('\n', "\r");

        if self.mode().contains(TermMode::BRACKETED_PASTE) {
            // Without the end marker inside, pasted text cannot break out of the paste.
            let text = text.replace("\x1b[201~", "");
            format!("\x1b[200~{text}\x1b[201~").into_bytes()
        } else {
            text.into_bytes()
        }
    }

    /// Scrolls the history by `lines` lines (positive: up).
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// How many lines above the last one are being viewed.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn point(&self, line: usize, column: usize) -> Point {
        let line = line.min(self.lines().saturating_sub(1)) as i32 - self.display_offset() as i32;
        let column = column.min(self.columns() - 1);

        Point::new(Line(line), Column(column))
    }

    /// Starts a selection at a screen cell. `words`: double click (by word).
    pub fn start_selection(&mut self, line: usize, column: usize, right_half: bool, words: bool) {
        let side = if right_half {
            CellSide::Right
        } else {
            CellSide::Left
        };
        let kind = if words {
            SelectionType::Semantic
        } else {
            SelectionType::Simple
        };

        self.term.selection = Some(Selection::new(kind, self.point(line, column), side));
    }

    pub fn update_selection(&mut self, line: usize, column: usize, right_half: bool) {
        let side = if right_half {
            CellSide::Right
        } else {
            CellSide::Left
        };
        let point = self.point(line, column);

        if let Some(selection) = &mut self.term.selection {
            selection.update(point, side);
        }
    }

    pub fn select_all(&mut self) {
        let top = Point::new(self.term.topmost_line(), Column(0));
        let bottom = Point::new(self.term.bottommost_line(), self.term.last_column());
        let mut selection = Selection::new(SelectionType::Simple, top, CellSide::Left);

        selection.update(bottom, CellSide::Right);
        self.term.selection = Some(selection);
    }

    pub fn clear_selection(&mut self) {
        self.term.selection = None;
    }

    /// The selected text, if any.
    pub fn selection_text(&self) -> Option<String> {
        self.term.selection_to_string().filter(|t| !t.is_empty())
    }

    pub fn has_selection(&self) -> bool {
        self.term.selection.as_ref().is_some_and(|s| !s.is_empty())
    }

    /// The user `sudo` asks for, if the cursor line is its password prompt
    /// (`[sudo] password for jose:`, `[sudo] contraseña para jose:`…). It is only text: any
    /// program can print it, so whoever uses it must ask for confirmation before filling it in.
    pub fn sudo_prompt(&self) -> Option<String> {
        if self.alt_screen() {
            return None;
        }

        let grid = self.term.grid();
        let row = &grid[grid.cursor.point.line];
        let text: String = (0..self.columns()).map(|c| row[Column(c)].c).collect();
        let text = text.trim();

        if !text.starts_with("[sudo]") {
            return None;
        }

        let user = text.strip_suffix(':')?.split_whitespace().last()?;
        Some(user.to_string())
    }

    /// Line and column of the cursor on the visible screen; `None` if the history has scrolled it out.
    pub fn cursor(&self) -> Option<(usize, usize)> {
        let point = self.term.grid().cursor.point;
        let line = usize::try_from(point.line.0 + self.display_offset() as i32).ok()?;

        (line < self.lines()).then_some((line, point.column.0))
    }

    /// Forgets the history and leaves the cursor line at the very top, like `clear` in xterm.js.
    pub fn clear(&mut self) {
        let line = self.term.grid().cursor.point.line.0;

        // Scrolls up until the cursor line is the first one (SU) and moves the cursor up with
        // it (CUU). It is local: nothing is sent to the server.
        if line > 0 && !self.alt_screen() {
            let up = format!("\x1b[{line}S\x1b[{line}A");
            self.parser.advance(&mut self.term, up.as_bytes());
        }

        self.term.clear_screen(ClearMode::Saved);
        self.term.selection = None;
    }

    /// The screen as it looks now, split into pieces of the same style.
    pub fn screen(&self, palette: &Palette) -> Screen {
        let content = self.term.renderable_content();
        let colors = content.colors;
        let offset = content.display_offset as i32;
        let columns = self.columns();
        let mut lines: Vec<Vec<Run>> = vec![Vec::new(); self.lines()];

        let rgb = |color: Color, bold: bool| -> Rgb8 {
            let index = match color {
                Color::Spec(Rgb { r, g, b }) => return Rgb8 { r, g, b },
                Color::Indexed(i) => i as usize,
                Color::Named(NamedColor::Foreground) => return palette.foreground,
                Color::Named(NamedColor::Background) => return palette.background,
                Color::Named(NamedColor::Cursor) => return palette.cursor,
                Color::Named(named) => named as usize,
            };

            // Bold for the first 8 colors uses their bright version, as in xterm.
            let index = if bold && index < 8 { index + 8 } else { index };

            match colors[index] {
                Some(Rgb { r, g, b }) => Rgb8 { r, g, b },
                None => indexed(index, palette),
            }
        };

        for item in content.display_iter {
            let cell = item.cell;
            let flags = cell.flags;

            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }

            let Ok(line) = usize::try_from(item.point.line.0 + offset) else {
                continue;
            };

            let Some(runs) = lines.get_mut(line) else {
                continue;
            };

            let bold = flags.contains(Flags::BOLD);
            let mut fg = rgb(cell.fg, bold);
            let mut bg = match cell.bg {
                Color::Named(NamedColor::Background) => None,
                color => Some(rgb(color, false)),
            };

            if flags.contains(Flags::INVERSE) {
                let back = bg.unwrap_or(palette.background);
                bg = Some(fg);
                fg = back;
            }

            if flags.contains(Flags::DIM) {
                fg = mix(fg, palette.background);
            }

            let selected = content.selection.is_some_and(|s| {
                s.contains_cell(&item, content.cursor.point, content.cursor.shape)
            });

            if selected {
                bg = Some(palette.selection);
            }

            let width = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let hidden = flags.contains(Flags::HIDDEN);
            let mut text = String::new();
            text.push(if hidden { ' ' } else { cell.c });

            if let Some(zero_width) = cell.zerowidth() {
                text.extend(zero_width);
            }

            let style = Run {
                column: item.point.column.0,
                width,
                text,
                fg,
                bg,
                bold,
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                strike: flags.contains(Flags::STRIKEOUT),
            };

            // Merged into the previous piece if it directly follows it with the same style.
            match runs.last_mut() {
                Some(last)
                    if last.column + last.width == style.column && same_style(last, &style) =>
                {
                    last.text.push_str(&style.text);
                    last.width += style.width;
                }
                _ => runs.push(style),
            }
        }

        let cursor = content.cursor;
        let visible = cursor.shape != CursorShape::Hidden && content.display_offset == 0;

        Screen {
            columns,
            lines,
            cursor: visible.then(|| {
                (
                    (cursor.point.line.0 + offset).max(0) as usize,
                    cursor.point.column.0,
                )
            }),
            thin_cursor: matches!(cursor.shape, CursorShape::Beam | CursorShape::Underline),
        }
    }
}

fn same_style(a: &Run, b: &Run) -> bool {
    a.fg == b.fg
        && a.bg == b.bg
        && a.bold == b.bold
        && a.italic == b.italic
        && a.underline == b.underline
        && a.strike == b.strike
}

/// Midpoint between two colors (dimmed text).
fn mix(a: Rgb8, b: Rgb8) -> Rgb8 {
    let m = |x: u8, y: u8| ((x as u16 + y as u16) / 2) as u8;

    Rgb8 {
        r: m(a.r, b.r),
        g: m(a.g, b.g),
        b: m(a.b, b.b),
    }
}

/// The 256 xterm colors: 16 ANSI, the 6×6×6 cube and 24 grays. From 256 on are
/// the named colors (text, background…), which come from the app's palette.
fn indexed(index: usize, palette: &Palette) -> Rgb8 {
    match index {
        0..16 => palette.ansi[index],
        16..232 => {
            let i = index - 16;
            let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };

            Rgb8 {
                r: level(i / 36),
                g: level(i / 6 % 6),
                b: level(i % 6),
            }
        }
        232..256 => {
            let v = (8 + (index - 232) * 10) as u8;
            Rgb8 { r: v, g: v, b: v }
        }
        _ => match index {
            i if i == NamedColor::Background as usize => palette.background,
            i if i == NamedColor::Cursor as usize => palette.cursor,
            _ => palette.foreground,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb8 = Rgb8::hex(0xcd3131);

    const PALETTE: Palette = Palette {
        background: Rgb8::hex(0x1b1b1b),
        foreground: Rgb8::hex(0xd4d4d4),
        cursor: Rgb8::hex(0xd4d4d4),
        selection: Rgb8::hex(0x444444),
        ansi: {
            let mut ansi = [Rgb8::hex(0); 16];
            ansi[1] = RED;
            ansi
        },
    };

    fn text(screen: &Screen, line: usize) -> String {
        screen.lines[line]
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>()
    }

    #[test]
    fn paints_text_and_colors() {
        let mut term = Emulator::new(20, 3);
        term.feed(b"hello \x1b[31mred\x1b[0m\r\nend");

        let screen = term.screen(&PALETTE);
        assert!(text(&screen, 0).starts_with("hello red"));
        assert!(text(&screen, 1).starts_with("end"));

        let red = screen.lines[0]
            .iter()
            .find(|r| r.text.contains("red"))
            .unwrap();
        assert_eq!(red.fg, RED);
        assert_eq!(screen.cursor, Some((1, 3)));
    }

    #[test]
    fn detects_sudo_prompt() {
        let mut term = Emulator::new(40, 3);
        term.feed(b"$ sudo ls\r\n[sudo] password for jose: ");
        assert_eq!(term.sudo_prompt().as_deref(), Some("jose"));

        // After pressing Enter the cursor moves down and is no longer the prompt.
        term.feed(b"\r\n");
        assert_eq!(term.sudo_prompt(), None);

        let mut term = Emulator::new(40, 3);
        term.feed("[sudo] contraseña para ana: ".as_bytes());
        assert_eq!(term.sudo_prompt().as_deref(), Some("ana"));

        let mut term = Emulator::new(40, 3);
        term.feed(b"Password: ");
        assert_eq!(term.sudo_prompt(), None);
    }

    #[test]
    fn answers_what_the_server_asks() {
        let mut term = Emulator::new(20, 3);
        // Cursor position (DSR 6): the terminal answers with `ESC [ row ; column R`.
        let out = term.feed(b"ab\x1b[6n");

        assert_eq!(out.reply, b"\x1b[1;3R");
    }

    #[test]
    fn selection_and_clear() {
        let mut term = Emulator::new(20, 3);
        term.feed(b"one two\r\n$ ");

        term.start_selection(0, 0, false, false);
        term.update_selection(0, 2, true);
        assert_eq!(term.selection_text().as_deref(), Some("one"));

        term.select_all();
        assert!(term.selection_text().unwrap().contains("two"));

        term.clear();
        assert!(!term.has_selection());

        let screen = term.screen(&PALETTE);
        assert_eq!(text(&screen, 0).trim(), "$");
        assert_eq!(screen.cursor, Some((0, 2)));
    }

    #[test]
    fn paste_respects_bracketed_mode() {
        let mut term = Emulator::new(20, 3);
        assert_eq!(term.paste("a\r\nb"), b"a\rb");

        term.feed(b"\x1b[?2004h");
        assert_eq!(term.paste("ls\x1b[201~"), b"\x1b[200~ls\x1b[201~");
    }

    #[test]
    fn history_and_size() {
        let mut term = Emulator::new(10, 2);

        for i in 0..5 {
            term.feed(format!("l{i}\r\n").as_bytes());
        }

        term.scroll(2);
        assert_eq!(term.display_offset(), 2);
        assert_eq!(term.screen(&PALETTE).cursor, None);

        term.scroll_to_bottom();
        assert!(term.resize(30, 5));
        assert!(!term.resize(30, 5));
        assert_eq!((term.columns(), term.lines()), (30, 5));
    }
}

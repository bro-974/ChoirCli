# ChoirCli POC Terminal Integration — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a Rust/iced GUI application that opens a native PTY, runs the system shell, and renders its ANSI-colored output in a canvas widget with full keyboard passthrough.

**Architecture:** A `std::thread` reads raw bytes from the PTY master and sends them via `std::sync::mpsc` to the iced event loop (polled every 8ms). The `vte::Parser` processes the bytes into a `TerminalScreen` grid of `Cell`s, which a `canvas::Program` draws using an embedded Nerd Font. Keyboard events captured in the canvas are converted to byte sequences and written directly to the PTY master.

**Tech Stack:** Rust 2021, `iced 0.13` (canvas + advanced features), `portable-pty 0.8`, `vte 0.13`, JetBrainsMono Nerd Font (embedded via `include_bytes!`).

---

## File Structure

```
src/
├── main.rs                        # iced entry point, font loading
├── terminal/
│   ├── mod.rs                     # pub use
│   ├── pty.rs                     # PtyHandle, spawn_pty()
│   └── screen.rs                  # Rgb, Cell, TerminalScreen, TerminalEmulator, vte::Perform
└── ui/
    ├── mod.rs                     # pub use
    ├── app.rs                     # App, Message, update(), view(), subscription()
    └── terminal_widget.rs         # TerminalWidget, impl canvas::Program
assets/
└── JetBrainsMonoNerdFont-Regular.ttf   # must be downloaded manually (step 1)
```

---

## Task 1: Project scaffolding

**Files:**
- Modify: `Cargo.toml`
- Create: `src/terminal/mod.rs`
- Create: `src/ui/mod.rs`
- Create: `src/main.rs` (stub)

- [ ] **Step 1: Download the Nerd Font asset**

```
URL: https://github.com/ryanoasis/nerd-fonts/releases/latest
→ Download JetBrainsMono.zip
→ Extract JetBrainsMonoNerdFont-Regular.ttf
→ Place at: assets/JetBrainsMonoNerdFont-Regular.ttf
```

- [ ] **Step 2: Update Cargo.toml**

```toml
[package]
name = "choircli"
version = "0.1.0"
edition = "2021"

[dependencies]
iced        = { version = "0.13", features = ["canvas", "advanced"] }
portable-pty = "0.8"
vte         = "0.13"
```

- [ ] **Step 3: Create module stubs**

`src/terminal/mod.rs`:
```rust
pub mod pty;
pub mod screen;

pub use pty::{PtyHandle, spawn_pty};
pub use screen::TerminalEmulator;
```

`src/ui/mod.rs`:
```rust
pub mod app;
pub mod terminal_widget;

pub use app::App;
```

`src/main.rs`:
```rust
mod terminal;
mod ui;

fn main() {
    println!("placeholder");
}
```

- [ ] **Step 4: Verify dependencies resolve**

```
cargo check
```

Expected: compiles (possibly with unused import warnings — ignore them).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/terminal/mod.rs src/ui/mod.rs src/main.rs assets/
git commit -m "chore: scaffold project with dependencies and asset"
```

---

## Task 2: Cell and grid data structures

**Files:**
- Create: `src/terminal/screen.rs`

- [ ] **Step 1: Write the failing tests**

Add to `src/terminal/screen.rs`:

```rust
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const WHITE: Rgb = Rgb { r: 204, g: 204, b: 204 };
    pub const BLACK: Rgb = Rgb { r: 0, g: 0, b: 0 };
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub underline: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { ch: ' ', fg: Rgb::WHITE, bg: Rgb::BLACK, bold: false, underline: false }
    }
}

pub struct TerminalScreen {
    pub grid: Vec<Vec<Cell>>,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub cols: usize,
    pub rows: usize,
    current_fg: Rgb,
    current_bg: Rgb,
    current_bold: bool,
    current_underline: bool,
}

impl TerminalScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        TerminalScreen {
            grid: vec![vec![Cell::default(); cols]; rows],
            cursor_row: 0,
            cursor_col: 0,
            cols,
            rows,
            current_fg: Rgb::WHITE,
            current_bg: Rgb::BLACK,
            current_bold: false,
            current_underline: false,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.cols = cols;
        self.rows = rows;
        self.grid.resize_with(rows, || vec![Cell::default(); cols]);
        for row in &mut self.grid {
            row.resize_with(cols, Cell::default);
        }
        self.cursor_row = self.cursor_row.min(rows.saturating_sub(1));
        self.cursor_col = self.cursor_col.min(cols.saturating_sub(1));
    }

    fn current_cell(&self) -> Cell {
        Cell {
            ch: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            bold: self.current_bold,
            underline: self.current_underline,
        }
    }
}

pub struct TerminalEmulator {
    parser: vte::Parser,
    pub screen: TerminalScreen,
}

impl TerminalEmulator {
    pub fn new(cols: usize, rows: usize) -> Self {
        TerminalEmulator {
            parser: vte::Parser::new(),
            screen: TerminalScreen::new(cols, rows),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.parser.advance(&mut self.screen, byte);
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.screen.resize(cols, rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_initializes_with_default_cells() {
        let screen = TerminalScreen::new(10, 5);
        assert_eq!(screen.grid.len(), 5);
        assert_eq!(screen.grid[0].len(), 10);
        assert_eq!(screen.grid[0][0].ch, ' ');
        assert_eq!(screen.grid[0][0].fg, Rgb::WHITE);
        assert_eq!(screen.grid[0][0].bg, Rgb::BLACK);
    }

    #[test]
    fn cursor_starts_at_origin() {
        let screen = TerminalScreen::new(10, 5);
        assert_eq!(screen.cursor_row, 0);
        assert_eq!(screen.cursor_col, 0);
    }

    #[test]
    fn resize_clamps_cursor_within_new_bounds() {
        let mut screen = TerminalScreen::new(10, 5);
        screen.cursor_col = 9;
        screen.cursor_row = 4;
        screen.resize(5, 3);
        assert!(screen.cursor_col < 5, "cursor_col {} must be < 5", screen.cursor_col);
        assert!(screen.cursor_row < 3, "cursor_row {} must be < 3", screen.cursor_row);
    }

    #[test]
    fn resize_preserves_existing_rows() {
        let mut screen = TerminalScreen::new(5, 3);
        screen.grid[0][0].ch = 'X';
        screen.resize(5, 5);
        assert_eq!(screen.grid[0][0].ch, 'X');
    }
}
```

- [ ] **Step 2: Run tests to verify they compile and pass**

```
cargo test terminal::screen::tests
```

Expected: 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/terminal/screen.rs
git commit -m "feat: add Cell, TerminalScreen, TerminalEmulator data structures"
```

---

## Task 3: vte::Perform — text output and line control

**Files:**
- Modify: `src/terminal/screen.rs`

- [ ] **Step 1: Write failing tests** (add to the `tests` module in `screen.rs`)

```rust
    fn make(cols: usize, rows: usize) -> TerminalEmulator {
        TerminalEmulator::new(cols, rows)
    }

    #[test]
    fn print_writes_char_at_cursor_and_advances() {
        let mut em = make(10, 5);
        em.process(b"A");
        assert_eq!(em.screen.grid[0][0].ch, 'A');
        assert_eq!(em.screen.cursor_col, 1);
        assert_eq!(em.screen.cursor_row, 0);
    }

    #[test]
    fn carriage_return_moves_cursor_to_col_zero() {
        let mut em = make(10, 5);
        em.process(b"ABC\r");
        assert_eq!(em.screen.cursor_col, 0);
        assert_eq!(em.screen.cursor_row, 0);
        assert_eq!(em.screen.grid[0][0].ch, 'A');
    }

    #[test]
    fn newline_moves_cursor_down() {
        let mut em = make(10, 5);
        em.process(b"A\nB");
        assert_eq!(em.screen.grid[0][0].ch, 'A');
        assert_eq!(em.screen.grid[1][0].ch, 'B');
    }

    #[test]
    fn scroll_when_cursor_exceeds_last_row() {
        let mut em = make(10, 3);
        // Fill all 3 rows then push past bottom
        em.process(b"A\nB\nC\nD");
        // Row 0 should now be 'B' (A scrolled off)
        assert_eq!(em.screen.grid[0][0].ch, 'B');
        assert_eq!(em.screen.grid[1][0].ch, 'C');
        assert_eq!(em.screen.grid[2][0].ch, 'D');
    }

    #[test]
    fn print_wraps_at_end_of_line() {
        let mut em = make(3, 5);
        em.process(b"ABCD");
        assert_eq!(em.screen.grid[0][0].ch, 'A');
        assert_eq!(em.screen.grid[0][1].ch, 'B');
        assert_eq!(em.screen.grid[0][2].ch, 'C');
        assert_eq!(em.screen.grid[1][0].ch, 'D');
    }
```

- [ ] **Step 2: Run tests — verify they fail**

```
cargo test terminal::screen::tests
```

Expected: the 5 new tests fail (trait not implemented yet).

- [ ] **Step 3: Implement `vte::Perform` on `TerminalScreen`**

Add after the `TerminalScreen` impl block:

```rust
impl vte::Perform for TerminalScreen {
    fn print(&mut self, c: char) {
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.newline();
        }
        let mut cell = self.current_cell();
        cell.ch = c;
        self.grid[self.cursor_row][self.cursor_col] = cell;
        self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | b'\x0B' | b'\x0C' => self.newline(),
            b'\r' => self.cursor_col = 0,
            b'\x08' => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            _ => {}
        }
    }

    // All other vte::Perform methods are handled in later tasks
    fn csi_dispatch(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}
```

Also add the `newline` helper inside `impl TerminalScreen`:

```rust
    fn newline(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.grid.remove(0);
            self.grid.push(vec![Cell::default(); self.cols]);
        } else {
            self.cursor_row += 1;
        }
    }
```

- [ ] **Step 4: Run tests — verify they all pass**

```
cargo test terminal::screen::tests
```

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/terminal/screen.rs
git commit -m "feat: implement vte::Perform print/execute for basic text output"
```

---

## Task 4: vte::Perform — SGR colors and cursor movement

**Files:**
- Modify: `src/terminal/screen.rs`

- [ ] **Step 1: Write failing tests** (add to `tests` module)

```rust
    #[test]
    fn sgr_8color_red_foreground() {
        let mut em = make(10, 5);
        em.process(b"\x1b[31mA");
        assert_eq!(em.screen.grid[0][0].fg, Rgb { r: 170, g: 0, b: 0 });
    }

    #[test]
    fn sgr_truecolor_foreground() {
        let mut em = make(10, 5);
        em.process(b"\x1b[38;2;255;128;0mA");
        assert_eq!(em.screen.grid[0][0].fg, Rgb { r: 255, g: 128, b: 0 });
    }

    #[test]
    fn sgr_truecolor_background() {
        let mut em = make(10, 5);
        em.process(b"\x1b[48;2;10;20;30m ");
        assert_eq!(em.screen.grid[0][0].bg, Rgb { r: 10, g: 20, b: 30 });
    }

    #[test]
    fn sgr_reset_restores_defaults() {
        let mut em = make(10, 5);
        em.process(b"\x1b[31mA\x1b[0mB");
        assert_eq!(em.screen.grid[0][1].fg, Rgb::WHITE);
        assert_eq!(em.screen.grid[0][1].bg, Rgb::BLACK);
    }

    #[test]
    fn sgr_bold() {
        let mut em = make(10, 5);
        em.process(b"\x1b[1mA");
        assert!(em.screen.grid[0][0].bold);
    }

    #[test]
    fn cursor_position_home() {
        let mut em = make(10, 5);
        em.process(b"ABC\x1b[H");
        assert_eq!(em.screen.cursor_row, 0);
        assert_eq!(em.screen.cursor_col, 0);
    }

    #[test]
    fn cursor_position_specific() {
        let mut em = make(10, 5);
        em.process(b"\x1b[3;5H");
        assert_eq!(em.screen.cursor_row, 2); // 1-based → 0-based
        assert_eq!(em.screen.cursor_col, 4);
    }

    #[test]
    fn cursor_up() {
        let mut em = make(10, 5);
        em.screen.cursor_row = 3;
        em.process(b"\x1b[2A");
        assert_eq!(em.screen.cursor_row, 1);
    }

    #[test]
    fn cursor_forward() {
        let mut em = make(10, 5);
        em.process(b"\x1b[4C");
        assert_eq!(em.screen.cursor_col, 4);
    }
```

- [ ] **Step 2: Run tests — verify they fail**

```
cargo test terminal::screen::tests
```

Expected: the 9 new tests fail.

- [ ] **Step 3: Add ANSI 8-color table** (add as a const array in `screen.rs`, outside any impl)

```rust
const ANSI_COLORS: [Rgb; 16] = [
    Rgb { r: 0,   g: 0,   b: 0   }, // 0  Black
    Rgb { r: 170, g: 0,   b: 0   }, // 1  Red
    Rgb { r: 0,   g: 170, b: 0   }, // 2  Green
    Rgb { r: 170, g: 85,  b: 0   }, // 3  Yellow
    Rgb { r: 0,   g: 0,   b: 170 }, // 4  Blue
    Rgb { r: 170, g: 0,   b: 170 }, // 5  Magenta
    Rgb { r: 0,   g: 170, b: 170 }, // 6  Cyan
    Rgb { r: 170, g: 170, b: 170 }, // 7  White
    Rgb { r: 85,  g: 85,  b: 85  }, // 8  Bright Black
    Rgb { r: 255, g: 85,  b: 85  }, // 9  Bright Red
    Rgb { r: 85,  g: 255, b: 85  }, // 10 Bright Green
    Rgb { r: 255, g: 255, b: 85  }, // 11 Bright Yellow
    Rgb { r: 85,  g: 85,  b: 255 }, // 12 Bright Blue
    Rgb { r: 255, g: 85,  b: 255 }, // 13 Bright Magenta
    Rgb { r: 85,  g: 255, b: 255 }, // 14 Bright Cyan
    Rgb { r: 255, g: 255, b: 255 }, // 15 Bright White
];
```

- [ ] **Step 4: Replace the stub `csi_dispatch` with the real implementation**

Replace the stub `fn csi_dispatch(...){}` inside `impl vte::Perform for TerminalScreen` with:

```rust
    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let ps: Vec<u16> = params.iter().map(|p| p[0]).collect();

        match action {
            // SGR — Select Graphic Rendition
            'm' => {
                let mut i = 0;
                while i < ps.len() {
                    match ps[i] {
                        0 => {
                            self.current_fg = Rgb::WHITE;
                            self.current_bg = Rgb::BLACK;
                            self.current_bold = false;
                            self.current_underline = false;
                        }
                        1 => self.current_bold = true,
                        4 => self.current_underline = true,
                        22 => self.current_bold = false,
                        24 => self.current_underline = false,
                        // 8-color foreground (30–37) and bright (90–97)
                        n @ 30..=37 => self.current_fg = ANSI_COLORS[(n - 30) as usize],
                        39 => self.current_fg = Rgb::WHITE,
                        n @ 40..=47 => self.current_bg = ANSI_COLORS[(n - 40) as usize],
                        49 => self.current_bg = Rgb::BLACK,
                        n @ 90..=97 => self.current_fg = ANSI_COLORS[(n - 90 + 8) as usize],
                        n @ 100..=107 => self.current_bg = ANSI_COLORS[(n - 100 + 8) as usize],
                        // 256-color foreground: 38;5;n
                        38 if ps.get(i + 1).copied() == Some(5) => {
                            if let Some(&n) = ps.get(i + 2) {
                                self.current_fg = ansi_256_to_rgb(n);
                                i += 2;
                            }
                        }
                        // TrueColor foreground: 38;2;r;g;b
                        38 if ps.get(i + 1).copied() == Some(2) => {
                            if let (Some(&r), Some(&g), Some(&b)) =
                                (ps.get(i + 2), ps.get(i + 3), ps.get(i + 4))
                            {
                                self.current_fg = Rgb { r: r as u8, g: g as u8, b: b as u8 };
                                i += 4;
                            }
                        }
                        // 256-color background: 48;5;n
                        48 if ps.get(i + 1).copied() == Some(5) => {
                            if let Some(&n) = ps.get(i + 2) {
                                self.current_bg = ansi_256_to_rgb(n);
                                i += 2;
                            }
                        }
                        // TrueColor background: 48;2;r;g;b
                        48 if ps.get(i + 1).copied() == Some(2) => {
                            if let (Some(&r), Some(&g), Some(&b)) =
                                (ps.get(i + 2), ps.get(i + 3), ps.get(i + 4))
                            {
                                self.current_bg = Rgb { r: r as u8, g: g as u8, b: b as u8 };
                                i += 4;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
            }
            // CUP — Cursor Position: ESC[row;colH (1-based)
            'H' | 'f' => {
                let row = ps.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = ps.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            // CUU — Cursor Up
            'A' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            // CUD — Cursor Down
            'B' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
            }
            // CUF — Cursor Forward
            'C' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
            }
            // CUB — Cursor Backward
            'D' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            // ED — Erase in Display
            'J' => {
                match ps.first().copied().unwrap_or(0) {
                    0 => { // erase from cursor to end
                        for col in self.cursor_col..self.cols {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                        for row in (self.cursor_row + 1)..self.rows {
                            self.grid[row] = vec![Cell::default(); self.cols];
                        }
                    }
                    2 | 3 => { // erase whole screen
                        self.grid = vec![vec![Cell::default(); self.cols]; self.rows];
                    }
                    _ => {}
                }
            }
            // EL — Erase in Line
            'K' => {
                match ps.first().copied().unwrap_or(0) {
                    0 => { // erase from cursor to end of line
                        for col in self.cursor_col..self.cols {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                    }
                    1 => { // erase from start to cursor
                        for col in 0..=self.cursor_col {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                    }
                    2 => { // erase whole line
                        self.grid[self.cursor_row] = vec![Cell::default(); self.cols];
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
```

- [ ] **Step 5: Add `ansi_256_to_rgb` helper** (add before the `tests` module)

```rust
fn ansi_256_to_rgb(n: u16) -> Rgb {
    match n {
        0..=15 => ANSI_COLORS[n as usize],
        16..=231 => {
            let n = n - 16;
            let b = n % 6;
            let g = (n / 6) % 6;
            let r = n / 36;
            let c = |v: u16| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb { r: c(r) as u8, g: c(g) as u8, b: c(b) as u8 }
        }
        232..=255 => {
            let v = (8 + (n - 232) * 10) as u8;
            Rgb { r: v, g: v, b: v }
        }
        _ => Rgb::WHITE,
    }
}
```

- [ ] **Step 6: Run all tests**

```
cargo test terminal::screen::tests
```

Expected: all tests pass (including the 9 new ones).

- [ ] **Step 7: Commit**

```bash
git add src/terminal/screen.rs
git commit -m "feat: implement SGR colors and cursor movement in vte::Perform"
```

---

## Task 5: PtyHandle — spawn, read thread, write, resize

**Files:**
- Create: `src/terminal/pty.rs`

- [ ] **Step 1: Write the integration test**

```rust
// src/terminal/pty.rs

use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty, ChildKiller};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, mpsc};

pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pub child: Box<dyn ChildKiller + Send>,
}

impl PtyHandle {
    pub fn write_bytes(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}

pub fn spawn_pty(cols: u16, rows: u16) -> (PtyHandle, mpsc::Receiver<Vec<u8>>) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .expect("failed to open PTY");

    let cmd = if cfg!(windows) {
        CommandBuilder::new("cmd.exe")
    } else {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        CommandBuilder::new(shell)
    };

    let child = pair.slave.spawn_command(cmd).expect("failed to spawn shell");
    let writer = pair.master.take_writer().expect("failed to get PTY writer");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut reader = pair.master.try_clone_reader().expect("failed to clone PTY reader");

    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });

    let handle = PtyHandle {
        master: pair.master,
        writer,
        child,
    };

    (handle, rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore] // requires system PTY — run with: cargo test -- --ignored
    fn pty_spawns_shell_and_echo_works() {
        let (mut pty, rx) = spawn_pty(80, 24);

        std::thread::sleep(Duration::from_millis(200)); // wait for shell prompt
        pty.write_bytes(b"echo __HELLO__\n").unwrap();

        let deadline = Instant::now() + Duration::from_secs(3);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            if let Ok(data) = rx.try_recv() {
                output.extend_from_slice(&data);
                if output.windows(9).any(|w| w == b"__HELLO__") {
                    return; // success
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("PTY did not echo: {:?}", String::from_utf8_lossy(&output));
    }
}
```

- [ ] **Step 2: Run the integration test**

```
cargo test -- --ignored
```

Expected: `pty_spawns_shell_and_echo_works` passes (PTY echoes `__HELLO__`).

- [ ] **Step 3: Commit**

```bash
git add src/terminal/pty.rs
git commit -m "feat: add PtyHandle with spawn, read thread, write, and resize"
```

---

## Task 6: iced Application skeleton

**Files:**
- Create: `src/ui/app.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Write `src/ui/app.rs`**

```rust
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use iced::{Element, Task, Subscription};

use crate::terminal::{PtyHandle, TerminalEmulator, spawn_pty};
use crate::ui::terminal_widget::TerminalWidget;

const COLS: usize = 80;
const ROWS: usize = 24;

pub struct App {
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let (pty, rx) = spawn_pty(COLS as u16, ROWS as u16);
        let app = App {
            emulator: TerminalEmulator::new(COLS, ROWS),
            pty,
            pty_rx: Arc::new(Mutex::new(rx)),
        };
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PtyTick => {
                while let Ok(bytes) = self.pty_rx.lock().unwrap().try_recv() {
                    self.emulator.process(&bytes);
                }
            }
            Message::KeyInput(bytes) => {
                let _ = self.pty.write_bytes(&bytes);
            }
            Message::WindowResized(width, height) => {
                let char_w = TerminalWidget::CHAR_WIDTH;
                let char_h = TerminalWidget::CHAR_HEIGHT;
                let cols = (width as f32 / char_w).floor() as usize;
                let rows = (height as f32 / char_h).floor() as usize;
                if cols > 0 && rows > 0 {
                    self.emulator.resize(cols, rows);
                    self.pty.resize(cols as u16, rows as u16);
                }
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<Message> {
        TerminalWidget::new(&self.emulator.screen).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        use iced::time;

        let poll = time::every(Duration::from_millis(8)).map(|_| Message::PtyTick);

        let resize = iced::event::listen_with(|event, _status, _id| {
            if let iced::Event::Window(iced::window::Event::Resized(size)) = event {
                Some(Message::WindowResized(size.width, size.height))
            } else {
                None
            }
        });

        Subscription::batch([poll, resize])
    }
}
```

- [ ] **Step 2: Write `src/main.rs`**

```rust
mod terminal;
mod ui;

use ui::app::App;

static NERD_FONT_BYTES: &[u8] =
    include_bytes!("../assets/JetBrainsMonoNerdFont-Regular.ttf");

fn main() -> iced::Result {
    iced::application("ChoirCli", App::update, App::view)
        .subscription(App::subscription)
        .font(NERD_FONT_BYTES)
        .run_with(App::new)
}
```

- [ ] **Step 3: Build to verify it compiles (terminal_widget stub needed first)**

Create a minimal stub for `src/ui/terminal_widget.rs` so the build succeeds:

```rust
use iced::{Element, widget::container, Length};
use crate::terminal::screen::TerminalScreen;
use crate::ui::app::Message;

pub struct TerminalWidget<'a> {
    screen: &'a TerminalScreen,
}

impl<'a> TerminalWidget<'a> {
    pub const CHAR_WIDTH: f32 = 9.6;
    pub const CHAR_HEIGHT: f32 = 16.0;

    pub fn new(screen: &'a TerminalScreen) -> Self {
        TerminalWidget { screen }
    }
}

impl<'a> From<TerminalWidget<'a>> for Element<'a, Message> {
    fn from(w: TerminalWidget<'a>) -> Self {
        container(iced::widget::text("terminal placeholder"))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::BLACK)),
                ..Default::default()
            })
            .into()
    }
}
```

```
cargo build
```

Expected: compiles without errors.

- [ ] **Step 4: Commit**

```bash
git add src/ui/app.rs src/ui/terminal_widget.rs src/main.rs
git commit -m "feat: add iced App skeleton with PTY polling subscription"
```

---

## Task 7: Canvas terminal widget — font rendering

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Replace the stub with the full canvas implementation**

```rust
use iced::{
    Element, Length, Rectangle, Size, Point,
    widget::canvas::{self, Canvas, Frame, Geometry, Program, Text},
    mouse,
};
use crate::terminal::screen::{TerminalScreen, Rgb};
use crate::ui::app::Message;

const FONT_SIZE: f32 = 14.0;
const NERD_FONT: iced::Font = iced::Font::with_name("JetBrainsMono Nerd Font");

pub struct TerminalWidget<'a> {
    screen: &'a TerminalScreen,
}

impl<'a> TerminalWidget<'a> {
    pub const CHAR_WIDTH: f32 = FONT_SIZE * 0.601;
    pub const CHAR_HEIGHT: f32 = FONT_SIZE * 1.2;

    pub fn new(screen: &'a TerminalScreen) -> Self {
        TerminalWidget { screen }
    }
}

impl<'a> Program<Message> for TerminalWidget<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let bytes = match event {
            canvas::Event::Keyboard(key_event) => map_key_event(key_event),
            _ => return (canvas::event::Status::Ignored, None),
        };

        match bytes {
            Some(b) => (canvas::event::Status::Captured, Some(Message::KeyInput(b))),
            None => (canvas::event::Status::Ignored, None),
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        // Black background
        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            iced::Color::BLACK,
        );

        let cw = Self::CHAR_WIDTH;
        let ch = Self::CHAR_HEIGHT;

        for (row_idx, row) in self.screen.grid.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let x = col_idx as f32 * cw;
                let y = row_idx as f32 * ch;

                // Draw background rectangle (only if not default black)
                if cell.bg != Rgb::BLACK {
                    frame.fill_rectangle(
                        Point::new(x, y),
                        Size::new(cw, ch),
                        rgb_to_iced(cell.bg),
                    );
                }

                // Draw character
                if cell.ch != ' ' {
                    frame.fill_text(Text {
                        content: cell.ch.to_string(),
                        position: Point::new(x, y),
                        color: rgb_to_iced(cell.fg),
                        size: iced::Pixels(FONT_SIZE),
                        font: NERD_FONT,
                        horizontal_alignment: iced::alignment::Horizontal::Left,
                        vertical_alignment: iced::alignment::Vertical::Top,
                        ..Text::default()
                    });
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

fn rgb_to_iced(rgb: Rgb) -> iced::Color {
    iced::Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

impl<'a> From<TerminalWidget<'a>> for Element<'a, Message> {
    fn from(w: TerminalWidget<'a>) -> Self {
        Canvas::new(w)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}
```

- [ ] **Step 2: Build**

```
cargo build
```

Expected: compiles. Fix any API discrepancies in `canvas::Text` field names if needed — check `iced::widget::canvas::Text` docs for the exact field names in your installed version.

- [ ] **Step 3: Run the app and verify it opens**

```
cargo run
```

Expected: a black window opens, shell starts, cursor may be visible as a character.

- [ ] **Step 4: Commit**

```bash
git add src/ui/terminal_widget.rs
git commit -m "feat: implement canvas grid rendering with Nerd Font"
```

---

## Task 8: Keyboard passthrough

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Add the `map_key_event` function**

Add this function to `src/ui/terminal_widget.rs` (before the `impl` blocks):

```rust
fn map_key_event(event: iced::keyboard::Event) -> Option<Vec<u8>> {
    use iced::keyboard::{Event, Key, key::Named};

    match event {
        Event::KeyPressed { key, modifiers, .. } => {
            // Ctrl+<letter> combinations
            if modifiers.control() {
                if let Key::Character(s) = &key {
                    let c = s.chars().next()?;
                    let byte = match c {
                        'c' | 'C' => 0x03u8,
                        'd' | 'D' => 0x04,
                        'z' | 'Z' => 0x1A,
                        'l' | 'L' => 0x0C,
                        'a' | 'A' => 0x01,
                        'e' | 'E' => 0x05,
                        'u' | 'U' => 0x15,
                        'k' | 'K' => 0x0B,
                        'w' | 'W' => 0x17,
                        _ => return None,
                    };
                    return Some(vec![byte]);
                }
            }

            match key {
                Key::Character(s) => Some(s.as_bytes().to_vec()),
                Key::Named(Named::Enter) => Some(b"\r".to_vec()),
                Key::Named(Named::Backspace) => Some(vec![0x7F]),
                Key::Named(Named::Tab) => Some(b"\t".to_vec()),
                Key::Named(Named::Escape) => Some(vec![0x1B]),
                Key::Named(Named::Space) => Some(b" ".to_vec()),
                Key::Named(Named::ArrowUp) => Some(b"\x1b[A".to_vec()),
                Key::Named(Named::ArrowDown) => Some(b"\x1b[B".to_vec()),
                Key::Named(Named::ArrowRight) => Some(b"\x1b[C".to_vec()),
                Key::Named(Named::ArrowLeft) => Some(b"\x1b[D".to_vec()),
                Key::Named(Named::Home) => Some(b"\x1b[H".to_vec()),
                Key::Named(Named::End) => Some(b"\x1b[F".to_vec()),
                Key::Named(Named::Delete) => Some(b"\x1b[3~".to_vec()),
                Key::Named(Named::PageUp) => Some(b"\x1b[5~".to_vec()),
                Key::Named(Named::PageDown) => Some(b"\x1b[6~".to_vec()),
                _ => None,
            }
        }
        _ => None,
    }
}
```

- [ ] **Step 2: Build and run**

```
cargo run
```

Expected: you can type in the shell. `ls` / `dir` shows output. `Ctrl+C` interrupts.

- [ ] **Step 3: Test TrueColor rendering**

On Unix, run inside the app:
```
printf '\x1b[38;2;255;100;0mORANGE\x1b[0m \x1b[48;2;0;100;255mBLUE BG\x1b[0m\n'
```

Expected: "ORANGE" appears in orange, "BLUE BG" has a blue background.

- [ ] **Step 4: Commit**

```bash
git add src/ui/terminal_widget.rs
git commit -m "feat: add keyboard passthrough with Ctrl combos and escape sequences"
```

---

## Task 9: Window resize → PTY resize

**Files:**
- Modify: `src/ui/app.rs` (already has the resize logic — verify it works end-to-end)

- [ ] **Step 1: Verify the resize subscription fires correctly**

Run the app and resize the window. Expected: the shell reflows content. If you run `bash` and type `tput cols`, it should return the updated column count.

- [ ] **Step 2: If resize doesn't work, debug the event listener**

In `src/ui/app.rs`, the `listen_with` closure may need adjustment depending on the exact iced 0.13 API. If `iced::window::Event::Resized(size)` doesn't match, try:

```rust
iced::event::listen_with(|event, _status, _id| {
    match event {
        iced::Event::Window(iced::window::Event::Resized { width, height }) => {
            Some(Message::WindowResized(width, height))
        }
        _ => None,
    }
})
```

- [ ] **Step 3: Run the full acceptance checklist**

```
cargo run
```

Acceptance criteria verification:
- [ ] No GUI freeze when shell outputs text quickly: `yes hello | head -1000`
- [ ] All keystrokes transmitted: type `echo $TERM` — should print terminal type
- [ ] Nerd Font embedded: the binary runs without the TTF file next to it
- [ ] No panics in PTY threads: run for 30s, resize window several times
- [ ] TrueColor renders correctly: run the `printf` color test from Task 8

- [ ] **Step 4: Final commit**

```bash
git add -p  # stage any fixes
git commit -m "feat: POC terminal emulator complete — iced canvas + PTY + ANSI TrueColor"
```

---

## Self-Review

**Spec coverage:**
- [x] PTY backend (`pty.rs`) — Task 5
- [x] Screen/grid emulator (`screen.rs`) — Tasks 2–4
- [x] Canvas widget (`terminal_widget.rs`) — Tasks 7–8
- [x] Keyboard passthrough — Task 8
- [x] Window resize → PTY resize — Task 9
- [x] TrueColor rendering — Task 4 (SGR 38;2) + Task 8 (canvas draw)
- [x] Nerd Font embedded — Task 1 (asset) + Task 6 (main.rs font loading)
- [x] No panic in PTY threads — reader thread uses `Ok/Err` matching, no unwrap on read

**Type consistency check:**
- `PtyHandle::write_bytes` used consistently in `app.rs` ✓
- `TerminalEmulator::process` called in `update()` ✓
- `TerminalWidget::CHAR_WIDTH/HEIGHT` used in both `app.rs` resize and `terminal_widget.rs` draw ✓
- `Message::KeyInput(Vec<u8>)` produced in widget, consumed in `update()` ✓

**No placeholders found.**

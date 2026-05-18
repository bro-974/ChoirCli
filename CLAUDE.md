# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build
cargo build --release

# Run the app (opens the terminal GUI)
cargo run

# Unit tests (18 tests in terminal::screen)
cargo test

# PTY integration test (spawns a real shell, takes ~2s)
cargo test -- --ignored

# Single test
cargo test terminal::screen::tests::sgr_truecolor_foreground
```

## Architecture

This is a **terminal emulator POC** built with iced. Three layers, each with a clear boundary:

### 1. `src/terminal/screen.rs` — ANSI state machine
- `TerminalEmulator` owns a `vte::Parser` and a `TerminalScreen`. Call `emulator.process(&[u8])` to feed raw PTY bytes; `vte` calls back into `impl vte::Perform for TerminalScreen`.
- `TerminalScreen` is the grid state: `grid: Vec<Vec<Cell>>`, cursor position, and current SGR style (`current_fg/bg/bold/underline`). The `Perform` impl writes to the grid and mutates cursor/style.
- **`\n` resets `cursor_col` to 0** (implicit CR+LF, matching POSIX `onlcr` behavior) — this is intentional and differs from a strict LF-only interpretation.
- No scrollback buffer: scrolling removes `grid[0]` and pushes a blank row at the end.

### 2. `src/terminal/pty.rs` — PTY bridge
- `spawn_pty(cols, rows)` opens a native PTY (`portable-pty`), spawns `cmd.exe` on Windows or `$SHELL` on Unix, and starts a `std::thread` that reads raw bytes into a `mpsc::Sender<Vec<u8>>`.
- Returns `(PtyHandle, mpsc::Receiver<Vec<u8>>)`. The `PtyHandle` holds the write end and the `child` (kept alive to prevent premature shell exit — do not drop it early).

### 3. `src/ui/` — iced application
- `app.rs` / `App`: The iced application struct. `subscription()` polls the PTY receiver every 8ms via `iced::time::every` and listens for window resize via `iced::event::listen_with`. On `Message::PtyTick`, drains the channel with `try_recv()` and feeds bytes to `emulator.process()`.
- `terminal_widget.rs` / `TerminalWidget<'a>`: `impl canvas::Program<Message>` — draws the grid cell-by-cell using `JetBrainsMonoNerdFont-Regular.ttf` (embedded via `include_bytes!` in `main.rs`). Keyboard events are captured in `canvas::Program::update()` and converted to byte sequences (`Ctrl+C` → `\x03`, arrows → `\x1b[A` etc.) sent as `Message::KeyInput(Vec<u8>)`.

### Data flow
```
PTY reader thread → mpsc → App::update (PtyTick) → TerminalEmulator::process → vte::Parser → TerminalScreen grid
Keyboard event → canvas::Program::update → Message::KeyInput → PtyHandle::write_bytes → PTY master
Window resize → Message::WindowResized → TerminalEmulator::resize + PtyHandle::resize
```

## Key design decisions

- **iced feature `"tokio"`** is required for `iced::time::every` — without it the subscription won't compile.
- `Rgb::WHITE` is `(204,204,204)` not `(255,255,255)` — it's ANSI color index 7.
- The canvas re-renders at ~125 Hz driven by the 8ms timer tick; there is no dirty-flag optimization.
- `spawn_pty` uses `expect()` for PTY init — a PTY failure is unrecoverable, so early panic is correct.
- The PTY integration test is `#[ignore]` because it requires a real shell and ~2s to run.

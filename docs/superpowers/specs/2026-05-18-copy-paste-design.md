# Copy/Paste Design — ChoirCli Terminal

**Date:** 2026-05-18  
**Status:** Approved

## Goal

Add mouse text selection and clipboard copy/paste to the terminal emulator:

- Drag mouse to select text (visual highlight)
- Right-click to copy selection to clipboard
- `Ctrl+V` to paste from clipboard
- `Ctrl+C` stays unchanged (SIGINT → `\x03`)

## Approach

iced clipboard API (`iced::clipboard::write/read`) — no extra dependencies.

## Architecture

### 1. SelectionState (terminal_widget.rs)

`canvas::Program::type State` changes from `()` to `SelectionState`:

```rust
#[derive(Default)]
pub struct SelectionState {
    start: Option<(usize, usize)>,  // (row, col) on ButtonPressed Left
    end:   Option<(usize, usize)>,  // (row, col) updated by CursorMoved
    is_selecting: bool,
}
```

Pixel → grid cell conversion:
```
row = (y / CHAR_HEIGHT) as usize, clamped to [0, rows-1]
col = (x / CHAR_WIDTH)  as usize, clamped to [0, cols-1]
```

Position comes from `cursor.position_in(bounds)`.

### 2. Mouse & Keyboard Events

| Event | Action |
|-------|--------|
| `Mouse::ButtonPressed(Left)` | set `start = end = Some(cell)`, `is_selecting = true` |
| `Mouse::CursorMoved` | if `is_selecting` → update `end = Some(cell)` |
| `Mouse::ButtonReleased(Left)` | `is_selecting = false` |
| `Mouse::ButtonPressed(Right)` | extract text from selection → `Message::CopyToClipboard(text)`, reset selection |
| `Keyboard Ctrl+V` | → `Message::PasteFromClipboard` |
| `Keyboard Ctrl+C` | unchanged → `\x03` SIGINT |

`map_key_event` changes signature from `→ Option<Vec<u8>>` to `→ Option<Message>` to allow returning `Message::PasteFromClipboard` directly.

### 3. Selection Rendering

In `draw()`, before drawing characters, render highlight rectangles for selected cells using color `rgba(100, 150, 255, 0.4)`.

Multi-line selection convention:
- First row: `start_col` → end of row
- Middle rows: full row
- Last row: start of row → `end_col`

### 4. Text Extraction

At right-click, in `canvas::Program::update` (has access to `&self.screen` and `&state`):

```
for each row from start_row to end_row:
  take chars from col_start to col_end
  trim trailing spaces
join rows with "\n"
```

### 5. New Messages (app.rs)

```rust
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),   // right-click with selection
    PasteFromClipboard,        // Ctrl+V
    PasteText(String),         // clipboard content received
}
```

`App::update` handlers:
- `CopyToClipboard(text)` → `iced::clipboard::write(text)`
- `PasteFromClipboard` → `iced::clipboard::read().map(|opt| Message::PasteText(opt.unwrap_or_default()))`
- `PasteText(text)` → `self.pty.write_bytes(text.as_bytes())`

## Data Flow

```
Mouse drag         → SelectionState.{start, end, is_selecting}
Mouse right-click  → extract text from screen grid → Message::CopyToClipboard(text)
CopyToClipboard    → iced::clipboard::write(text) → Task<Message>
Ctrl+V             → Message::PasteFromClipboard
PasteFromClipboard → iced::clipboard::read() → Message::PasteText(text)
PasteText          → PtyHandle::write_bytes(text.as_bytes())
```

## Files Changed

- `src/ui/terminal_widget.rs` — SelectionState, mouse event handling, highlight rendering, text extraction, map_key_event signature change
- `src/ui/app.rs` — 3 new Message variants, 3 new update arms

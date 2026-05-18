# Copy/Paste Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add mouse text selection (drag to highlight), right-click to copy, and Ctrl+V to paste from clipboard — while keeping Ctrl+C as SIGINT.

**Architecture:** Extend `canvas::Program::State` from `()` to `SelectionState` to track mouse drag. Pure functions `extract_text` and `map_key_input` handle the logic; iced clipboard Tasks connect copy/paste to the system clipboard.

**Tech Stack:** iced 0.13 (`iced::clipboard::read/write` Tasks), Rust

---

## File Map

| File | Changes |
|------|---------|
| `src/ui/terminal_widget.rs` | Add `SelectionState`, `extract_text`, refactor `map_key_event` → `map_key_input`, handle mouse events, draw highlight |
| `src/ui/app.rs` | Add 3 new `Message` variants, 3 new `update` arms |

---

### Task 1: SelectionState struct + normalization

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Add `SelectionState` after the existing imports/constants, before `TerminalWidget`**

In `src/ui/terminal_widget.rs`, insert after line 9 (`const NERD_FONT`):

```rust
#[derive(Default, Clone, Copy)]
pub struct SelectionState {
    pub start: Option<(usize, usize)>,
    pub end: Option<(usize, usize)>,
    pub is_selecting: bool,
}

impl SelectionState {
    /// Returns (top_left, bottom_right) in grid coordinates, or None if no selection.
    pub fn normalized(&self) -> Option<((usize, usize), (usize, usize))> {
        match (self.start, self.end) {
            (Some(s), Some(e)) => {
                if s <= e { Some((s, e)) } else { Some((e, s)) }
            }
            _ => None,
        }
    }
}
```

- [ ] **Step 2: Write failing tests for `normalized()`**

At the bottom of `src/ui/terminal_widget.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_forward_selection() {
        let s = SelectionState { start: Some((1, 3)), end: Some((2, 5)), is_selecting: false };
        assert_eq!(s.normalized(), Some(((1, 3), (2, 5))));
    }

    #[test]
    fn normalized_backward_selection() {
        let s = SelectionState { start: Some((2, 5)), end: Some((1, 3)), is_selecting: false };
        assert_eq!(s.normalized(), Some(((1, 3), (2, 5))));
    }

    #[test]
    fn normalized_same_cell() {
        let s = SelectionState { start: Some((0, 0)), end: Some((0, 0)), is_selecting: false };
        assert_eq!(s.normalized(), Some(((0, 0), (0, 0))));
    }

    #[test]
    fn normalized_no_selection() {
        let s = SelectionState::default();
        assert_eq!(s.normalized(), None);
    }
}
```

- [ ] **Step 3: Run tests (expect PASS — struct is already added)**

```
cargo test terminal_widget::tests
```

Expected: 4 tests pass.

- [ ] **Step 4: Commit**

```
git add src/ui/terminal_widget.rs
git commit -m "feat: add SelectionState with normalized()"
```

---

### Task 2: extract_text pure function + tests

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Write failing tests first**

In the `#[cfg(test)] mod tests` block (inside `terminal_widget.rs`), add:

```rust
    fn make_row(s: &str) -> Vec<crate::terminal::screen::Cell> {
        use crate::terminal::screen::{Cell, Rgb};
        s.chars().map(|ch| Cell { ch, fg: Rgb::WHITE, bg: Rgb::BLACK, bold: false, underline: false }).collect()
    }

    #[test]
    fn extract_single_line() {
        let grid = vec![make_row("Hello World")];
        assert_eq!(extract_text(&grid, (0, 0), (0, 4)), "Hello");
    }

    #[test]
    fn extract_trims_trailing_spaces() {
        let grid = vec![make_row("Hi   ")];
        assert_eq!(extract_text(&grid, (0, 0), (0, 4)), "Hi");
    }

    #[test]
    fn extract_multi_line() {
        let grid = vec![make_row("Hello"), make_row("World")];
        assert_eq!(extract_text(&grid, (0, 0), (1, 4)), "Hello\nWorld");
    }

    #[test]
    fn extract_multi_line_partial() {
        let grid = vec![make_row("Hello"), make_row("World")];
        // Start at col 2 of row 0, end at col 2 of row 1
        assert_eq!(extract_text(&grid, (0, 2), (1, 2)), "llo\nWor");
    }
```

- [ ] **Step 2: Run tests (expect FAIL — extract_text not defined yet)**

```
cargo test terminal_widget::tests::extract
```

Expected: compile error "cannot find function `extract_text`".

- [ ] **Step 3: Implement `extract_text`**

Add this function in `src/ui/terminal_widget.rs`, before the `#[cfg(test)]` block:

```rust
fn extract_text(grid: &[Vec<crate::terminal::screen::Cell>], start: (usize, usize), end: (usize, usize)) -> String {
    let (r1, c1) = start;
    let (r2, c2) = end;
    let mut lines = Vec::new();

    for row in r1..=r2 {
        if row >= grid.len() { break; }
        let row_len = grid[row].len();
        if row_len == 0 {
            lines.push(String::new());
            continue;
        }
        let col_start = if row == r1 { c1 } else { 0 }.min(row_len - 1);
        let col_end = if row == r2 { c2 } else { row_len - 1 }.min(row_len - 1);
        let line: String = grid[row][col_start..=col_end]
            .iter()
            .map(|c| c.ch)
            .collect::<String>()
            .trim_end()
            .to_string();
        lines.push(line);
    }
    lines.join("\n")
}
```

- [ ] **Step 4: Run tests (expect PASS)**

```
cargo test terminal_widget::tests::extract
```

Expected: 4 tests pass.

- [ ] **Step 5: Commit**

```
git add src/ui/terminal_widget.rs
git commit -m "feat: add extract_text for clipboard copy"
```

---

### Task 3: New Message variants + app.rs update arms

**Files:**
- Modify: `src/ui/app.rs`

- [ ] **Step 1: Add 3 new variants to `Message` enum**

In `src/ui/app.rs`, change the `Message` enum from:

```rust
#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
}
```

to:

```rust
#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),
    PasteFromClipboard,
    PasteText(String),
}
```

- [ ] **Step 2: Add the 3 new arms in `App::update`**

In `src/ui/app.rs`, inside `pub fn update`, after the `Message::WindowResized` arm, add:

```rust
            Message::CopyToClipboard(text) => {
                return iced::clipboard::write(text).map(|_| Message::PtyTick);
            }
            Message::PasteFromClipboard => {
                return iced::clipboard::read()
                    .map(|opt| Message::PasteText(opt.unwrap_or_default()));
            }
            Message::PasteText(text) => {
                let _ = self.pty.write_bytes(text.as_bytes());
            }
```

- [ ] **Step 3: Add `iced` import for clipboard at top of app.rs (it's already in scope via `use iced::...`)**

No change needed — `iced::clipboard` is accessed via the full path.

- [ ] **Step 4: Compile check**

```
cargo build
```

Expected: success. If `iced::clipboard::write/read` API differs, the error will name the correct types — fix accordingly (e.g., different argument order or return type).

- [ ] **Step 5: Commit**

```
git add src/ui/app.rs
git commit -m "feat: add CopyToClipboard/PasteFromClipboard/PasteText messages"
```

---

### Task 4: Refactor map_key_event → map_key_input + Ctrl+V

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Write failing tests for Ctrl+V and verify Ctrl+C unchanged**

In the `#[cfg(test)] mod tests` block, add:

```rust
    #[test]
    fn ctrl_v_produces_paste_message() {
        use iced::keyboard::{Key, Modifiers};
        let result = map_key_input(&Key::Character("v".into()), Modifiers::CTRL);
        assert!(matches!(result, Some(Message::PasteFromClipboard)));
    }

    #[test]
    fn ctrl_c_produces_sigint() {
        use iced::keyboard::{Key, Modifiers};
        let result = map_key_input(&Key::Character("c".into()), Modifiers::CTRL);
        assert!(matches!(result, Some(Message::KeyInput(ref b)) if *b == vec![0x03u8]));
    }

    #[test]
    fn ctrl_d_produces_eof() {
        use iced::keyboard::{Key, Modifiers};
        let result = map_key_input(&Key::Character("d".into()), Modifiers::CTRL);
        assert!(matches!(result, Some(Message::KeyInput(ref b)) if *b == vec![0x04u8]));
    }
```

- [ ] **Step 2: Run (expect compile error — `map_key_input` not defined)**

```
cargo test terminal_widget::tests::ctrl
```

Expected: compile error "cannot find function `map_key_input`".

- [ ] **Step 3: Replace `map_key_event` with `map_key_input` + thin wrapper**

Replace the entire `fn map_key_event` function in `src/ui/terminal_widget.rs` with:

```rust
fn map_key_input(key: &iced::keyboard::Key, modifiers: iced::keyboard::Modifiers) -> Option<Message> {
    use iced::keyboard::{Key, key::Named};

    if modifiers.control() {
        if let Key::Character(s) = key {
            let c = s.chars().next()?;
            let msg = match c {
                'c' | 'C' => Message::KeyInput(vec![0x03u8]),
                'd' | 'D' => Message::KeyInput(vec![0x04]),
                'z' | 'Z' => Message::KeyInput(vec![0x1A]),
                'l' | 'L' => Message::KeyInput(vec![0x0C]),
                'a' | 'A' => Message::KeyInput(vec![0x01]),
                'e' | 'E' => Message::KeyInput(vec![0x05]),
                'u' | 'U' => Message::KeyInput(vec![0x15]),
                'k' | 'K' => Message::KeyInput(vec![0x0B]),
                'w' | 'W' => Message::KeyInput(vec![0x17]),
                'v' | 'V' => Message::PasteFromClipboard,
                _ => return None,
            };
            return Some(msg);
        }
    }

    match key {
        Key::Character(s) => Some(Message::KeyInput(s.as_bytes().to_vec())),
        Key::Named(Named::Enter) => Some(Message::KeyInput(b"\r".to_vec())),
        Key::Named(Named::Backspace) => Some(Message::KeyInput(vec![0x7F])),
        Key::Named(Named::Tab) => Some(Message::KeyInput(b"\t".to_vec())),
        Key::Named(Named::Escape) => Some(Message::KeyInput(vec![0x1B])),
        Key::Named(Named::Space) => Some(Message::KeyInput(b" ".to_vec())),
        Key::Named(Named::ArrowUp) => Some(Message::KeyInput(b"\x1b[A".to_vec())),
        Key::Named(Named::ArrowDown) => Some(Message::KeyInput(b"\x1b[B".to_vec())),
        Key::Named(Named::ArrowRight) => Some(Message::KeyInput(b"\x1b[C".to_vec())),
        Key::Named(Named::ArrowLeft) => Some(Message::KeyInput(b"\x1b[D".to_vec())),
        Key::Named(Named::Home) => Some(Message::KeyInput(b"\x1b[H".to_vec())),
        Key::Named(Named::End) => Some(Message::KeyInput(b"\x1b[F".to_vec())),
        Key::Named(Named::Delete) => Some(Message::KeyInput(b"\x1b[3~".to_vec())),
        Key::Named(Named::PageUp) => Some(Message::KeyInput(b"\x1b[5~".to_vec())),
        Key::Named(Named::PageDown) => Some(Message::KeyInput(b"\x1b[6~".to_vec())),
        _ => None,
    }
}

fn map_key_event(event: iced::keyboard::Event) -> Option<Message> {
    use iced::keyboard::Event;
    match event {
        Event::KeyPressed { key, modifiers, .. } => map_key_input(&key, modifiers),
        _ => None,
    }
}
```

- [ ] **Step 4: Update `canvas::Program::update` to use `Option<Message>` return from `map_key_event`**

The `update` method already assigns the result of `map_key_event` and returns it — no change needed there since `map_key_event` still returns `Option<Message>`. Confirm the `update` body still compiles (the `bytes` variable rename is gone; we now work directly with `Option<Message>`):

```rust
    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let msg = match event {
            canvas::Event::Keyboard(key_event) => map_key_event(key_event),
            _ => return (canvas::event::Status::Ignored, None),
        };

        match msg {
            Some(m) => (canvas::event::Status::Captured, Some(m)),
            None => (canvas::event::Status::Ignored, None),
        }
    }
```

Replace the existing `update` body with the above.

- [ ] **Step 5: Run tests (expect PASS)**

```
cargo test terminal_widget::tests
```

Expected: all tests pass (including the 3 new Ctrl tests + the 4 normalized + 4 extract tests = 11 total in this module).

- [ ] **Step 6: Commit**

```
git add src/ui/terminal_widget.rs
git commit -m "feat: refactor map_key_event, add Ctrl+V → PasteFromClipboard"
```

---

### Task 5: Mouse event handling — SelectionState wired to canvas

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Add `pixel_to_cell` helper**

Add before `fn extract_text`:

```rust
fn pixel_to_cell(pos: iced::Point, rows: usize, cols: usize) -> (usize, usize) {
    let row = (pos.y / TerminalWidget::CHAR_HEIGHT) as usize;
    let col = (pos.x / TerminalWidget::CHAR_WIDTH) as usize;
    (row.min(rows.saturating_sub(1)), col.min(cols.saturating_sub(1)))
}
```

- [ ] **Step 2: Change `type State = ()` to `type State = SelectionState`**

In the `impl canvas::Program<Message> for TerminalWidget<'a>` block, change:

```rust
    type State = ();
```

to:

```rust
    type State = SelectionState;
```

- [ ] **Step 3: Replace `update` with the full mouse + keyboard version**

Replace the entire `fn update` method with:

```rust
    fn update(
        &self,
        state: &mut Self::State,
        event: canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        match event {
            canvas::Event::Keyboard(key_event) => {
                match map_key_event(key_event) {
                    Some(m) => (canvas::event::Status::Captured, Some(m)),
                    None => (canvas::event::Status::Ignored, None),
                }
            }
            canvas::Event::Mouse(mouse_event) => {
                use iced::mouse::Event as ME;
                match mouse_event {
                    ME::ButtonPressed(mouse::Button::Left) => {
                        if let Some(pos) = cursor.position_in(bounds) {
                            let cell = pixel_to_cell(pos, self.screen.rows, self.screen.cols);
                            state.start = Some(cell);
                            state.end = Some(cell);
                            state.is_selecting = true;
                        }
                        (canvas::event::Status::Captured, None)
                    }
                    ME::CursorMoved { .. } => {
                        if state.is_selecting {
                            if let Some(pos) = cursor.position_in(bounds) {
                                state.end = Some(pixel_to_cell(pos, self.screen.rows, self.screen.cols));
                            }
                        }
                        (canvas::event::Status::Ignored, None)
                    }
                    ME::ButtonReleased(mouse::Button::Left) => {
                        state.is_selecting = false;
                        (canvas::event::Status::Captured, None)
                    }
                    ME::ButtonPressed(mouse::Button::Right) => {
                        if let Some((start, end)) = state.normalized() {
                            let text = extract_text(&self.screen.grid, start, end);
                            *state = SelectionState::default();
                            (canvas::event::Status::Captured, Some(Message::CopyToClipboard(text)))
                        } else {
                            (canvas::event::Status::Ignored, None)
                        }
                    }
                    _ => (canvas::event::Status::Ignored, None),
                }
            }
            _ => (canvas::event::Status::Ignored, None),
        }
    }
```

- [ ] **Step 4: Compile check**

```
cargo build
```

Expected: success. If `cursor.position_in` doesn't exist in iced 0.13, use `cursor.position().map(|p| Point::new(p.x - bounds.x, p.y - bounds.y))` instead.

- [ ] **Step 5: Run all tests**

```
cargo test
```

Expected: all 18 existing tests + 11 new tests pass.

- [ ] **Step 6: Commit**

```
git add src/ui/terminal_widget.rs
git commit -m "feat: wire mouse events to SelectionState"
```

---

### Task 6: Selection highlight rendering

**Files:**
- Modify: `src/ui/terminal_widget.rs`

- [ ] **Step 1: Update `draw` signature to use `state: &SelectionState`**

The `draw` signature currently has `_state: &Self::State`. Remove the underscore:

```rust
    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
```

- [ ] **Step 2: Add highlight drawing before the character loop**

Inside `fn draw`, after `frame.fill_rectangle(Point::ORIGIN, bounds.size(), iced::Color::BLACK);` and before the `for (row_idx, row) in self.screen.grid.iter().enumerate()` loop, add:

```rust
        // Selection highlight
        if let Some(((r1, c1), (r2, c2))) = state.normalized() {
            let highlight = iced::Color { r: 0.39, g: 0.59, b: 1.0, a: 0.4 };
            for row in r1..=r2 {
                if row >= self.screen.rows { break; }
                let col_start = if row == r1 { c1 } else { 0 };
                let col_end = if row == r2 { c2 } else { self.screen.cols.saturating_sub(1) };
                let x = col_start as f32 * cw;
                let y = row as f32 * ch;
                let w = (col_end - col_start + 1) as f32 * cw;
                frame.fill_rectangle(Point::new(x, y), Size::new(w, ch), highlight);
            }
        }
```

- [ ] **Step 3: Compile check + run all tests**

```
cargo build && cargo test
```

Expected: build succeeds, all tests pass.

- [ ] **Step 4: Manual test — smoke test the full feature**

```
cargo run
```

Verify:
1. Click and drag → blue highlight appears over selected text
2. Right-click on selection → highlight disappears (text in clipboard)
3. Paste in a text editor (outside the app) → copied text appears correctly
4. In the terminal app, type something, then `Ctrl+V` → clipboard content pasted into shell
5. Run `ping 127.0.0.1` then `Ctrl+C` → ping stops (SIGINT still works)
6. Multi-line selection: drag across 2+ rows → first row highlighted from selection start, last row to selection end, middle rows fully highlighted

- [ ] **Step 5: Commit**

```
git add src/ui/terminal_widget.rs
git commit -m "feat: render selection highlight for copy/paste"
```

---

## Self-Review

**Spec coverage:**
- ✅ Mouse drag to select → Tasks 1, 2, 5
- ✅ Right-click to copy → Task 5 (mouse event handler)
- ✅ Ctrl+V to paste → Tasks 3, 4
- ✅ Ctrl+C stays SIGINT → Task 4 (Ctrl+C arm unchanged)
- ✅ Visual highlight → Task 6
- ✅ iced clipboard API → Task 3

**Placeholder scan:** No TBD, no TODO, no "add appropriate error handling", all code blocks present.

**Type consistency:**
- `SelectionState` defined in Task 1, used in Tasks 5, 6 ✅
- `extract_text(&[Vec<Cell>], (usize,usize), (usize,usize)) -> String` defined in Task 2, called in Task 5 ✅
- `map_key_input` defined in Task 4, called via `map_key_event` ✅
- `Message::CopyToClipboard(String)` added in Task 3, produced in Task 5, handled in Task 3 ✅
- `Message::PasteFromClipboard` added Task 3, produced Task 4, handled Task 3 ✅
- `Message::PasteText(String)` added Task 3, produced Task 3, handled Task 3 ✅

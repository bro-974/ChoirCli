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

    pub fn current_cell(&self) -> Cell {
        Cell {
            ch: ' ',
            fg: self.current_fg,
            bg: self.current_bg,
            bold: self.current_bold,
            underline: self.current_underline,
        }
    }

    fn newline(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.grid.remove(0);
            self.grid.push(vec![Cell::default(); self.cols]);
        } else {
            self.cursor_row += 1;
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
            b'\n' | b'\x0B' | b'\x0C' => {
                self.newline();
                self.cursor_col = 0;
            }
            b'\r' => self.cursor_col = 0,
            b'\x08' => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(cols: usize, rows: usize) -> TerminalEmulator {
        TerminalEmulator::new(cols, rows)
    }

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
        em.process(b"A\nB\nC\nD");
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
}

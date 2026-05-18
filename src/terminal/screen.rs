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

    fn csi_dispatch(&mut self, params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        let ps: Vec<u16> = params.iter().map(|p| p[0]).collect();

        match action {
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
                        n @ 30..=37 => self.current_fg = ANSI_COLORS[(n - 30) as usize],
                        39 => self.current_fg = Rgb::WHITE,
                        n @ 40..=47 => self.current_bg = ANSI_COLORS[(n - 40) as usize],
                        49 => self.current_bg = Rgb::BLACK,
                        n @ 90..=97 => self.current_fg = ANSI_COLORS[(n - 90 + 8) as usize],
                        n @ 100..=107 => self.current_bg = ANSI_COLORS[(n - 100 + 8) as usize],
                        38 if ps.get(i + 1).copied() == Some(5) => {
                            if let Some(&n) = ps.get(i + 2) {
                                self.current_fg = ansi_256_to_rgb(n);
                                i += 2;
                            }
                        }
                        38 if ps.get(i + 1).copied() == Some(2) => {
                            if let (Some(&r), Some(&g), Some(&b)) =
                                (ps.get(i + 2), ps.get(i + 3), ps.get(i + 4))
                            {
                                self.current_fg = Rgb { r: r as u8, g: g as u8, b: b as u8 };
                                i += 4;
                            }
                        }
                        48 if ps.get(i + 1).copied() == Some(5) => {
                            if let Some(&n) = ps.get(i + 2) {
                                self.current_bg = ansi_256_to_rgb(n);
                                i += 2;
                            }
                        }
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
            'H' | 'f' => {
                let row = ps.first().copied().unwrap_or(1).saturating_sub(1) as usize;
                let col = ps.get(1).copied().unwrap_or(1).saturating_sub(1) as usize;
                self.cursor_row = row.min(self.rows.saturating_sub(1));
                self.cursor_col = col.min(self.cols.saturating_sub(1));
            }
            'A' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            'B' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_row = (self.cursor_row + n).min(self.rows.saturating_sub(1));
            }
            'C' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = (self.cursor_col + n).min(self.cols.saturating_sub(1));
            }
            'D' => {
                let n = ps.first().copied().unwrap_or(1).max(1) as usize;
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            'J' => {
                match ps.first().copied().unwrap_or(0) {
                    0 => {
                        for col in self.cursor_col..self.cols {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                        for row in (self.cursor_row + 1)..self.rows {
                            self.grid[row] = vec![Cell::default(); self.cols];
                        }
                    }
                    2 | 3 => {
                        self.grid = vec![vec![Cell::default(); self.cols]; self.rows];
                    }
                    _ => {}
                }
            }
            'K' => {
                match ps.first().copied().unwrap_or(0) {
                    0 => {
                        for col in self.cursor_col..self.cols {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                    }
                    1 => {
                        for col in 0..=self.cursor_col {
                            self.grid[self.cursor_row][col] = Cell::default();
                        }
                    }
                    2 => {
                        self.grid[self.cursor_row] = vec![Cell::default(); self.cols];
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _byte: u8) {}
}

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

fn ansi_256_to_rgb(n: u16) -> Rgb {
    match n {
        0..=15 => ANSI_COLORS[n as usize],
        16..=231 => {
            let n = n - 16;
            let b = n % 6;
            let g = (n / 6) % 6;
            let r = n / 36;
            let c = |v: u16| if v == 0 { 0u8 } else { (55 + v * 40) as u8 };
            Rgb { r: c(r), g: c(g), b: c(b) }
        }
        232..=255 => {
            let v = (8 + (n - 232) * 10) as u8;
            Rgb { r: v, g: v, b: v }
        }
        _ => Rgb::WHITE,
    }
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
        assert_eq!(em.screen.cursor_row, 2);
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
}

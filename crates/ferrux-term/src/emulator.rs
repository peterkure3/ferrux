/// Wraps `vt100::Parser`: feed it raw PTY output bytes, read back a
/// rendered `Screen` (grid of cells with colors/attributes) at any time.
pub struct Emulator {
    parser: vt100::Parser,
}

impl Emulator {
    pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self {
        Self {
            parser: vt100::Parser::new(rows, cols, scrollback_len),
        }
    }

    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn screen(&self) -> &vt100::Screen {
        self.parser.screen()
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.screen_mut().set_size(rows, cols);
    }

    pub fn scroll(&mut self, lines: isize) {
        let offset = self.screen().scrollback().saturating_add_signed(lines);
        self.parser.screen_mut().set_scrollback(offset);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_plain_text_into_screen_cells() {
        let mut emulator = Emulator::new(24, 80, 0);
        emulator.process(b"hi");

        let screen = emulator.screen();
        assert_eq!(screen.cell(0, 0).unwrap().contents(), "h");
        assert_eq!(screen.cell(0, 1).unwrap().contents(), "i");
    }

    #[test]
    fn honors_sgr_bold_attribute() {
        let mut emulator = Emulator::new(24, 80, 0);
        emulator.process(b"\x1b[1mbold\x1b[0m");

        let screen = emulator.screen();
        assert!(screen.cell(0, 0).unwrap().bold());
        assert!(!screen.cell(0, 4).unwrap().bold());
    }

    #[test]
    fn bounded_scrollback_returns_to_live_screen_before_input() {
        let mut emulator = Emulator::new(3, 20, 10);
        for _ in 0..50 {
            emulator.process(b"line\r\n");
        }
        emulator.scroll(100);
        assert!(emulator.screen().scrollback() <= 10);
        assert!(emulator.screen().scrollback() > 0);
        emulator.scroll(isize::MIN);
        assert_eq!(emulator.screen().scrollback(), 0);
    }
}

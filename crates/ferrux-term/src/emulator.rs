/// Wraps `vt100::Parser`: feed it raw PTY output bytes, read back a
/// rendered `Screen` (grid of cells with colors/attributes) at any time.
pub struct Emulator {
    parser: vt100::Parser<TerminalModes>,
}

struct TerminalModes {
    alternate_scroll: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        // ConPTY can consume mode 1007 instead of forwarding it to the host.
        // Match terminal-host alternate scrolling unless the child disables it.
        Self {
            alternate_scroll: true,
        }
    }
}

impl vt100::Callbacks for TerminalModes {
    fn unhandled_csi(
        &mut self,
        _: &mut vt100::Screen,
        first: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        command: char,
    ) {
        if first == Some(b'?') && params.contains(&&[1007][..]) && matches!(command, 'h' | 'l') {
            self.alternate_scroll = command == 'h';
        }
    }
}

impl Emulator {
    pub fn new(rows: u16, cols: u16, scrollback_len: usize) -> Self {
        Self {
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                scrollback_len,
                TerminalModes::default(),
            ),
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

    /// Routes wheel input to child mouse reporting or alternate-screen keys.
    /// Coordinates are zero-based cells inside the child viewport, never chrome.
    pub fn wheel(&mut self, up: bool, column: u16, row: u16, local: bool) -> Option<Vec<u8>> {
        let screen = self.screen();
        if !local && screen.scrollback() == 0 {
            if screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None {
                let button = if up { 64u32 } else { 65 };
                let x = u32::from(column) + 1;
                let y = u32::from(row) + 1;
                return match screen.mouse_protocol_encoding() {
                    vt100::MouseProtocolEncoding::Sgr => {
                        Some(format!("\x1b[<{button};{x};{y}M").into_bytes())
                    }
                    vt100::MouseProtocolEncoding::Default if x <= 223 && y <= 223 => Some(vec![
                        27,
                        b'[',
                        b'M',
                        (button + 32) as u8,
                        (x + 32) as u8,
                        (y + 32) as u8,
                    ]),
                    vt100::MouseProtocolEncoding::Utf8 if x <= 2015 && y <= 2015 => {
                        let mut bytes = b"\x1b[M".to_vec();
                        for value in [button + 32, x + 32, y + 32] {
                            let mut buffer = [0; 4];
                            bytes.extend_from_slice(
                                char::from_u32(value)
                                    .unwrap()
                                    .encode_utf8(&mut buffer)
                                    .as_bytes(),
                            );
                        }
                        Some(bytes)
                    }
                    _ => None, // Legacy encodings cannot represent larger coordinates.
                };
            }
            if screen.alternate_screen() && self.parser.callbacks().alternate_scroll {
                let prefix = if screen.application_cursor() {
                    "\x1bO"
                } else {
                    "\x1b["
                };
                return Some(
                    format!("{prefix}{}", if up { 'A' } else { 'B' })
                        .repeat(3)
                        .into_bytes(),
                );
            }
        }
        self.scroll(if up { 3 } else { -3 });
        None
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

    #[test]
    fn wheel_history_stays_anchored_during_new_output() {
        let mut emulator = Emulator::new(3, 20, 100);
        for index in 0..20 {
            emulator.process(format!("line{index:02}\r\n").as_bytes());
        }
        assert!(emulator.wheel(true, 0, 0, false).is_none());
        let old = emulator.screen().rows(0, 20).collect::<Vec<_>>();
        emulator.process(b"new output\r\n");
        assert_eq!(emulator.screen().rows(0, 20).collect::<Vec<_>>(), old);
        emulator.scroll(isize::MIN);
        assert!(emulator.screen().contents().contains("new output"));
    }

    #[test]
    fn application_wheel_uses_child_coordinates_and_does_not_replay_mouse_clicks() {
        let mut emulator = Emulator::new(10, 400, 100);
        emulator.process(b"\x1b[?1049h\x1b[?1000h\x1b[?1006h");
        assert_eq!(
            emulator.wheel(true, 300, 4, false).unwrap(),
            b"\x1b[<64;301;5M"
        );
        assert_eq!(
            emulator.wheel(false, 1, 2, false).unwrap(),
            b"\x1b[<65;2;3M"
        );
        assert!(emulator.wheel(true, 1, 2, true).is_none());
        emulator.process(b"\x1b[?1006l");
        assert_eq!(
            emulator.wheel(true, 1, 2, false).unwrap(),
            vec![27, b'[', b'M', 96, 34, 35]
        );
        assert!(emulator.wheel(true, 300, 2, false).is_none());
        emulator.process(b"\x1b[?1005h");
        assert_eq!(
            emulator.wheel(true, 300, 2, false).unwrap(),
            "\x1b[M`ō#".as_bytes()
        );
    }

    #[test]
    fn alternate_scroll_mode_survives_chunked_sequences_and_can_be_disabled() {
        let mut emulator = Emulator::new(10, 20, 100);
        emulator.process(b"\x1b[?1049h");
        assert!(emulator.wheel(true, 0, 0, false).is_some());
        emulator.process(b"\x1b[?1007l");
        assert!(emulator.wheel(true, 0, 0, false).is_none());
        for byte in b"\x1b[?1007h" {
            emulator.process(&[*byte]);
        }
        assert_eq!(
            emulator.wheel(true, 0, 0, false).unwrap(),
            b"\x1b[A\x1b[A\x1b[A"
        );
        emulator.process(b"\x1b[?1h");
        assert_eq!(
            emulator.wheel(false, 0, 0, false).unwrap(),
            b"\x1bOB\x1bOB\x1bOB"
        );
        emulator.process(b"\x1b[?1007l");
        assert!(emulator.wheel(true, 0, 0, false).is_none());
        emulator.process(b"\x1b[?1049l");
        assert!(emulator.wheel(true, 0, 0, false).is_none());
    }
}

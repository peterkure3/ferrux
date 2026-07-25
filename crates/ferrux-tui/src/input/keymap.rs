use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

/// What a key press should do.
pub enum Action {
    /// Raw bytes to write to the focused pane's PTY input.
    Bytes(Vec<u8>),
    /// Detach from the workspace and return to the shell (panes keep
    /// running on the daemon).
    Detach,
    /// `Ctrl+B` was pressed: the next key is a workspace command rather
    /// than pane input.
    EnterPrefix,
    SplitVertical,
    SplitHorizontal,
    FocusNext,
    ClosePane,
    SaveWorkspace,
}

/// Translates a crossterm key event into an `Action`.
///
/// `awaiting_prefix` selects which key language is active: right after
/// `Ctrl+B` (`EnterPrefix`), the next key is a workspace command (split,
/// focus, close, save); otherwise `Ctrl+B` itself enters that mode,
/// `Ctrl+Q` detaches, and everything else with an obvious byte sequence
/// (printable chars, arrows, enter/tab/backspace/esc, other Ctrl+letter
/// combos) is forwarded to the focused pane as-is.
pub fn translate(event: KeyEvent, awaiting_prefix: bool) -> Option<Action> {
    if event.kind == KeyEventKind::Release {
        return None;
    }

    if awaiting_prefix {
        return match event.code {
            KeyCode::Char('%') => Some(Action::SplitVertical),
            KeyCode::Char('"') => Some(Action::SplitHorizontal),
            KeyCode::Char('o') => Some(Action::FocusNext),
            KeyCode::Char('x') => Some(Action::ClosePane),
            KeyCode::Char('s') => Some(Action::SaveWorkspace),
            // Unrecognized prefix combo: swallow it rather than leaking a
            // stray keystroke into the pane.
            _ => None,
        };
    }

    if event.code == KeyCode::Char('b') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::EnterPrefix);
    }
    if event.code == KeyCode::Char('q') && event.modifiers.contains(KeyModifiers::CONTROL) {
        return Some(Action::Detach);
    }

    let bytes = match event.code {
        KeyCode::Char(c) if event.modifiers.contains(KeyModifiers::CONTROL) => {
            let lower = c.to_ascii_lowercase();
            if lower.is_ascii_lowercase() {
                vec![(lower as u8) - b'a' + 1]
            } else {
                return None;
            }
        }
        KeyCode::Char(c) => c.to_string().into_bytes(),
        KeyCode::Enter => b"\r".to_vec(),
        KeyCode::Backspace => vec![0x7f],
        KeyCode::Tab => b"\t".to_vec(),
        KeyCode::Esc => vec![0x1b],
        KeyCode::Up => b"\x1b[A".to_vec(),
        KeyCode::Down => b"\x1b[B".to_vec(),
        KeyCode::Right => b"\x1b[C".to_vec(),
        KeyCode::Left => b"\x1b[D".to_vec(),
        KeyCode::Home => b"\x1b[H".to_vec(),
        KeyCode::End => b"\x1b[F".to_vec(),
        KeyCode::Delete => b"\x1b[3~".to_vec(),
        _ => return None,
    };

    Some(Action::Bytes(bytes))
}

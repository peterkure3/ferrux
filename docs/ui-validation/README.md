# Football Heritage Ratatui redesign

Implemented and validated on Windows with the native MSVC toolchain, 2026-11-05.
This report describes the implementation in the working tree; it does not claim
that the remote GitHub release or the already-installed daemon was updated.

## Architecture audit and changes

Ferrux already used Ratatui 0.30.2 and Crossterm 0.29.0. The nine workspace crates,
ConPTY backend, vt100 terminal parser, framed JSON control protocol and raw output
streams remain. The redesign extends the existing TUI rather than replacing the
terminal engine. Source and Cargo inspection supplied the audit; Fallow was not
installed in this environment.

`ferrux-tui/src/ui/theme.rs` centralizes the Football Heritage RGB palette and
indexed/monochrome fallbacks. `ui/layout.rs` owns responsive application geometry,
shared pane borders and content viewport dimensions. `ui/header.rs`, the existing
sidebar/status widgets and `pane_view::draw_panel` render the new chrome. The app
module retains input, daemon operations and workspace orchestration.

The header and status bar use FH Ink (#202723), the application uses FH Charcoal
(#141b17), active borders use FH Green (#147b4c), selected headers/items use
Landing Green (#1c5131), text uses Landing Cream (#f4f5ee), and attention uses
amber (#f59e0b). Inactive outlines use FH Muted (#69746c). `ACTIVE`, selection
markers and mode labels communicate focus even without color. Only default
terminal colors inherit the theme; explicit ANSI foreground/background values
are preserved, including indexed colors and truecolor.

The full-height sidebar adapts between 20 and 28 columns and automatically hides
below 60 columns. Sessions can be selected with the mouse or keyboard, collapsed,
and scrolled. Adjacent pane rectangles share exactly one border cell; their
content viewports exclude borders. Junctions merge without moving geometry when
focus changes. Empty viewports never send a zero-sized ConPTY resize.

Git branch, repository and dirty state run off the UI thread, refreshed every
five seconds. Daemon lifecycle/agent snapshots run separately with a timeout and
two-second polling. Only changed snapshots trigger redraws. Existing bounded
output notifications coalesce redraws with a 16 ms frame limit; there is no
continuous idle render loop. Scrollback remains bounded to 1,000 lines per pane.

`PaneInfo.running` is additive and optional, allowing old daemons to report
unknown lifecycle status instead of fabricated running state. Default local pipe
access and security remain unchanged. `FERRUX_PIPE_NAME` accepts only a local
`\\.\pipe\ferrux-*` endpoint for isolated instances. `FERRUX_SESSION_DIR` isolates
saved-layout files during tests. No production dependency was added; the UI
integration example reuses portable-pty and ferrux-term as development dependencies.

## Controls and running the build

```powershell
cargo build --workspace --release --locked
.\target\release\ferrux.exe open
```

- Click a pane, its border or a session row to focus it. The mouse wheel scrolls
  the hovered terminal/session list. Typing returns a scrolled pane to live output.
- Existing controls remain: Ctrl+O focus, Ctrl+B then v/h split, x close, s save,
  Ctrl+Q detach. Ctrl+S remains child input.
- Ctrl+B then j/k selects next/previous; Tab enters session navigation, arrows
  select/expand/collapse, Enter/Esc returns to terminal input.
- Ctrl+B then b toggles the sidebar, ? opens help, comma opens read-only settings,
  and w opens the saved-layout picker. Esc closes overlays.
- `FERRUX_COLOR_MODE=rgb|256|mono` overrides UI colors; `NO_COLOR` enables mono.

Run the graphical review helper from a local interactive PowerShell session:

```powershell
.\scripts\launch-ui-review.ps1 -Columns 160 -Rows 48
```

It copies the two release executables into a unique `target/ui-review/` directory,
uses its own pipe and saved layouts, and starts real PowerShell/CMD panes. The
printed instance record identifies its daemon PID and endpoint. It deliberately
leaves that review instance running for inspection; the normal installed daemon
and saved workspaces are not stopped or changed. Ctrl+Q detaches; close test panes
explicitly when finished. Review binaries/logs are generated files under target.

## Verification

Passed:

- `cargo fmt --all -- --check`.
- `cargo clippy --offline --workspace --all-targets --locked -- -D warnings`.
- `cargo test --offline --workspace --locked`: 43 unit tests, no failures.
- `cargo build --offline --workspace --release --locked`.
- Release executables: `ferrux.exe` 2,391,552 bytes;
  `ferrux-daemon.exe` 1,084,416 bytes. Packaging includes only these executables,
  with PDBs in a separate symbols archive under `target/dist/`.
- TestBackend renders at 80x24, 100x30, 120x40, 160x48 and extremely small sizes,
  with theme mappings, Unicode metadata, ANSI preservation, alternate screen,
  selected sidebar rows, focus geometry, shared edges, junctions and footer checks.
- Native PowerShell/CMD smoke test: 0/1/4/8 panes, resizing eight panes, sustained
  output, detach preserves eight live panes, reattach, CMD Unicode roundtrip,
  and five rapid creation/termination cycles. Raw results: [windows-smoke.json](windows-smoke.json).

Repeat the native UI interaction test after building the release binaries:

```powershell
cargo run --locked -p ferrux-cli --example windows_ui_smoke -- target/release target/ui-smoke
```

This starts the actual Ferrux executable inside Windows ConPTY, answers the
terminal host cursor query, sends real terminal input sequences and parses its
actual output. It checks keyboard focus, mouse pane focus, sidebar selection,
PowerShell/CMD command execution, help, save, split/close, the four requested
terminal sizes and detach preserving two shells. It uses its own daemon and
cleans up its own panes. Text captures are actual terminal cells, not generated
mockup data: [160x48](160x48.txt), [120x40](120x40.txt),
[100x30](100x30.txt), [80x24](80x24.txt).

## Remaining verification and limitations

Graphical screenshots are **not delivered**. The launched Windows Terminal and
Ferrux processes ran, and the isolated daemon listed PowerShell and CMD panes,
but Windows Terminal exposed no window to the desktop capture API. Repeated
window inventories could not select a window for capture. Text captures and
TestBackend assertions are not represented as graphical screenshots.

Physical mouse/keyboard input and visual inspection in the user's Windows
Terminal still need manual verification. ConPTY input sequences were tested;
they do not prove Windows Terminal's physical device event forwarding. Other
interactive applications and antivirus behavior have not been retested.

Approvals and notification counts have no authoritative integration in the
current architecture and display unavailable. Configuration is read-only; the
theme is controlled through environment variables. Saved TOML layouts retain
their format and existing restore behavior: reopening a saved layout respawns
shells with remapped daemon IDs. The picker preserves detached daemon processes,
but does not introduce live workspace reattachment or command-history restoration.
Old daemons without lifecycle metadata display unknown process state until an
updated daemon is used. Child ANSI mouse reporting is not forwarded: captured
mouse input belongs to Ferrux focus/scroll controls, as in the existing design.

# Pane scrolling

Ferrux previously consumed every wheel event as local vt100 scrollback. That
cannot navigate an application's fullscreen transcript: vt100's alternate grid
has no history, and applications requesting mouse input never received it.
PageUp and PageDown were also missing from the child key encoder.

Wheel events now use the hovered pane's content viewport. When its child has
requested mouse reporting, Ferrux emits the requested SGR, legacy or UTF-8 wheel
encoding, using coordinates relative to that viewport. Borders and sidebar
coordinates never reach the child. Clicks remain Ferrux focus controls.

DEC alternate-scroll mode 1007 is tracked through vt100's existing CSI callbacks,
including sequences split across PTY reads. An application requesting this mode
on the alternate screen receives three Up/Down keys per wheel event, respecting
application-cursor mode. Alternate-wheel key translation defaults on because
the Windows ConPTY fixture did not work when it depended on receiving mode 1007
from the child. Explicit mode changes received by the parser override this default.
Codex's [terminal implementation](https://github.com/openai/codex/blob/main/codex-rs/tui/src/tui.rs)
defines this alternate-scroll request. Ferrux does not fabricate transcript
history by recording every fullscreen repaint.

For ordinary terminal output, wheel scrolling uses bounded local history. New
output preserves the viewed history position; typing returns to live output.
An already-scrolled pane continues to navigate local history instead of sending
wheel events unexpectedly to its child. Shift+wheel always requests local history.

| Control | Behavior |
|---|---|
| Shift+PageUp / Shift+PageDown | Local history by one viewport, with one-line overlap |
| Shift+Home / Shift+End | Oldest retained history / live output |
| Ctrl+B then PageUp/PageDown/Home/End | Equivalent local workspace controls |
| Plain PageUp / PageDown | Forward to the child application |

The header shows `[SCROLL +N]` while viewing N rows back, and hides the child
cursor during local scrolling. Both workspace panes and `ferrux view` support
wheel/keyboard scrolling. Default history is now 10,000 rows, configured through
`FERRUX_SCROLLBACK_LINES`, bounded to 100,000 rows per attached pane. Zero disables
history; invalid settings use the default. Rows are allocated as output arrives.
This changes UI memory limits, not daemon persistence or IPC framing.

Regression tests cover stable history during new output, SGR and legacy/UTF-8
wheel encoding, viewport-relative coordinates, forced local history, chunked
alternate-scroll enable/disable and application-cursor encoding. The native
ConPTY UI harness exercises long PowerShell output, mouse history, keyboard
history, return-to-live and a fullscreen alternate-scroll fixture. The fixture
tests terminal protocols without starting an AI agent or submitting prompts.
A live authenticated Codex conversation was not exercised.

The Windows fixture passed: real PowerShell read three UpArrow key events from
one wheel event while using an alternate screen. Local history, Shift+PageUp,
Shift+End, normal typing returning to live output and existing split/focus/detach
flows also passed in the actual ConPTY-hosted Ferrux UI. The workspace has 47
passing unit tests, including the additional scrolling regressions.

Validation evidence: [native smoke output](scrolling-smoke.txt),
[actual scrolled terminal cells](scrollback.txt), and
[alternate-screen wheel result](alternate-screen.txt). These text captures are
not graphical screenshots.

Build with `cargo build --workspace --release --locked` and launch
`target/release/ferrux.exe`. Existing UI processes must be restarted to load the
new code. Existing daemons need not be terminated. Already-discarded history
cannot be recovered by this update.

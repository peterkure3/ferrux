# Streaming pane rendering

Measured on native Windows on 2026-11-16, using the repository's optimized
release profile and locked dependencies.

The workspace renderer previously constructed a `Text` with one borrowed span
per terminal cell, applied theme colors in a second pass, and asked `Paragraph`
to lay out those cells again. This happened for every visible pane every frame,
while holding that pane's emulator lock. `TerminalScreen` now copies the
emulator's existing cell grid directly into the Ratatui buffer, without the
intermediate span vectors or Unicode layout pass. Both workspace panes and the
single-pane viewer use it. Tests cover explicit ANSI colors, theme defaults,
modifiers, wide and combining characters, alternate screens, clipped viewports,
and replacing old wide glyphs.

The workspace also leaves its one-slot output notification pending while a
frame is already dirty. Previously, it could consume and refill that slot for
every output chunk between frames. Output parsing still processes every byte;
only redundant UI notifications are coalesced. The single-pane viewer releases
its emulator lock before the host-terminal flush.

## Reproduce

```powershell
cargo run --release --locked -p ferrux-tui --example multi_pane_performance
```

The example compares the previous production path, `render_screen_themed` plus
`Paragraph`, with `TerminalScreen` in the same executable. It verifies equal
colored Unicode buffers before timing, then alternates measurement order for
seven samples per pane count. Each sample parses output from every pane and
renders 500 frames into a reset 160x48 buffer. There are 1, 2, 4, or 8 emulators
with 10,000-line scrollback, matching the TUI default. The total visible cell
count stays constant as panes are added. No compiler jobs ran during timing.

Median time per frame, including parsing and buffer reset:

| Active panes | Previous (ms) | Direct grid (ms) | Speedup |
| --- | ---: | ---: | ---: |
| 1 | 1.405 | 0.559 | 2.51x |
| 2 | 1.382 | 0.553 | 2.50x |
| 4 | 1.394 | 0.529 | 2.63x |
| 8 | 1.454 | 0.618 | 2.36x |

Raw samples are in [multi-pane-render.txt](multi-pane-render.txt). These results
measure in-process parsing and terminal-grid rendering. They exclude ConPTY,
named-pipe transport, frame diffing, terminal-device writes, and agent work;
they do not establish an equivalent improvement in end-to-end input latency.
The notification-coalescing change is not included in these timings.

Validation: `cargo test --workspace --locked` (49 tests),
`cargo clippy --workspace --all-targets --locked -- -D warnings`, and
`cargo fmt --all --check` passed.

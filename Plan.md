# Ferrux — Build Plan

**Ferrux** is a Rust rewrite of `wmux` (originally built in Go), a native Windows
terminal/workspace multiplexer for AI coding agents, inspired by `cmux`.

The wmux project: https://github.com/peterkure3/wmux.git

This plan covers: the foundation strategy, SOLID application in Rust, the
workspace/folder structure, and the phased build order.

---

## 1. Foundation Strategy — Build on What Exists

`cmux` itself isn't a from-scratch terminal engine — it's an orchestration
layer wrapped around Electron + xterm.js. The Rust equivalent: **don't
hand-roll VT100 parsing or PTY handling.** Wrap proven crates and put
Ferrux's value-add — workspace/pane/agent orchestration, the daemon,
notifications — on top of them.

| Concern | Crate | Why |
|---|---|---|
| PTY spawning (ConPTY on Windows) | `portable-pty` (wezterm's crate) | Battle-tested ConPTY + Unix PTY abstraction; same crate wezterm/zellij use |
| Terminal state machine (VT100/xterm parsing, scrollback, cell grid) | `alacritty_terminal` or `vt100` | Avoids writing a custom ANSI parser — this is where most bugs live |
| TUI rendering (panes, splits, sidebar, status line) | `ratatui` + `crossterm` | De facto standard, strong Windows terminal backend support |
| CLI arg parsing / subcommands | `clap` (derive) | `ferrux new`, `ferrux split`, `ferrux attach`, etc. |
| Async runtime (daemon, IPC, PTY IO) | `tokio` | Needed for daemon + named pipe IPC concurrency |
| IPC (client ↔ daemon) | `tokio::net::windows::named_pipe` or `interprocess` | Named Pipes, matches the original Go daemon design |
| Config | `serde` + `toml` | `~/.ferrux/config.toml` |
| Cross-platform paths | `directories` | `%APPDATA%\ferrux` etc. |
| Notifications | `windows-rs` toast APIs or `winrt-notification` | Native Windows toasts |

This gives Ferrux a real head start — like cmux had xterm.js — instead of
spending the first month debugging a hand-rolled VT parser.

---

## 2. SOLID Principles in a Rust Workspace

Rust has no classes/inheritance, so SOLID translates to: **traits for
boundaries, small structs with one job, dependency injection via generics or
trait objects, and a Cargo workspace of crates so layers can't casually reach
into each other.**

- **S — Single Responsibility.** Each crate/module owns one concern: pane
  layout, PTY IO, IPC transport, and rendering are separate crates, not one
  giant `main.rs`.
- **O — Open/Closed.** `PtyBackend`, `NotificationSink`, `IpcTransport` are
  traits. Adding a new backend (a future Unix PTY, a Slack notification sink)
  means writing a new impl, not touching existing code.
- **L — Liskov Substitution.** Any `PtyBackend` impl — real ConPTY vs. a mock
  used in tests — must be swappable without the orchestrator caring.
- **I — Interface Segregation.** No single giant `Multiplexer` trait; instead
  narrow traits like `PaneManager`, `SessionStore`, `AgentDetector`, each with
  a small surface.
- **D — Dependency Inversion.** The core (`ferrux-core`) depends only on
  traits. `ferrux-daemon`, `ferrux-cli`, `ferrux-ipc` provide concrete
  implementations, wired together at the composition root (`main.rs`).

---

## 3. Cargo Workspace / Folder Structure

```
ferrux/
├── Cargo.toml                     # [workspace] members
├── crates/
│   ├── ferrux-core/                # pure domain logic, no IO, no async runtime
│   │   ├── src/
│   │   │   ├── domain/
│   │   │   │   ├── workspace.rs    # Workspace, SplitNode, Pane structs
│   │   │   │   ├── session.rs      # Session, SessionId
│   │   │   │   └── agent.rs        # AgentStatus, AgentKind
│   │   │   ├── ports/                # traits = SOLID boundaries
│   │   │   │   ├── pty_backend.rs    # trait PtyBackend
│   │   │   │   ├── notification_sink.rs
│   │   │   │   ├── session_store.rs  # trait SessionStore (persistence)
│   │   │   │   └── ipc_transport.rs
│   │   │   ├── layout/                # split-tree algorithms (pure, testable)
│   │   │   │   └── split_tree.rs
│   │   │   └── lib.rs
│   │   └── Cargo.toml
│   │
│   ├── ferrux-pty/                 # impl PtyBackend using portable-pty + ConPTY
│   │   └── src/conpty_backend.rs
│   │
│   ├── ferrux-term/                # wraps alacritty_terminal/vt100 for grid+scrollback
│   │   └── src/emulator.rs
│   │
│   ├── ferrux-daemon/              # long-running process: owns PTYs, supervises panes
│   │   ├── src/
│   │   │   ├── supervisor.rs       # init-system-style pane restart logic
│   │   │   ├── watchdog.rs         # memory pressure / health checks
│   │   │   ├── pipe_server.rs      # IPC transport impl
│   │   │   └── main.rs
│   │   └── Cargo.toml
│   │
│   ├── ferrux-ipc/                 # shared request/response types, named pipe transport
│   │   └── src/protocol.rs         # serde-serializable messages, versioned
│   │
│   ├── ferrux-tui/                 # ratatui rendering, keybindings, sidebar
│   │   ├── src/
│   │   │   ├── widgets/
│   │   │   │   ├── pane_view.rs
│   │   │   │   ├── sidebar.rs
│   │   │   │   └── status_line.rs
│   │   │   ├── input/
│   │   │   │   └── keymap.rs
│   │   │   └── app.rs              # ties TUI to daemon via ipc client
│   │   └── Cargo.toml
│   │
│   ├── ferrux-notify/              # impl NotificationSink (Windows toasts, bell)
│   │
│   ├── ferrux-agent-detect/        # AgentDetector impl (Claude Code / Codex / etc process sniffing)
│   │
│   └── ferrux-cli/                 # thin binary: clap subcommands -> ipc client calls
│       ├── src/main.rs
│       └── Cargo.toml
│
├── config/
│   └── default.toml
└── tests/
    └── integration/                # spin up daemon + fake pty backend, assert behavior
```

**Why this shape enforces SOLID in practice:**
- `ferrux-core` has zero IO dependencies — fast to test, and it can't
  accidentally couple to ConPTY or tokio.
- Swapping `ferrux-pty`'s ConPTY implementation for a mock in `tests/` means
  providing a different `PtyBackend` impl — no core code changes (O + L in
  action).
- `ferrux-cli` and `ferrux-tui` never talk to PTYs directly, only through
  `ferrux-ipc` to the daemon — the same separation the Go daemon already had,
  now enforced by the crate boundary instead of by convention.

---

## 4. Naming Conventions

- Binary/command: `ferrux` (e.g. `ferrux new`, `ferrux split -H`, `ferrux attach`)
- Config file: `~/.ferrux/config.toml`
- Daemon IPC pipe: `\\.\pipe\ferrux-daemon`
- Crate prefix: `ferrux-*`

---

## 5. Build Phases

1. **Skeleton + core domain.** `ferrux-core` with `Workspace` / `Pane` /
   `SplitNode`, unit-tested split-tree math, zero IO.
2. **PTY + daemon spine.** `ferrux-pty` (ConPTY via `portable-pty`),
   `ferrux-daemon` that can spawn/list/kill a pane over a Named Pipe,
   `ferrux-cli` with `new` / `ls` / `attach` / `kill-pane`.
3. **Terminal rendering.** Wire `alacritty_terminal`/`vt100` grid into
   `ferrux-tui` via ratatui, get one pane rendering live output.
4. **Splits + sidebar + persistence.** Split-tree rendering, session
   save/restore (`SessionStore` impl backed by TOML/JSON), workspace
   switching.
5. **Agent layer.** `ferrux-agent-detect` (process-name based detection),
   sidebar status lines, notification sink (toast + bell).
6. **Supervisor / init-system behavior.** Auto-restart panes declared in a
   `ferrux.json` manifest, watchdog for memory pressure, matching the
   original daemon's resilience story.

---

## 6. Open Questions to Resolve Before Coding

- Named Pipes via `tokio`'s built-in Windows support vs. the `interprocess`
  crate — evaluate both for ergonomics before locking in `ferrux-ipc`.
- `alacritty_terminal` vs. `vt100` for the terminal emulator — `alacritty_terminal`
  is more feature-complete but has a heavier API surface; `vt100` is lighter
  and may be enough for scrollback + cell grid needs.
- Session persistence format — TOML (human-editable) vs. JSON (matches the
  original `wmux.json` manifest naming) for `SessionStore`.
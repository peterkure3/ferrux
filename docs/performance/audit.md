# Ferrux implementation audit

Audited on Windows with Rust 1.99.0, x86_64-pc-windows-msvc. The checked-out tree and Plan.md are authoritative. Existing untracked CODEBASE_INDEX.md, codebase-index.json and Updates.md were left untouched.

## Architecture

All nine planned crates exist. Core contains serde domain records, pure split-tree operations and narrow adapter traits. PTY wraps portable-pty/ConPTY; term wraps vt100 (one parser implementation). IPC owns version-2 length-prefixed JSON requests/responses and switches to raw bytes after attach. Daemon owns PTYs, screen snapshots, restart supervision, process detection and the memory watchdog. TUI owns local emulators, Ratatui/Crossterm rendering, key translation and TOML persistence; CLI composes it with daemon startup. Notify wraps Windows beep and WinRT toast; agent-detect classifies Claude/Codex descendant processes.

Plan.md's proposed strict UI/daemon separation is mostly preserved, though TUI necessarily has its own screen emulator for rendering. This is two instances of the same proven parser, not redundant parser dependencies. No crate consolidation is justified. Core stays independent of Tokio and OS APIs. Existing public trait boundaries remain intact.

## Repository and build inventory

Reviewed every crate manifest and Rust module, Cargo.lock, root manifest, ignore file, config/default.toml, README, installer and Plan.md. No workspace build.rs, CI workflows, custom Cargo configuration, benchmark harness, or release packaging workflow existed. The integration-test folder contains only .gitkeep; actual existing tests are module unit tests. Original profiles were Cargo defaults. The daemon's direct portable-pty declaration and TUI's direct serde declaration have no source usages; compilation verified they can be removed while retaining the actual PTY and serialization implementations. No tracing dependency exists.

Fallow was requested by the codebase-intelligence skill but is not installed. Its JS/TS-oriented findings cannot substitute for Cargo/Rust inspection. cargo-bloat, cargo-udeps, cargo-machete, sccache and lld-link were not found on PATH. No optional analyzer or alternate linker is required to build.

## Findings before changes

* Existing debug cache exceeds 7.6 billion bytes: accumulated hashed dependency outputs, object files, PDBs and incremental sessions, not application payload.
* Each pane polls a new full sysinfo snapshot every two seconds, then repeatedly scans every process to traverse children. The watchdog also creates one snapshot per pane every five seconds. Both run blocking collection via Tokio workers.
* PTY synchronous readers correctly require dedicated blocking threads. Output broadcast is bounded (1024 chunks of at most 4096 bytes), but Vec payloads clone per subscriber. No-subscriber output still allocates before send fails.
* TUI redraw and input queues are unbounded. Every output chunk requests a full draw; unchanged agent status also triggers a draw every two seconds. PaneSession does not cancel its reader when dropped, so detach can leave a pipe task alive until process exit.
* render_screen builds one Span per cell, with vector/Span overhead rather than owned strings per cell. Wide-character continuation cells become extra spaces, shifting subsequent text. vt100 already exposes borrowed cell contents. A style-run coalescing experiment reduced span storage but consistently regressed the selected-release full-render benchmark; it was reverted. The final renderer retains borrowing and fixes wide-cell continuation handling.
* Crossterm resize events are discarded. TUI raw-mode cleanup can be skipped by early error propagation.
* IPC trusts an arbitrary u32 length before allocation. Preserve framing and JSON compatibility while bounding incoming messages.
* Attach subscribes before snapshot and can replay output twice. Lagged broadcast currently silently loses bytes; parser state may then be invalid. This requires ordered snapshot/stream recovery rather than a serialization rewrite.
* Natural pane exit does not clear alive; process detection remains scheduled. Restart loops have no backoff.
* Existing workspace load respawns saved panes even if the original daemon still owns them. Saved sessions represent layouts, not durable daemon session identities. Do not silently change that compatibility contract during optimization.
* Named pipes use Tokio defaults and first-instance exclusion. Local access-control hardening needs careful explicit ACL design; benchmark tools must never attach to or terminate an existing user daemon.

## Dependency decisions

See dependency-audit.md for retained versions and feature reductions. No major migrations, handwritten terminal parser, new async framework or unsafe hot-path code is warranted. Build experiments must use fresh target directories and warm dependency downloads, and keep their storage separate from the historical cache. No automatic cleanup is added.

## Validation scope

Unit tests cover domain layout, persistence, parser behavior and agent-name classification. New regressions should cover framing limits, Unicode rendering, attach cancellation and shared detection. A deterministic example supplies parser/render/serialization measurements without a benchmark framework dependency. The Windows harness measures daemon-only memory/CPU/handles and real named-pipe operations with PowerShell children. Interactive Windows Terminal keyboard/visual behavior, antivirus behavior and long-running agents require additional manual validation; automated compilation alone does not prove those requirements.

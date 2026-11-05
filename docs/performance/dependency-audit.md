# Dependency audit

Cargo.lock remains committed. The audit used `cargo metadata --format-version 1`, `cargo tree --workspace`, `cargo tree --duplicates`, `cargo tree --edges features`, and `cargo tree --workspace --depth 1` with --locked. The initial offline attempt failed because the shared cache required unpacking outside the sandbox; the approved retry populated it. Subsequent build measurements use --offline and exclude downloads.

## Direct dependency review

| Dependency | Source requirement / decision |
|---|---|
| Tokio | Named pipes, runtime macros, select, time, spawn_blocking, channels and split I/O. No `full` feature was present. CLI uses io-std; its process feature is unnecessary because daemon startup uses std::process. IPC only needs io-util in normal dependencies; async tests need runtime/macros. |
| Ratatui | Paragraph, Block, List, style/text, Terminal and CrosstermBackend. Calendar and macros defaults are unused; preserve Crossterm 0.29 and underline-color capability. Ferrux computes pane rectangles itself. |
| Crossterm | Terminal mode, key/resize events, command execution. Preserve normal event support. No backend migration. |
| sysinfo | Process names, parents, PID and memory only. Component/disk/network/user features expose unused Windows APIs and can be disabled; retain system. |
| portable-pty | Required in ferrux-pty for ConPTY handles, process lifecycle and native command builder. Removed the unused direct daemon declaration; methods on the exported PTY handle types compile without it. |
| windows-sys | MessageBeep and MB_OK in notifier. Both declared features are used. |
| winrt-notification | Native toast implementation; older Windows binding graph is required by this maintained integration surface. No speculative replacement. |
| vt100 | Single proven VT parser and bounded scrollback. Preserve UTF-8, alternate-screen and escape semantics. |
| clap | CLI derive and help/error experience retained. Removing derive without a measured benefit would reduce maintainability. |
| serde / serde_json | Domain persistence and IPC/manifest JSON. Removed TUI's unused direct serde declaration; core types retain their derives. |
| toml | Existing saved workspace format; not redundant with JSON because both formats are public compatibility contracts. |
| directories | Resolves home directory for workspace store; preserve Windows path behavior. |
| anyhow | PTY adapter converts the portable-pty error type; used explicitly. |

## Duplicate versions

The original graph includes bitflags 1/2, hashbrown 0.16/0.17, heck 0.3/0.5, strum and macros 0.22/0.28, syn 1/2/3, thiserror 1/2 and Windows 0.24/0.62. These are transitive incompatible version requirements, not nine independent copies of every workspace dependency. Forcing a single version via patches would require upstream migrations. Windows-sys 0.61 and Windows 0.62 expose different APIs; WinRT notification also uses Windows 0.24. Retain required bindings while narrowing sysinfo's feature surface.

Cargo's resolver 2 already unifies compatible normal workspace dependencies; workspace.dependencies prevents declaration drift but does not magically eliminate build/test/host compilation units. Proc macros (serde/clap/Tokio/strum/Windows/instability) are compile-time tools. No runtime async/synchronization framework duplication warrants migration.

## Tools and limits

cargo-machete, cargo-udeps and cargo-bloat were unavailable. Source usage, platform modules and Cargo graph were checked manually; no direct dependency was removed solely on static-analysis output. Fresh executable and artifact lengths are authoritative; no symbol-level size attribution is claimed without a bloat/PE profiler. No package upgrades were intentionally introduced as performance work.

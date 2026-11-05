# Ferrux performance and storage report

Implemented against baseline commit `8715805f9e2ab259ffe8a46960f2992f09148e33`; all nine crates and the native Windows ConPTY/named-pipe architecture remain intact. Full acceptance is **not claimed** for the unverified items below.

## Storage diagnosis

The initial target contained **8,272,548,498 bytes**, including debug dependencies **4,024,979,798 bytes** and debug incremental state **3,319,145,044 bytes**. Crate source was **94,179 bytes**. Repeated hashed dependency outputs, full symbols, object files and retained incremental sessions caused the growth. Shared Cargo caches are accounted separately in [storage-audit.md](storage-audit.md); they are not Ferrux-owned application data.

## Implemented changes

* Shared workspace dependency declarations; removed unused direct daemon portable-pty and TUI serde declarations. Disabled unused Ratatui calendar/macros/layout-cache and sysinfo component/disk/network/user features. Narrowed IPC's Tokio requirements and removed CLI's unused process feature. Required dependency versions and Windows notification support remain intact.
* Dev keeps incremental compilation and limited workspace PDBs, without dependency debug information. Release uses measured fat LTO and retains unwind behavior and available PDBs. Thin LTO and the much slower size-optimized candidate were rejected. Style-run coalescing also regressed full-buffer rendering and was reverted.
* One minimal process snapshot serves every pane per detection interval; watchdog memory collection is batched. Natural exit clears the current PID, and crash restarts have a short backoff.
* PTY output uses shared byte slices; detached output avoids allocation. Attach snapshot/subscription is ordered with output publication, and lagged clients recover the authoritative screen. Blocking PTY commands/input writes run off Tokio I/O workers. Writer acquisition happens before child spawning to avoid leaving a child after writer-setup failure.
* UI queues are bounded; redraw notifications coalesce and dirty frames are paced at 16 ms. Unchanged agent status does not redraw. Dropped attachments cancel tasks; normal detach drains accepted input first. Resize events propagate, terminal modes restore on error, and wide-character continuation cells no longer shift subsequent text.
* Control frames are limited to 4 MiB before allocation, preserving v2 framing and raw attach bytes. Named-pipe local-access defaults remain intact; no new network service or command interface is introduced.
* Storage analysis, confirmation-gated selective cleanup, build/runtime measurements, Windows smoke tests and two-executable release packaging are provided under scripts/. PDBs are packaged separately. install.ps1 remains compatible.

## Final clean-build comparison

Fresh directories, locked offline dependencies, same native compiler, serial final builds. Bytes are logical lengths, not physical NTFS allocation.

| Metric | Before | Final |
|---|---:|---:|
| Debug build artifacts (bytes) | 1,181,212,445 | 659,851,367 |
| Clean debug build (s) | 237.9954754 | 112.2627065 |
| Release build artifacts (bytes) | 681,482,372 | 549,977,798 |
| Clean release build (s) | 249.3366218 | 181.5965828 |
| ferrux.exe release (bytes) | 2,289,664 | 2,147,840 |
| ferrux-daemon.exe release (bytes) | 1,154,560 | 1,083,904 |
| Eight-pane daemon private memory, matched debug (bytes) | 6,094,848 | 4,747,264 |
| Eight-pane idle CPU, matched debug (% of one core) | 41.666667 | 3.645833 |
| Eight-pane connect/ListPanes mean, matched debug (ms) | 2.636215 | 1.848663 |

Fresh debug storage fell **44.14%**, below the aspirational 50% target while retaining diagnostics and local incremental reuse. Clean-build time improved in these samples; no-change rebuilds did not improve. The eight-pane CPU sample fell **91.25%**. Empty-daemon memory rose by 65,536 bytes, and sampled daemon readiness regressed from 754.9796 to 993.2300 ms. Final paired parser, in-memory serialization and CLI-startup medians also regressed by 5.45%, 14.25% and 4.95%; fat LTO is retained for size, without claiming uniform runtime gains. See [benchmarks.md](benchmarks.md) for raw-file provenance, rendering results and measurement limitations.

Historical caches were **not deleted**. With explicit user approval, the optional cleanup helper removed only `D:\Github\ferrux\target\performance`, recovering **8,417,729,585 logical bytes** of this task's temporary benchmark caches. All saved measurements, source, Cargo.lock, release archives and global Cargo caches remain. The post-cleanup target is **8,274,600,363 bytes**, mostly the untouched historical cache; the 44.14% reduction compares equivalent fresh builds, not that retained directory. New profile settings cannot retroactively remove old artifacts. `-Scope Debug` can separately preview the historical debug cache; actual deletion requires `-Delete` and explicit confirmation. No automatic/global cleanup runs.

## Validation and remaining work

Passed: 27 unit tests plus doc tests, cargo fmt check, strict all-target Clippy, locked native debug/release builds, saved-workspace TOML round trips, framed IPC regressions, attachment cancellation/input-drain regressions, Unicode/colored-blank rendering tests, actual 0/1/4/8-pane PowerShell tests, CMD Unicode output, resize, simultaneous output, detach/reattach, five create/kill cycles, storage preview/fixture tests and archive-content inspection. The final packaged-release smoke test also passed; its eight-pane private memory was 4,825,088 bytes and idle CPU was 2.083333% of one core (windows-final-release.json). Matched debug binaries supply the controlled before/after daemon comparison above. Exact commands and evidence are in benchmarks.md and adjacent JSON/text results.

Still unverified: interactive Windows Terminal keyboard/visual behavior and live UI latency/CPU; long-running real AI agents; repeated interactive workspace switching; peak build memory and native allocation profiles; full descendant cleanup/restart races and intentional overflow stress; antivirus behavior. The computer-use skill's guidance prohibits terminal-app automation, so those desktop checks remain manual. Missing Fallow/bloat/udeps/machete/sccache tools are documented, not represented as successful checks. Cross-platform compatibility is not claimed.

Remaining bottlenecks include mandatory periodic OS process enumeration, per-cell frame construction/whole-layout work when one pane changes, synchronous PTY reader threads required by portable-pty, two terminal grids per attached pane, and legacy workspace restore semantics that respawn saved layouts rather than reconnect durable daemon workspace identities. Scrollback remains bounded at the existing 1,000-line app default and configurable through the Emulator constructor; user config/default.toml is not newly wired into runtime configuration.

Build/run instructions are in [README.md](../../README.md). Build both executables with `cargo build --workspace --locked`, then run `target/debug/ferrux.exe open`. Distribution uses `cargo build --workspace --release --locked` followed by `scripts/package-release.ps1`.

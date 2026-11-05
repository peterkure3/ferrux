# Reproducible measurements

Measured 2026-11-04 on native Windows 11 build 26200, MSVC Rust 1.99.0, PowerShell 5.1. See environment.json for the baseline Git commit and environment. Baseline dependencies were downloaded/unpacked before timing. Every clean build uses a separate initially empty target directory, the same toolchain, workspace scope and locked dependencies, with networking disabled. No cargo clean was used. Logical file sizes can count NTFS hardlinks twice.

Raw build-*.json files preserve elapsed seconds, logical artifact bytes, executable lengths and commands. A no-change rebuild is not a source-edit incremental benchmark. Cold filesystem/antivirus effects and unrelated machine load remain possible; these are engineering samples, not statistically controlled machine-wide performance guarantees. A final development build that overlapped validation is identified separately rather than treated as an equivalent clean-build timing.

## Runtime workloads

`cargo run --release --locked -p ferrux-tui --example performance` feeds 11,010,048 bytes of representative colored Unicode/combining-character output through vt100, constructs 2,000 120x40 terminal Text frames, and serializes/deserializes 10,000 framed JSON ResizePane requests in memory. `scripts/measure-runtime.ps1` runs seven samples of each already-built candidate, rotating order. Medians from runtime-paired.json:

| Candidate | Parse 11,010,048 bytes (ms) | Construct 2,000 Text frames (ms) | 10,000 serialization round trips (ms) | CLI --help process wall time (ms) |
|---|---:|---:|---:|---:|
| Original defaults | 375.7127 | 448.6280 | 13.8137 | 41.3655 |
| Optimized code, no cross-crate LTO | 353.4668 | 575.2794 | 8.9699 | 42.6831 |
| Thin LTO | 282.8872 | 459.1910 | 7.1235 | 41.9180 |
| Fat LTO (selected) | 320.6945 | 419.4684 | 7.6957 | 40.3523 |
| Size candidate: s + thin + one codegen unit | 646.0858 | 675.8997 | 20.7724 | 48.4058 |

These candidate measurements include the subsequently rejected style-run coalescing experiment. Text construction alone became slower without LTO: coalescing created owned strings instead of borrowed per-cell spans. It was reverted after the full-render regression below. Final-source measurements are reported separately; the Unicode continuation correctness fix remains.

`render_comparison` freezes the original per-cell implementation in benchmark-only code and checks identical rendered ASCII buffers before comparing Text construction + Paragraph rendering into a real Ratatui Buffer. It alternates order for seven 500-frame samples, resetting the buffer each frame. The steady run medians are 380.0508 ms before and 411.0986 ms after in the no-LTO comparison binary: this sample does **not** establish a rendering speed improvement. Individual samples vary considerably (render-comparison-steady.txt). Earlier samples under build load favored the coalesced renderer, so they cannot justify a speed claim. The selected release profile and final render comparison must be considered separately. This benchmark excludes terminal-device writes and user-visible frame latency.

The selected fat-LTO coalescing comparison (render-comparison-fat.txt) regressed in all seven samples: medians were 774.6114 ms before and 799.9481 ms after for 500 complete buffer frames. Coalescing is therefore **not retained**. The final implementation keeps borrowed per-cell spans and skips wide-character continuation cells. Rendering improvements focus on bounded/coalesced notifications, a 16 ms frame interval while dirty, and no redraw for unchanged agent status.

The memory serialization test excludes kernel IPC. The Windows harness below measures real connect + ListPanes request + response, including PowerShell/.NET/JSON overhead; it is not a Rust-only serialization latency measurement. Existing raw and initially contended sample files are retained for provenance, not substituted for the paired medians.

## Release profile experiments

`scripts/compare-release-profiles.ps1` generates isolated Cargo configuration files and builds every variant serially. These candidate builds include the later-reverted coalescing experiment; exact final-source builds are listed below. Build sizes are sampled before compiling the examples, which can create additional feature-unification artifacts. No dependency versions were upgraded.

| Candidate | Clean build (s) | Build artifacts (bytes) | ferrux.exe (bytes) | daemon.exe (bytes) |
|---|---:|---:|---:|---:|
| Optimized code, no cross-crate LTO | 166.6048992 | 548,779,155 | 2,339,328 | 1,234,944 |
| Thin LTO | 151.6371401 | 555,299,038 | 2,356,736 | 1,238,016 |
| Fat LTO | 164.1761379 | 549,688,589 | 2,115,584 | 1,083,904 |
| Size candidate | 138.8451904 | 530,087,984 | 1,547,264 | 887,296 |

Fat LTO reduced combined executable bytes by 10.486% relative to optimized code without cross-crate LTO and improved the representative parser median. It is selected only for release; development keeps ordinary fast code generation. Thin LTO made these executables larger. The size candidate saves a further 764,928 executable bytes relative to fat LTO, but its parser workload takes about twice as long. It is rejected as the normal distribution profile, and no size profile is imposed on developers.

Unwind behavior is retained: Tokio task panic isolation and diagnostics should not be sacrificed for an unmeasured abort-size gain. `strip=debuginfo` is not retained; available Windows PDBs remain separate diagnostic artifacts. A release package override of opt-level=3 would duplicate the normal release dependency setting. Profile semantics and defaults follow the [Cargo profile reference](https://doc.rust-lang.org/cargo/reference/profiles.html).

## Real Windows daemon and ConPTY

The harness refuses to run if a user daemon is already listening. It owns the daemon it starts, creates PowerShell children through the actual v2 named pipe, and cleans up only those panes and that daemon. Working directories are isolated from any ferrux.json manifest. It runs 0/1/4/8 panes, samples daemon-only private bytes, handles and CPU over three seconds, resizes all eight, attaches eight raw streams, drains 2,000 output lines per pane through an end marker, detaches/re-attaches, checks CMD Unicode output, and creates/kills five additional CMD panes. End-marker commands concatenate strings so echoed input cannot falsely satisfy the check. UTF-8 decoding preserves characters split between reads.

Steady run results (windows-before-steady.json and windows-after-steady.json), with no concurrent compiler job:

| Metric | Before | After |
|---|---:|---:|
| Daemon readiness including connection retry (ms) | 754.9796 | 993.2300 |
| 0 panes private bytes | 1,933,312 | 1,998,848 |
| 0 panes idle CPU, % of one core | 0 | 0 |
| 1 pane private bytes | 2,772,992 | 2,727,936 |
| 1 pane idle CPU, % of one core | 5.208333 | 4.687500 |
| 4 panes private bytes | 3,682,304 | 3,399,680 |
| 4 panes idle CPU, % of one core | 21.875000 | 1.562500 |
| 8 panes private bytes | 6,094,848 | 4,747,264 |
| 8 panes idle CPU, % of one core | 41.666667 | 3.645833 |
| 8 panes sampled daemon handle count | 272 | 181 |
| 8 panes connect/ListPanes mean (ms) | 2.636215 | 1.848663 |
| Reattach command response (ms) | 95.7663 | 55.6456 |

Eight-pane CPU dropped 91.25% in this sample; private memory dropped about 22.11%. Empty-daemon fixed memory rose by 65,536 bytes. Startup regressed in this readiness sample and needs further investigation; CLI --help startup remained similar. Handles are instantaneous samples and can include transient process-enumeration handles, not proof of a leak-rate improvement. Shell/agent memory is excluded. Drain timings are sequential stream-drain waits, not independent per-pane throughput or total input latency. Marker receipt verifies completion but does not assert every intermediate line survives an intentionally overwhelmed subscriber; overflow recovery preserves the visible authoritative screen, not unlimited output history.

## Coverage and remaining measurements

Passed: workspace unit/doc tests, formatting, strict Clippy, Windows compilation/linking, real PowerShell/CMD PTY creation, named-pipe control, resize, simultaneous output, detach/reattach, Unicode command output, repeated pane kill/create, and storage helper preview/fixture tests. The renderer comparison asserts complete ASCII buffer equality; unit tests cover wide/combining characters and colored blanks. Saved workspace TOML round trips remain compatible.

Not measured/verified: peak build memory, allocation counts from a native profiler, user-visible keyboard/frame latency, repeated interactive workspace switching, live UI idle CPU, long-running real AI agents, shell descendant/job-tree cleanup in every application, restart races under kill, deliberate subscriber-overflow stress, antivirus packaging behavior and interactive Windows Terminal visual/keyboard compatibility. The installed computer-use guidance prohibits automating terminal apps, so scripted ConPTY success does not mark those UI checks complete. cargo-bloat/udeps/machete, Fallow and sccache were unavailable. No alternate linker was enabled.

## Reproduction

```powershell
./scripts/measure-build.ps1 -TargetDirectory target/performance/new-dev -Profile dev -OutputPath target/new-dev.json
./scripts/measure-build.ps1 -TargetDirectory target/performance/new-release -Profile release -OutputPath target/new-release.json
./scripts/compare-release-profiles.ps1 -ExperimentDirectory target/performance/new-experiments -ResultsDirectory target/new-results
cargo run --release --locked -p ferrux-tui --example performance
cargo run --release --locked -p ferrux-tui --example render_comparison
./scripts/benchmark-windows.ps1 -BinaryDirectory target/release -OutputPath target/windows.json
./scripts/test-storage-scripts.ps1
```

Use the recorded baseline Git commit in a separate checkout to reproduce original profiles/code. Do not compare an accumulated historical target directory to a freshly built target and call that an optimization percentage. Experiment caches were retained until verification, then removed using the cleanup helper with explicit user approval. All measurements and release ZIPs remain; rebuilding is required to rerun deleted benchmark executables. Final build comparisons follow below.

## Final source comparison

These fresh serial builds include the accepted-input detach fix and the reversal of style-run coalescing. `build-after-*-clean.json` and `build-after-*-incremental.json` are the final results; the latter files measure no-change rebuilds. Final runtime samples rotate the baseline and verified executables seven times (runtime-final-paired.json), with no compiler job running.

| Metric | Before | Final | Change |
|---|---:|---:|---:|
| Fresh debug artifact storage (bytes) | 1,181,212,445 | 659,851,367 | -44.14% |
| Fresh release artifact storage (bytes) | 681,482,372 | 549,977,798 | -19.30% |
| Clean debug build (s) | 237.9954754 | 112.2627065 | -52.83% |
| No-change debug build (s) | 0.8478425 | 1.7983596 | +0.9505171 s |
| Clean release build (s) | 249.3366218 | 181.5965828 | -27.17% |
| No-change release build (s) | 1.0680504 | 1.2536426 | +0.1855922 s |
| Release executables combined (bytes) | 3,444,224 | 3,231,744 | -6.17% |
| Warm CLI --help wall time, median (ms) | 50.0263 | 52.5049 | +4.95% |
| Eight-pane daemon private memory (bytes) | 6,094,848 | 4,747,264 | -1,347,584 bytes |
| Eight-pane idle CPU (% of one core) | 41.666667 | 3.645833 | -91.25% relative |
| Eight-pane real pipe connect/ListPanes mean (ms) | 2.636215 | 1.848663 | -0.787552 ms |
| Parse 11,010,048 bytes, median (ms) | 528.8564 | 557.6716 | +5.45% |
| Construct 2,000 terminal Text frames, median (ms) | 564.3886 | 322.6951 | -42.82% |
| In-memory 10,000 framed JSON round trips, median (ms) | 11.3627 | 12.9817 | +14.25% |
| Peak build memory | Not measured | Not measured | No claim |
| User-visible terminal frame latency | Not measured | Not measured | No claim |

The final same-profile buffer-render reference comparison has medians of 483.0858 ms before and 421.8009 ms after for 500 frames (render-comparison-final.txt). This is a benchmark-only reference for old per-cell behavior compiled with the selected profile, not a measurement of the original application's device writes. Samples vary and some regress; it does not justify a universal per-frame speed claim. The final renderer retains borrowed cells and the Unicode fix; event coalescing/frame pacing reduces how often full rendering is requested.

Final parser, serialization and CLI-startup medians regressed despite earlier candidate medians favoring fat LTO. Do not substitute the earlier favorable samples. The parser implementation was not replaced or stripped of functionality. Fat LTO is retained for the measured executable-size benefit; it is not a claim of uniform runtime acceleration. The serialization regression is about 0.162 microseconds per in-memory round trip, while control-pipe transactions include millisecond-scale connection/host overhead. Existing JSON compatibility and the allocation limit take priority over an unsupported binary-protocol rewrite. Repeated, isolated native profiling would be needed to separate code-generation effects from machine variability.

The fresh-debug storage target of 50% was not reached: the measured reduction is 44.14% while retaining incremental compilation and useful workspace PDBs. Original accumulated caches are untouched. The isolated experiments temporarily consumed 8,417,729,585 logical bytes and were subsequently removed with user approval. Source-edit incremental speed, peak build memory and interactive UI performance remain open measurements. No-change builds did not improve.

The final packaged release also passed the complete Windows harness: eight panes survived detach, CMD Unicode and five rapid create/kill cycles succeeded, and all panes were resized and produced output. That run sampled 4,825,088 daemon private bytes and 2.083333% of one core at eight panes (windows-final-release.json). The controlled before/after daemon rows above deliberately use matched debug builds. The release archive was inspected and contains exactly ferrux.exe and ferrux-daemon.exe; symbols are in a separate archive.

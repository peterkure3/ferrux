# Ferrux

A native Windows terminal/workspace multiplexer for AI coding agents,
written in Rust — a rewrite of [wmux](https://github.com/peterkure3/wmux),
inspired by `cmux`. See [Plan.md](Plan.md) for the architecture and build
phases.

## Install

```powershell
irm https://raw.githubusercontent.com/peterkure3/ferrux/main/install.ps1 | iex
```

Installs to `%ProgramFiles%\Ferrux` and adds it to the system PATH.
Requires administrator privileges — you'll get a UAC prompt if the
shell isn't already elevated. Open a new terminal afterward and run:

```powershell
ferrux open
```

To install a specific release instead of the latest:

```powershell
$Version = "v0.1.0"; irm https://raw.githubusercontent.com/peterkure3/ferrux/main/install.ps1 | iex
```

**Verified**: v0.1.0's installer was run end-to-end on 2026-07-25 —
self-elevation via UAC, download with live progress, extraction to
Program Files, machine PATH update, and `ferrux --help` resolving
correctly from a fresh shell all confirmed working.

## Usage

- `ferrux open [name]` — open (or resume) a multi-pane workspace
- `ferrux new --shell <exe>` — spawn a standalone pane
- `ferrux ls` — list panes on the daemon
- `ferrux attach <id>` / `ferrux view <id>` — raw / rendered attach to a single pane
- `ferrux kill-pane <id>` — kill a pane
- `ferrux workspaces` — list saved workspaces

Inside `ferrux open`: `Ctrl+O` to cycle focus, `Ctrl+B` then `v`/`h` to
split vertical/horizontal, `x` to close the focused pane, `s` to save
the workspace, `Ctrl+Q` to detach. Left-click inside a pane or on its border
to focus it. The active pane has a Football Heritage green border, a forest-green
header and an `ACTIVE` label; its sidebar entry is highlighted. Keyboard input goes to that pane. Mouse
capture is released when you detach or the UI exits.

The Ratatui interface uses charcoal, forest green, cream and amber, with shared
pane borders and a responsive sidebar. `Ctrl+B` then `?` opens help, `,` shows
theme settings, `b` toggles the sidebar, `Tab` enters keyboard session navigation,
and `w` selects a saved workspace layout. In session navigation use arrows and
Enter/Esc; `j`/`k` after the prefix also change focus. The mouse wheel scrolls the
hovered terminal or session list. `Ctrl+S` continues to reach the child application;
saving uses `Ctrl+B` then `s`.

UI color can be overridden with `$env:FERRUX_COLOR_MODE='rgb'`, `'256'` or `'mono'`;
`NO_COLOR` also enables monochrome UI chrome. Explicit child ANSI colors remain
unchanged. Git metadata is read in the background. Unsupported approvals and
notification counts are shown as unavailable. See the
[UI implementation and validation report](docs/ui-validation/README.md).

## Build and development

Use the native Windows MSVC Rust toolchain and Visual Studio C++ build tools.
Build both the CLI and its sibling daemon before launching:

```powershell
cargo build --workspace --locked
./target/debug/ferrux.exe open
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --locked
cargo build --workspace --release --locked
./scripts/package-release.ps1
```

Development builds keep incremental compilation and limited workspace PDBs;
dependency debug information is disabled. For full workspace variable/type
diagnostics use `cargo --config profile.dev.debug=2 build --workspace --locked`.
CI can set `CARGO_INCREMENTAL=0` for disposable builds; local iteration keeps it
enabled. No alternate linker or compiler cache is required.

Release uses measured fat LTO with unwind behavior preserved. Thin LTO made
the tested binaries larger; size-oriented optimization substantially slowed
terminal parsing. See the benchmark report for these tradeoffs. Use targeted
commands such as `cargo test -p ferrux-core --locked` when changing one crate.

The installer archive contains only `ferrux.exe` and `ferrux-daemon.exe`.
`package-release.ps1` writes a separate symbols archive when PDBs are present.
The installer asset name and upgrade workflow are preserved.

Inspect storage before cleaning anything:

```powershell
./scripts/analyze-build-size.ps1 -IncludeGlobalCaches -JsonPath target/storage.json
./scripts/clean-build-cache.ps1
./scripts/clean-build-cache.ps1 -Scope Incremental -Delete -WhatIf
```

The cleanup helper previews fixed generated directories. Actual deletion requires
`-Delete` and explicit confirmation. It never touches global Cargo caches, source,
or `Cargo.lock`. Stop builds first. Profile changes do not remove historical cache
variants; do not run `cargo clean` on every build.

See the [performance audit](docs/performance/audit.md),
[storage measurements](docs/performance/storage-audit.md),
[benchmarks](docs/performance/benchmarks.md), and
[implementation report](docs/performance/final-report.md) for results and limits.

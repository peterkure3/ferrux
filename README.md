# Ferrux

A native Windows terminal/workspace multiplexer for AI coding agents,
written in Rust — a rewrite of [wmux](https://github.com/peterkure3/wmux),
inspired by `cmux`. See [Plan.md](Plan.md) for the architecture and build
phases.

## Install

**The one-line installer downloads the latest published GitHub release, not the
latest code on `main`.** Releases can lag behind the code in this repository.
To get the recent performance improvements, command labels, and updater changes,
build from `main` using the instructions below.

### Latest code from main

Requires Git, the native Windows MSVC Rust toolchain, and Visual Studio C++
build tools. Close any running Ferrux UI and daemon before launching the new
build, so it does not connect to an older daemon. In PowerShell:

```powershell
git clone https://github.com/peterkure3/ferrux.git
cd ferrux
cargo build --workspace --release --locked
.\target\release\ferrux.exe --version
.\target\release\ferrux.exe open
```

For an existing checkout, run `git pull --ff-only` in that checkout instead of
cloning again, then rebuild. Build the whole workspace so `ferrux.exe` and
`ferrux-daemon.exe` are updated together.

To replace an existing `%ProgramFiles%\Ferrux` installation with this build,
close its UI and daemon, then run this from the checkout in an Administrator
PowerShell window:

```powershell
.\scripts\update-local-install.ps1
```

The helper updates both installed executables and rolls back on replacement
failure. It refuses to replace binaries while installed Ferrux processes are
running; it does not stop active sessions for you.

If `ferrux` still opens an older copy, check which executable your shell finds:

```powershell
Get-Command ferrux -All | Select-Object CommandType, Source
& "$env:ProgramFiles\Ferrux\ferrux.exe" --version
```

Use `.\target\release\ferrux.exe` to run the source build directly. Unreleased
builds can share the published release's version number; rebuilding from the
updated checkout is what includes the latest source changes.

### Latest published release

Check the [GitHub releases page](https://github.com/peterkure3/ferrux/releases)
for the available tag, release notes, and Windows ZIP. This command installs
that published ZIP, which may not include the recent changes on `main`:

```powershell
irm https://raw.githubusercontent.com/peterkure3/ferrux/main/install.ps1 | iex
```

Installs to `%ProgramFiles%\Ferrux` and adds it to the system PATH.
Requires administrator privileges — you'll get a UAC prompt if the
shell isn't already elevated. Open a new terminal afterward and run:

```powershell
ferrux open
```

## Usage

- `ferrux open [name]` — open (or resume) a multi-pane workspace
- `ferrux new --shell <exe>` — spawn a standalone pane
- `ferrux ls` — list panes on the daemon
- `ferrux attach <id>` / `ferrux view <id>` — raw / rendered attach to a single pane
- `ferrux kill-pane <id>` — kill a pane
- `ferrux workspaces` — list saved workspaces

Run `ferrux update` to compare your installed version with the latest GitHub
release and install it if newer. If you are already up to date (or running a
newer local build), it exits without downloading or requesting administrator
access. Use `ferrux update --check` to check without installing, and
`ferrux --version` to see the installed version.

When an update is available, the installer opens a separate PowerShell window
and requests administrator access. Close Ferrux's UI and daemon before
installing; active sessions are left alone and block binary replacement.
Updates use the installer's Windows archive and require a versioned GitHub
release containing `ferrux-windows-x64.zip`. Pushing source changes alone does
not publish an update. `ferrux update` does not build from `main`, and an older
release may not include the updater command. Use the source-build instructions
above to get changes that have not yet been released.

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
hovered terminal or session list. Applications requesting mouse reporting or
alternate-screen scrolling receive wheel input within their pane. Hold Shift
while using the wheel to force Ferrux's local history. Shift+PageUp/PageDown
browses history; Shift+Home/End selects oldest history/live output. Ctrl+B then
PageUp/PageDown/Home/End provides equivalent local controls. Plain PageUp/PageDown
reaches the child. A `[SCROLL +N]` title indicates older output; typing returns
to live output. `Ctrl+S` continues to reach the child application;
saving uses `Ctrl+B` then `s`.

UI color can be overridden with `$env:FERRUX_COLOR_MODE='rgb'`, `'256'` or `'mono'`;
`NO_COLOR` also enables monochrome UI chrome. Explicit child ANSI colors remain
unchanged. Git metadata is read in the background. Unsupported approvals and
notification counts are shown as unavailable. See the
[UI implementation and validation report](docs/ui-validation/README.md).

Pane history retains up to 10,000 lines by default. Set
`$env:FERRUX_SCROLLBACK_LINES='25000'` before launching to change the limit
(maximum 100,000; zero disables history). History belongs to the attached UI and
is bounded, not a permanent conversation log. Scrolling also works in
`ferrux view <id>`. See [scrolling validation](docs/ui-validation/scrolling.md).

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

To update an existing Program Files installation from a local release build,
run `./scripts/update-local-install.ps1` in an Administrator PowerShell window.

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

# Storage audit

Measurements are logical file lengths in bytes on the Windows checkout, not compressed archive sizes or physical NTFS allocation. Hardlinked executables/PDBs may be counted twice. No historical cache was deleted to produce an improvement.

## Initial repository build artifacts

Measured before any compilation in this task. Total existing target: **8,272,548,498 bytes** (7.704411 GiB; 1 GiB = 1,073,741,824 bytes).

| Location | Bytes | Percentage of existing target |
|---|---:|---:|
| target/debug | 7,612,210,188 | 92.0177% |
| target/release | 660,191,817 | 7.9805% |
| Other target files / flycheck | 146,493 | 0.0018% |
| debug/deps (included in debug) | 4,024,979,798 | 48.6547% |
| debug/incremental (included in debug) | 3,319,145,044 | 40.1224% |
| debug/build (included in debug) | 97,907,767 | 1.1835% |
| release/deps (included in release) | 584,422,545 | 7.0646% |
| release/build (included in release) | 65,504,931 | 0.7918% |

Parent/detail rows overlap; do not sum them. The 47 files under crates contain **94,179 bytes**. Git metadata contains **194,845 bytes**. The full source inventory including Plan.md, lockfile, installer and existing user notes is small; source sizes in storage-before.json include audit tools and dependency reports created in this task, so they are not the original source baseline.

## Shared Cargo storage

Measured after Cargo unpacked the lockfile dependencies. These caches serve other projects too and cannot be attributed entirely to Ferrux.

| Path beneath C:\\Users\\Peter Kure\\.cargo | Bytes | % of measured repository + caches total |
|---|---:|---:|
| registry/src | 1,244,077,728 | 12.6592% |
| registry/cache | 224,596,670 | 2.2854% |
| registry/index | 83,444,469 | 0.8491% |
| git | 0 | 0% |
| Ferrux target | 8,272,548,498 | 84.1777% |
| Repository source, including audit reports | 2,618,573 | 0.0266% |
| .git | 194,845 | 0.0020% |
| **Total** | **9,827,480,783** | **100%** |

The installed MSVC Rust standard-library directory separately contains **123,115,649 bytes** at `C:\Program Files\Rust stable MSVC 1.99\lib\rustlib\x86_64-pc-windows-msvc\lib`. It is toolchain-wide, outside the total above, and must not be cleaned as a Ferrux cache. Standard-library machine code linked into executables is already included in executable lengths.

## Actual cause

The existing target is an accumulated development cache. Its debug fingerprints include 53 ferrux-ipc, 51 daemon, 50 PTY, 46 CLI, 40 TUI and 22 Tokio directories, across compilation modes/settings/history. Fingerprints do not prove every output is still live or identify the precise previous invocation. Multiple variants are directly observable: three debug Windows archives of 136,679,210, 136,678,850 and 136,678,674 bytes each, plus matching 92,258,546-byte metadata files. Several Tokio archives are about 38 MB each. Release also has a 106,805,918-byte Windows archive and 102,814,182-byte metadata file. These are compiler inputs/intermediates, not files to distribute.

An extension inventory taken during baseline compilation recorded 2,521,257,562 bytes of rlibs, 1,849,117,846 bytes of object files, 1,577,818,822 bytes of rmeta, 1,475,005,695 bytes of incremental bin data and 1,087,356,928 bytes of PDBs. This live inventory includes some newly created experiment files; it must not be summed as an immutable initial snapshot. `storage-before.json` similarly labels overlapping directory rows; its experiment detail was enumerated after its top-level target measurement. Stop builds before taking a fully consistent snapshot.

Existing debug outputs include stale `ferrux-cli.exe` (5,195,264 bytes), current-name `ferrux.exe` (5,200,896 bytes), and daemon (2,756,096 bytes), plus roughly 40 MB CLI and 34 MB daemon PDBs. Existing release executables are only **2,326,016 bytes** for ferrux and **1,163,776 bytes** for the daemon. Fresh reproducible results are in benchmarks.md; existing executable lengths alone cannot establish a before/after comparison on a different compiler.

Test harnesses share debug/deps, build scripts/proc macros populate build/deps and DLL/PDB outputs, and cargo check generates metadata variants. No separate benchmark history or dependency Git checkout was found. Build-cache growth is caused by keeping these products over many invocations, compounded by full dependency debug data and broad Windows API feature sets. It is not a 4 GB executable, source tree, or single dependency download.

## Sustainable management

Use `scripts/analyze-build-size.ps1 -IncludeGlobalCaches -JsonPath target/storage.json` with builds stopped. It reads global cache sizes only. Dev profiles reduce symbol volume while preserving workspace PDB diagnostics; CI should set CARGO_INCREMENTAL=0 because ephemeral builds cannot reuse local incremental state. Avoid switching compiler settings repeatedly in the normal target directory.

`scripts/clean-build-cache.ps1` previews incremental-only cleanup by default. `-Scope Debug`, `Release`, or `Experiments` selects other fixed repository-owned artifact trees. `-Delete -WhatIf` previews the destructive operation; real deletion requires typing DELETE, with additional ShouldProcess confirmation available through -Confirm. It refuses reparse points. Stop builds first. Global caches, Cargo.lock and source are never eligible. Cleanup remains optional and never runs on build or startup. Existing caches are not rewritten retroactively by new profile settings.

## Post-benchmark snapshot, before optional cleanup

`storage-after.json` was captured with builds stopped. Total repository plus shared Cargo caches: **18,244,964,503 bytes**. The historical debug and release directories are unchanged; this task did not erase them to manufacture a result.

| Category | Bytes | % of measured total |
|---|---:|---:|
| Entire target (parent row) | 16,692,329,948 | 91.4901% |
| Historical debug (included in target) | 7,612,210,188 | 41.7223% |
| Historical release (included in target) | 660,191,817 | 3.6185% |
| Isolated experiments (included in target) | 8,417,729,585 | 46.1373% |
| Release archives (included in target) | 2,041,767 | 0.0112% |
| Shared registry sources | 1,244,077,728 | 6.8187% |
| Shared registry archives | 224,596,670 | 1.2310% |
| Shared registry index | 83,444,469 | 0.4574% |
| Repository source/docs/scripts at capture | 320,843 | 0.0018% |
| Git metadata | 194,845 | 0.0011% |

Parent/detail rows overlap. The experiments intentionally retain original, no-LTO, thin-LTO, fat-LTO, size-oriented and final serial builds, plus test/check/example products. They are not the sustainable footprint of a single development build. Their optional cleanup can recover up to **8,417,729,585 logical bytes**, without deleting the saved measurements or target/dist release archives. Confirmation is required; the snapshot records their size before that optional action.

The controlled final fresh debug build is **659,851,367 bytes**, down **44.14%** from a fresh original build of **1,181,212,445 bytes**. Fresh release artifacts are **549,977,798 bytes**, down **19.30%** from **681,482,372 bytes**. Neither figure claims that the existing historical target shrank automatically. The installer ZIP is **1,334,728 bytes** and separate PDB ZIP is **707,039 bytes**.

## Approved experiment cleanup

After explicit user approval, the cleanup helper removed only `D:\Github\ferrux\target\performance` (8,417,729,585 logical bytes). It displayed the exact absolute path and expected recoverable bytes before the DELETE confirmation. The original debug/release caches, shared Cargo caches, source, lockfile, saved measurements and target/dist archives were preserved. This one-time removal of benchmark scratch directories is separate from the controlled build-profile storage reductions above. `storage-after-cleanup.json` records the final post-cleanup inventory.

The post-cleanup target contains **8,274,600,363 bytes**: the original historical cache plus **2,041,767 bytes** of release archives and **10,098 bytes** of storage-script fixtures. Repository plus shared caches totals **9,827,257,776 bytes** at capture. Shared registry source/archive/index lengths are unchanged from the initial audit. The performance experiment directory is absent. Historical artifacts remain eligible only for a separately selected, explicitly confirmed cleanup; they were not part of the approved benchmark-cache deletion.

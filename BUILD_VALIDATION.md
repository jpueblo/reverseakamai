# Cross-platform build validation — 2026-11-19

Branch: `main`.
Validated source commit: `ddcfb507380dab92c3ad1fb153937b26d09e0fa8` (merged PRs #1, #2 and #3).
This documentation-only update records validation of the source commit above.

Windows builds used the maintained fork directly, not a generated harness.
Ubuntu used an exact Git export of the same merged commit. The ARM64 build below
is a fresh cross-build of this merged source; it supersedes the older ARM64
result from the pre-merge build branch.

## Current target matrix

| Target | Release build | Execution and qualification |
| --- | --- | --- |
| Windows x64 / MSVC | Workspace, all targets: passed | **165 tests passed**, one private-source test ignored; CLI help passed |
| Ubuntu 24.04 x64 / GNU, under WSL2 | Workspace, all targets: passed | **165 tests passed**, one private-source test ignored; CLI help passed |
| Windows ARM64 / MSVC | Workspace, all targets: cross-compiled and linked from Windows x64 | CLI and SDK test executable verified ARM64; **native ARM64 execution remains pending** |

Each native x64 test run covers 144 codec-library tests, two CLI tests, 12 SDK
tests and seven vendored native-payload tests. The SDK tests include the bounded,
proxy-disabled numeric-localhost HTTP transport smoke test; native payload tests
include the Windows/macOS captured-profile regression. No ignored private-source
test was executed in these latest runs.

The two x64 release codecs also matched the saved baseline and each other on
all **38 frozen inputs** for family, shuffle key and version token. Expectations
remain unreviewed: output equality is compatibility evidence, not independent
proof of extraction correctness. Existing historical disagreements remain.

The ARM64 command compiles examples and test executables through `--all-targets`
but does not execute them. Compiled test binaries are not counted as passed
ARM64 tests. The SDK is a library; the verified SDK executable is its test harness,
not a standalone solver executable.

## Windows executable architecture verification

PE checks read the DOS `MZ` signature, `e_lfanew`, `PE` signature, COFF Machine
field and optional-header magic from the actual files, rather than inferring
architecture from a directory or filename.

| Artifact | COFF Machine | Optional-header magic | Result |
| --- | --- | --- | --- |
| Latest Windows x64 codec CLI | `0x8664` | `0x20b` / PE32+ | x64 |
| Latest Windows x64 SDK test executable | `0x8664` | `0x20b` / PE32+ | x64 |
| Latest Windows ARM64 codec CLI | `0xaa64` | `0x20b` / PE32+ | ARM64 |
| Latest Windows ARM64 SDK test executable | `0xaa64` | `0x20b` / PE32+ | ARM64 |

All **7 target executables** inspected under the ARM64 release root,
`deps` and `examples` identify ARM64/PE32+. Host build-tool outputs are separate
from these target artifacts. The x64 host's supplied `libclang.dll` was also
verified as x64 (`0x8664`), appropriate for the host process doing binding
generation while targeting ARM64.

ARM64 artifacts from this run:

| Artifact | File | SHA-256 |
| --- | --- | --- |
| Codec CLI | `akamai-codec.exe` | `6f2430eb6ef7d51795f2d0f27bbe1b3d6f158b9f6f6557b81faa10de24624282` |
| SDK test harness | `akamai_solver-14f37f88f043d160.exe` | `268f8e34e02f7606fc3437b416a13e543ffc1787b2bc202dc19fac760e1fa64d` |

These hashes identify retained local artifacts, not a byte-for-byte
reproducibility guarantee. No ARM64 CLI/help or test executable was launched.

## Reproduction and platform selection

With the prerequisites and environment described in [BUILDING.md](BUILDING.md):

```powershell
# Windows x64 native release build and tests:
./scripts/build.ps1 -Package workspace -Offline -Test

# Windows x64 host: ARM64 release cross-build, without target test execution:
./scripts/build.ps1 -Architecture arm64 -Package workspace -Offline
```

```bash
# Ubuntu 24.04 x64 native release build and tests:
bash scripts/build.sh --package workspace --offline --test
```

All commands use Cargo `--locked`; the recorded runs also used `--offline`.
Fresh target directories retained previous validation outputs. Rust 1.98.1,
the ARM64 Rust standard library and target-appropriate MSVC tools were already
installed. Existing dependency caches were reused. Local host-native LLVM and,
for the earlier x64 build, NASM paths were supplied. No new toolchain/dependency
downloads were required for this ARM64 cross-build.

The ARM64 run used `x64_arm64` compiler setup selected by the Windows helper.
Current workspace Cargo feature graphs were recorded for all three targets:
neither explicit `sha1/asm` nor `sha2/asm` is selected for either MSVC target;
Linux x64 selects both. `Cargo.lock` and all source files were unchanged by the
cross-build and feature inspection, before this documentation-only update.

The helper rejects cross-architecture `-Test`. Native ARM64 execution requires
a Windows ARM64 machine, its native Rust toolchain and host-compatible LLVM.
That future validation must run the tests; successful linking and PE-header
verification alone do not establish runtime correctness.

## Review fixes retained and historical results

The original build-only change was independently reviewed and its four
confirmed findings were fixed before merge:

- XP-01: reset accepted localhost sockets to blocking mode before read/write
  timeouts, accounting for Winsock's inherited listener mode.
- XP-02: discover both Git command and Unix-utility directories while retaining
  MSVC linker precedence.
- XP-03: filter Visual Studio discovery for the requested x64/ARM64 C++ toolset.
- XP-04: identify CLI and SDK-test PE architectures separately for each target.

Earlier build-branch validation reported 145 tests (124 codec library, two CLI,
12 SDK and seven native payload) and an ARM64 cross-build. Its original Ubuntu
run preceded those review fixes, and temporary Linux outputs later became
unavailable. **That Ubuntu follow-up gap is now closed** by the merged-main
release build, 165 passing tests and CLI-help check recorded above. The current
Linux source/toolchain/cache/target environment and outputs are retained in a
persistent validation directory. The older 145-test counts are historical, not
the latest merged-main result.

## Retained local evidence

Evidence is outside the repository under the local validation root; no binaries,
raw captures or build logs are committed:

- `merged-main-validation/20261118T195016Z/`: merged-main manifest, Windows
  and Ubuntu release build/test logs, CLI-help output, binary hashes, 38-input
  comparisons and summary/report.
- `arm64-main-validation/20261118T200027Z/`: ARM64 build log, pre-update source
  manifest, PE architecture/size/hash records, all three target feature graphs,
  the previous version of this document and summary/report.

The production/build source at the validated commit was not changed. This
update changes only the validation record; raw evidence remains outside the
repository.

## Remaining limitations and live-test state

- Native Windows ARM64 runtime/tests remain pending. No ARM64 emulation result
  or Windows ARM64 build-host validation is claimed.
- Docker validation remains pending; it was not performed in these latest
  runs. The earlier daemon was unavailable.
- LLVM/libclang must match the Rust build host. The DLL used here was checked,
  but the build helper does not proactively enforce that compatibility for
  every machine.
- Linux ARM64, musl, MinGW and macOS remain outside the build helpers' coverage.
  Toolchain/dependency pins do not pin OS packages/images sufficiently to
  guarantee reproducible binary bytes.
- The pre-existing codec unused-field warning remains. These are build/unit
  checks, not performance/memory benchmarks, TLS negotiation tests, live sensor
  acceptance or GraphQL validation; parser/model limitations remain unchanged.

Builds are manual. GitHub Actions remains intentionally absent. Cargo offline
mode is not an OS network sandbox. The earlier native transport runs used
numeric localhost with proxies disabled; the ARM64 step was compile/header
inspection only. No solver example, protected-site workflow, browser profile,
proxy test, sensor submission or captured-evidence replay was run.

**Live website, browser and proxy tests remain paused.**

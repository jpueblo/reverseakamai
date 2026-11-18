# Cross-platform build validation — 2026-11-18

Branch: `AD-11054/cross-platform-build`.
Base: `3f962f7521d685188009645c7a30240b593edca5` (`AD-11054/sdk-validation`).
Validation used the working tree of the fork directly, not the generated harness.

| Target | Build | Execution |
| --- | --- | --- |
| Windows x64 / MSVC | Release workspace, all targets: passed | 145 tests passed; one private-capture test ignored; CLI help passed |
| Ubuntu 24.04.3 x64 / GNU | Release workspace, all targets: passed under WSL | 145 tests passed; one private-capture test ignored |
| Windows ARM64 / MSVC | Release workspace, all targets: cross-compiled successfully from Windows x64 | Native ARM64 execution pending |

Each complete test run covers 124 codec library tests, two codec CLI tests,
12 SDK tests (including the new numeric-localhost transport smoke test), and
seven native-payload tests. SDK-only Windows package selection was also built
and its 12 tests passed. The SDK is a library; the build does not execute a
solver example or protected-site workflow.

## Platform selection checks

- Windows x64 target: both the codec CLI and SDK test executable PE headers
  identify x64 (`0x8664`).
- Windows ARM64 target: both the codec CLI and SDK test executable PE headers
  identify ARM64 (`0xaa64`).
- Cargo feature graphs contain neither explicit SHA assembly feature on either
  MSVC target; Linux x64 selects both `sha1/asm` and `sha2/asm`.
- The pinned `wreq`, `wreq-util` and `btls` cached source checkouts used for
  Windows validation have no tracked local modifications.
- Cargo.lock is unchanged. Both Windows builds used Cargo `--offline --locked`.
- Bash syntax and ShellCheck passed; PowerShell AST parsing passed.
- Git whitespace checks passed.

## Fixes and build configuration

The workspace no longer forces SHA assembly features globally. The vendored
native-payload manifest selects explicit assembly only on Linux x64, avoiding
Cargo feature unification re-enabling it on MSVC. Windows setup imports the
appropriate MSVC compiler environment. Git Unix utilities are appended to PATH
so their `link.exe` cannot shadow the Microsoft linker.

Both build scripts support codec, SDK and workspace package selection, release
or debug builds, and Cargo offline mode. The Windows script supports automatic
native architecture selection or explicit x64/ARM64 targets. Cross-target test
execution is rejected; native ARM64 testing requires a Windows ARM64 machine. Ubuntu
selection explicitly requires release 24.04 and x86_64.

Builds and validation are manual. The optional GitHub Actions workflow was
removed at the user's request. An optional Ubuntu 24.04 Docker build definition
uses the same source/scripts.

## Limits

Native Windows ARM64 tests still require a Windows ARM64 machine. The Docker definition was
not built locally: the Docker daemon was unavailable. Ubuntu validation was
performed in the existing Ubuntu 24.04 WSL environment, with build dependencies
installed and an isolated temporary Rust toolchain/cache/target directory. Those
temporary Linux outputs were unavailable after the validation session; the
subsequent Linux CLI-help check could not run. The recorded build and 145 test
results passed, but retained Linux artifacts and CLI-help smoke remain pending
manual checks.
Toolchain and Rust dependency versions are pinned, but OS packages/images are
not pinned sufficiently to claim byte-for-byte reproducible binaries.

The existing codec unused-field warning is unchanged. Passing build/unit checks
does not resolve previously observed captured-script parsing compatibility gaps.
No parser implementation, device telemetry or solver/submission/SBSD policy was
changed. The only added Rust runtime exercise is a proxy-disabled HTTP GET to a
numeric loopback server; it does not test TLS negotiation or external acceptance.
No live website/proxy/browser tests or sensor submissions were performed.
Dependency and toolchain downloads for Linux setup were permitted infrastructure
traffic, not site tests. Live site/proxy tests remain paused.

## Review fixes and offline follow-up

The four confirmed review findings are addressed:

- XP-01: reset accepted localhost sockets to blocking mode before applying read
  and write timeouts, because Winsock inherits the listener's nonblocking mode.
- XP-02: append both the standard Git command and Unix-utility directories,
  preserving MSVC linker precedence.
- XP-03: select Visual Studio installations with the C++ component required by
  the requested x64 or ARM64 target, instead of selecting any newest instance.
- XP-04: state each target's CLI/SDK-test PE architecture separately above.

Post-fix offline Windows validation passed all 145 tests and rebuilt the full
ARM64 workspace successfully. Git was removed from the incoming PATH; the
script discovered Git/cmd/git.exe, and link.exe still resolved to MSVC. Updated
CLI and SDK-test PE headers matched both target architectures. PowerShell AST,
new Rust test formatting and whitespace checks passed. Cargo.lock is unchanged.

The Ubuntu results in the table are from the original implementation before
these review fixes. Ubuntu was not rerun after the fixes because its temporary
Rust/cache directory was no longer available and this follow-up was offline.
Native Windows ARM64 execution and Docker validation remain pending. GitHub
Actions remains removed; all build validation is manual. No commits or pushes
were made during this follow-up.

# Building the fork

Build the checked-out fork directly. No generated harness or private capture is
required. Platform build scripts select the target automatically; package choice
is independent of OS/CPU. Solver/submission/SBSD behavior is unchanged.

| Build environment | Rust target | Entry point |
| --- | --- | --- |
| Windows x64, MSVC | `x86_64-pc-windows-msvc` | `scripts/build.ps1` |
| Windows ARM64, MSVC | `aarch64-pc-windows-msvc` | `scripts/build.ps1` |
| Ubuntu 24.04 x64, glibc | `x86_64-unknown-linux-gnu` | `scripts/build.sh` |

The matrix describes intended build coverage. Build and test results on each
platform establish whether a particular revision passes.
Linux ARM64, musl, MinGW and macOS are not covered by these scripts. Cargo itself
is not artificially restricted to these targets. Rust cfg cannot distinguish
Ubuntu releases; the Linux script checks `/etc/os-release`.

## Toolchain and dependency selection

Rust is pinned to `1.98.1` in `rust-toolchain.toml`; retain `Cargo.lock` and use
`--locked`. Install Rust using rustup, then run `rustup show` in the repository.
The selected target's standard library must be installed with `rustup target add`.
The native-payload source is vendored in this repository.

Workspace hashing does not force assembly features globally. The vendored hash
assembly backends are selected only for Linux x86_64. MSVC targets use the hash
crates' portable/default implementations. This prevents workspace feature
unification from re-enabling an unsupported assembly backend on Windows.

The SDK's `wreq`/`btls` dependencies compile native BoringSSL and generate bindings.
They require more tools than the codec. Windows ARM64 Rust support alone does
not establish compatibility of this pinned native dependency chain.

## Windows

Install Visual Studio 2022 or newer / Build Tools with C++ tools and a Windows
SDK. For ARM64, install the ARM64 C++ toolchain too. The script discovers Visual
Studio with `vswhere`, imports the appropriate compiler environment, and uses
MSVC on both architectures. Install rustup/cargo on PATH.

SDK/workspace builds additionally need CMake, Ninja, Git, Perl and host-native
LLVM with `libclang.dll`. NASM is required for x64 SDK builds, not for ARM64.
Visual Studio's bundled CMake/Ninja and standard LLVM/NASM/Git installation
folders are discovered automatically. Other installations can be supplied on
PATH and by setting `LIBCLANG_PATH` to the directory containing `libclang.dll`.
LLVM's DLL architecture must match the process loading it (the Rust build host).
Do not point an ARM64 Rust build process at an x64 libclang DLL.

```powershell
# Native target: x64 on Windows x64; ARM64 on Windows ARM64.
./scripts/build.ps1 -Package codec -Test
./scripts/build.ps1 -Package workspace -Test

# Explicit target. Install its Rust target and MSVC tools first.
rustup target add aarch64-pc-windows-msvc
./scripts/build.ps1 -Architecture arm64 -Package workspace
```

`-Test` requires native OS architecture. Cross-building Windows ARM64 from x64
is build-only and additionally depends on native-library cross-compilation
support. It is not a substitute for running tests on a Windows ARM64 machine.
Use `-DebugBuild` for debug output and `-Offline` for Cargo's offline mode.

## Ubuntu 24.04 x64

```bash
sudo apt-get update
sudo apt-get install --no-install-recommends -y \
  build-essential cmake ninja-build clang libclang-dev llvm-dev perl pkg-config git
# Install rustup and ensure ~/.cargo/bin is on PATH first.
rustup show
rustup target add x86_64-unknown-linux-gnu
bash scripts/build.sh --package codec --test
bash scripts/build.sh --package workspace --test
```

The script uses Ninja and discovers libclang through `llvm-config` unless
`LIBCLANG_PATH` is already set. Use `--package sdk` for the SDK alone, `--debug`
for debug output and `--offline` for Cargo's offline mode. The Ubuntu check
rejects other releases explicitly; use a 24.04 machine or container.

An optional container builds and tests from the same fork sources:

```bash
docker build --platform linux/amd64 -f Dockerfile.ubuntu24 -t reverseakamai-build .
# Codec-only build:
docker build --platform linux/amd64 -f Dockerfile.ubuntu24 \
  --build-arg PACKAGE=codec -t reverseakamai-codec-build .
```

This is a build environment, not a minimal runtime image. The SDK is a Rust
library, not a standalone solver executable. The default container command shows
codec CLI help; use the workspace build to include the CLI in the image.

## Validation and artifacts

Both scripts build all targets of the chosen package(s), including examples,
but never execute examples or invoke the solver. With `--test` / `-Test`, they
run library and binary unit tests. A native transport test uses only a numeric
localhost HTTP server, disables proxies and bounds connection/read/write time.
It checks native dependency linking and basic transport, not TLS negotiation or
external website behavior. Private full-capture tests remain ignored by default.

Outputs are under `target/<target>/release` (or `debug`), unless
`CARGO_TARGET_DIR` overrides the root. Build and test manually on each supported
platform. No GitHub Actions workflow is configured; executables remain local.
No proxy files, private captures, browser sessions or sensor submissions are used.

The first dependency/toolchain installation needs network access. Cargo
`--offline` prevents Cargo downloads; it is not an OS network sandbox and does
not constrain arbitrary native build scripts. Site/proxy live tests remain
separate and paused.

Current local results and remaining checks: [BUILD_VALIDATION.md](BUILD_VALIDATION.md).

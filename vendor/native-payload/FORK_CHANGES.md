# Vendored native-payload

Source: https://github.com/9xz/native-payload/tree/61ca5cc232f4e1584168077008d0f671ac6cbb98

Local changes: normalize captured Win32/Win64 profiles to the Windows OS field,
with a regression test preserving macOS behavior. Enable explicit assembly hash backends
only on Linux x86_64; keep MSVC x64/ARM64 on portable defaults. The workspace
manifest must not enable those features globally, because Cargo unifies them.

The generated homepage harness copies this dependency from the fork. Keep the
OS-field implementation here; do not patch it separately in the harness.

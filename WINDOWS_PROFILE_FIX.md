# Windows captured-device OS correction

Loading a Windows capture changed `DeviceProfile.platform` to `Win32` or
`Win64`, but left `DeviceProfile.os_platform` at the default `macOS`. Native
sensor construction reads that OS field for worker telemetry `s057`, producing
an inconsistent Windows/browser identity.

## Changes on this branch

| File | Change |
| --- | --- |
| `vendor/native-payload/src/profile.rs` | Set `os_platform` to `Windows` for captured `Win32` and `Win64` values. Other platform behavior is preserved. |
| Same file, inline unit tests | Check both Windows platform values and verify that `MacIntel` still produces `macOS`. |
| `Cargo.toml` and `Cargo.lock` | Use the maintained vendored dependency rather than fetching an unpatched Git dependency. Remove its Git source entry from the lockfile. |
| `vendor/native-payload/Cargo.toml` | Enable SHA assembly backends only on non-MSVC targets. This addresses the vendored dependency's MSVC build requirement; it does not change the root workspace's existing hash feature settings. |
| `.gitignore` | Ignore Visual Studio `.vs/` directories. |

The vendor snapshot comes from `9xz/native-payload` commit
`61ca5cc232f4e1584168077008d0f671ac6cbb98`. Its README and source are preserved;
`vendor/native-payload/FORK_CHANGES.md` records the local changes.

## Offline validation

- The new Windows/macOS regression passed against the fork dependency.
- The generated test harness was rebuilt with a byte-identical copy of the
  maintained `profile.rs` from this fork.
- Five Rust compatibility checks passed: Windows client hints and XHR headers;
  explicit transport selection; inconsistent identity rejection; worker
  telemetry rejection; and template construction from a saved HomeDepot script
  for Chrome 155, 156, and 157.
- Three Python executable/loopback tests passed, including six rejection cases
  before requests and the existing custom-template/session regression.
- `cargo metadata --offline --locked --no-deps` resolved the fork dependency
  graph. Git whitespace checks and the `.vs/` ignore check passed.

The compatibility tests ran in the Windows instrumented harness with its
existing build configuration. They do not establish a clean standalone build
of every workspace package on every platform, or live site acceptance.

## Related work and limits

Codec extraction changes are separate, on
[`AD-11054/homedepot-shuffle-key`](https://github.com/jpueblo/reverseakamai/tree/AD-11054/homedepot-shuffle-key).
Its `CODEC_PARSER_FIXES.md` documents shuffle-key, version-token, lexer,
constant-expression, ambiguity, and fixture changes.

Experimental Chrome 155+ configuration remains in the local test harness. The
current transport dependency only supplies profiles through Chrome 154.
Exact mode rejects newer versions before requests; the explicitly selected
`chrome154-approximation` policy is not verified Chrome 155+ transport support.
Unit fixtures with synthetic geometry are not complete captured environments
for live use. A complete matching environment and sensor template are still
needed for a future live comparison.

No challenge submission policy, SBSD guard, SDK session behavior, or live test
was changed by this branch. Live browser and proxy tests remain paused.

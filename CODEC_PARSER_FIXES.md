# HomeDepot codec parser fixes

Captured HomeDepot bmak builds changed constant-table layouts, symbol names,
wrapper arguments, and lookup expressions. Upstream extraction relied on fixed
table indexes and narrow string-table markers, so supported captures either
failed extraction or selected an unrelated numeric candidate.

These changes recognize the saved layouts statically. They do not execute
captured JavaScript or replace the challenge interpreter.

## Shuffle-key extraction

`script.rs` delegates extraction to `numeric_tables.rs` instead of assuming the
key is always `s8[10]` in a single array with fixed anchor positions.

- Recover a consistent digit mapping from numeric anchors, including
  `4294967295.000000`, `8388607.000000`, and `65535.000000`, across split pools.
- Prefer a decoded value bound to a supported cipher-key initializer. Registered
  getter-pool bindings take priority over unrelated sentinel candidates.
- Recognize supported repeated conditional lookups without treating arithmetic
  operands, negated branches, unary-helper inputs, or a pool value used as
  another array's index as the actual key.
- Support captured lazy-accessor layouts through a unique `8888888` sentinel
  pair when stronger initializer evidence is unavailable.
- Reject conflicting initializer, registered-pool, or sentinel candidates.
  Diagnostic analysis records the method and candidate sets rather than hiding
  ambiguity by returning the first number encountered.

`numeric_constants.rs` resolves only the supported pure integer expressions
used for pool indexes. It handles bounded references, unary operations, and
checked arithmetic. Unsupported expressions, invalid coercions, arithmetic
overflow, and unresolved values do not produce guessed constants.

## Version-token extraction

`extract_bmak_ver` first uses the constrained delta-string recognizer in
`delta_version.rs`, then falls back to supported XOR string tables.

- Recognize the captured numeric delta tables, seed-stack setup, unary-minus
  helpers, and supported wrapper/lookup argument layouts.
- Handle returned tables, renamed symbols, reordered wrapper arguments, and the
  captured four-argument lookup layout.
- Reject incorrect seeds, changed lookup semantics, unknown values, malformed
  or truncated tables, conflicting decoders, and excessive expression nesting.
- Generalize the XOR fallback beyond hard-coded `return ["` / `W2=["` markers,
  a fixed 20-byte seed, and a non-wrapping master window. Search supported
  seed/master offsets and rotations.
- Validate the 44-character Base64 candidate and padding bits; accept only a
  unique result. Multiple plausible candidates return no token.

## Literal scanning

`literal_arrays.rs` finds complete quoted-string array literals through a
constrained lexer. It skips comments, strings, template literals, and supported
regex literals, so decoy text is not treated as an executable constant pool.
Mixed-expression and incomplete arrays are rejected.

## Saved fixtures and offline verification

Eight minimized JavaScript fixtures cover captured layouts associated with
`436427`, `36bb`, split numeric pools, registered getter pools, returned delta
tables, the earlier delta layout, and the `217bb3` four-argument lookup. These
are parser fragments, not full traffic logs, cookies, or proxy configurations.

Regression cases cover successful extraction and negative mutations: wrong
seeds, unknown references, truncated arrays, decoy literals/comments/regexes,
conflicting keys, nested indexes, negation, and recursion limits. The original
shuffle-key snippet remains covered.

All 46 codec unit tests passed, including the new parser regressions.

Validation command: `cargo test --offline -p akamai-codec --lib`, using the
existing Windows instrumented harness. The parser source and fixtures are
checked against the branch sources before testing. This validates those saved
layouts, not every future script build or successful live sensor acceptance.
Numeric anchor bindings and the lazy sentinel fallback remain constrained
recognition rules rather than a general JavaScript interpreter.

## Branch separation

This branch contains codec extraction changes and parser fixtures. The Windows
`DeviceProfile.os_platform` correction, vendored dependency, MSVC dependency
feature adjustment, and `.vs/` ignore rule are on
[`codex/windows-device-os`](https://github.com/jpueblo/reverseakamai/tree/codex/windows-device-os),
documented in `WINDOWS_PROFILE_FIX.md`.

The SBSD guard, sensor submission policy, SDK request workflow, and interpreter
implementation are unchanged by these parser fixes. Live tests remain paused.

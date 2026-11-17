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

Ten minimized JavaScript fixtures cover captured layouts associated with
`436427`, `36bb`, split numeric pools, registered getter pools, returned delta
tables, the earlier delta layout, and the `217bb3` four-argument lookup. These
are parser fragments, not full traffic logs, cookies, or proxy configurations.

Regression cases cover successful extraction and negative mutations: wrong
seeds, unknown references, truncated arrays, decoy literals/comments/regexes,
conflicting keys, nested indexes, negation, and recursion limits. The original
shuffle-key snippet remains covered.

All 51 codec unit tests passed, including the review regressions below.

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
[`AD-11054/windows-device-os`](https://github.com/jpueblo/reverseakamai/tree/AD-11054/windows-device-os),
documented in `WINDOWS_PROFILE_FIX.md`.

The SBSD guard, sensor submission policy, SDK request workflow, and interpreter
implementation are unchanged by these parser fixes. Live tests remain paused.

## Copilot review follow-up

Both reported false positives were reproduced before correction. A shuffle
initializer is now accepted only when its complete expression has a supported
value-preserving shape: a pool lookup, a parseInt lookup (optionally radix 10),
parentheses, or a simple conditional whose supported branches produce the same
key. Arithmetic and unknown transforming calls are rejected; an unsupported
bound lookup cannot silently fall through to an unrelated sentinel.

Delta decoding now retains the decoder function identity, body bounds, seed
stack, and parameter positions for length, initial accumulator, and table index.
Only apply calls to that recognized function with the correct argument mapping
are decoded. Unrelated/member callees, reassigned decoders, conflicting function
names, and changed argument positions fail closed. This remains a constrained
recognizer and may reject new layouts until they have dedicated evidence/tests.

## Fresh Chrome155 registration and legacy loopback restoration

The fresh Chrome155 build uses a three-slot numeric registration header:
`[u32_max, LCG_multiplier, build_key]`. The earlier supported header was
`[u32_max, LCG_multiplier, mask, build_key]`. Both are recognized statically;
the mask is never selected as a build key. Registration evidence outranks an
unrelated sentinel, while malformed headers, conflicting registered values,
and a transformed initializer referencing the registered key fail closed.
The new 5d7 fixture and mutation tests cover this layout.

The historical 217bb3 loopback script has an opaque global conversion and
state-dispatcher version decoder outside the general recognizers. Its bounded
compatibility route requires exact SHA256
`217bb3efd6f16f02ef7bb2336f21593e6eb8af15ccdf59c1f764a75b1b9f8e29`.
The shuffle key is still read from the full LCG parameter sequence (multiplier,
32-bit mask, increment, 23-bit mask, sentinel, key), and the version is decoded
from its saved delta tables. Neither value is hard-coded. Any source-byte
change disables this route; unknown scripts retain the stricter initializer
and decoder-identity checks. This is intentionally exact-build compatibility,
not general support for every obfuscated global conversion.

Verification against the complete private captures finds the fresh key
8,682,175 and legacy key 4,881,225. The fresh statically extracted key decodes
and re-encodes both captured ciphertexts exactly, without recovery input.
The unchanged legacy loopback solve and fresh captured-template loopback both
pass, along with executable rejection tests. Mutated legacy scripts are rejected.
No captured full scripts, decoded sensor bodies, or environment files are added
to this branch.

# HomeDepot codec extraction and AST binding analysis

Captured HomeDepot bmak builds vary their constant pools, registrations and
version lookup layouts. Fixed indexes missed supported builds; token-based
scope checks could also associate an initializer with a different variable or
decoder. Extraction now uses a parsed JavaScript AST and resolved binding IDs
for the general route, alongside the constrained format recognizers.

## Binding analysis

- Pin Oxc parser/semantic crates to 0.156.0. Parse classic JavaScript with syntax
  checking, then use semantic identifier references, declaration identities and
  write flags. Invalid/recovered syntax and dynamic `with` scopes reject.
- Resolve each occurrence independently. Function, arrow, method, class,
  catch, rest/destructured/default parameters and block declarations do not
  accidentally reuse an outer variable's constant. Unrelated bindings with the
  same spelling no longer invalidate each other.
- Detect plain/compound/logical/update, destructuring and loop-target writes
  through AST targets. Track member writes/deletes and array/function escapes
  separately from scalar assignment. Pools and key-pair variables must remain
  stable; pool aliases and unsupported mutations cannot supply a proven key.
- Recognize visible classic-script global-var property writes through
  `window`, `globalThis`, `self` and `this`, including literal escaped property
  names. These do not alias local IIFE variables. Generic dynamic alias and
  control-flow interpretation, including values passed through throw/catch,
  are not implemented. Local binding provenance is rejection evidence only.
- Require the actual unresolved builtin at the call site for `parseInt` and
  `String.fromCharCode`, with no visible replacement or constructor escape.
  Prototype/member mutations invalidate the corresponding dependency proof.
- Conservatively invalidate affected outer references to sloppy-mode block
  function declarations (Annex B); strict-mode block bindings stay independent.
- Read all variable declarators, including comma lists, `let` and `const`, from
  AST initializer spans. Require direct initialization and lexical availability;
  unsupported known-pool transformations cannot fall back to a sentinel.
  Unsupported assignment RHS values (including implicit globals and destructured
  writes) and negated pool sources also block weak sentinel fallback. Stronger
  independently proven bound/registered candidates retain their precedence.
  Pattern evidence includes defaults inside the pattern, rather than just its
  outer RHS. Loop targets retain the complete head input/default spans, and
  parameter defaults are retained as unsupported key-source evidence too.
- Propagate rejection evidence through local binding dependencies, including
  pool aliases and intermediate scalar values. A cycle-safe worklist reaches
  all pool-derived bindings; this does not accept new alias layouts. Dependency
  collection is capped at two million reference visits and rejects on exhaustion.
- Retain the second element of every two-element AST array as weak-fallback
  rejection evidence, including inline calls, unary/binary/sequence wrappers,
  spread values and complex first-element expressions. These forms are not
  newly accepted as key layouts; stronger proven strategies retain precedence.
- Build pool ownership/provenance and a sorted pool-derived read-position index
  once, outside digit-map iteration. Evidence-span queries use binary search
  rather than rescanning all reads inside nested expressions.
- Prepare skip/read/poison/binding facts and compact lookup plans once per
  script. Temporary RHS tokens are discarded; plans retain only encoded pool
  leaves and conditional-equality nodes. A shared two-million source-byte/token
  work allowance bounds preparation and rejects without partial results.
  Repeated leaves use an identity-keyed decoded-value cache local to each map.
  Annex B poisoned positions have a sorted index too; parenthesized lookup
  matching pairs delimiters linearly during planning. With no digit map,
  initializer preparation is skipped. Only decoding plans repeats per map.
- Deduplicate anchor strings and intermediate digit maps before combining them;
  infer the missing one glyph once per final map. The search has a 25-million
  glyph-slot/map-copy work allowance and a 256-map limit per phase. Exhaustion
  rejects the result without returning a partial candidate.
- Evaluate only bounded integer expressions and synchronous scalar unary
  helpers. Preserve Boolean/Number/Object/Array distinctions and require
  Numbers for indexes, deltas, lengths and seeds. Reject unsupported literals,
  coercions and arithmetic outside the exact safe-integer range. Input size is
  capped at 8 MB, arithmetic recursion at 32, and array length at 100,000.

The old general declaration/parameter/control-header/property-write scanners
and numeric constant module are removed. AST facts are local to an extraction;
there is no ambient source cache. `analyze_script` shares a single analysis for
both values, and `BuildTemplate::from_bmak_script` already uses that combined
path. Calling the two standalone extraction functions performs two analyses.
The SDK cache lookup now calls `source_sha256`, which hashes without parsing;
it no longer performs a full extraction just to obtain the source identity.

The literal/table lexer and exact supported decoder-body matcher remain format
recognizers; they no longer infer general JavaScript scopes. Recognition does
not execute the script or claim to interpret arbitrary callbacks and aliases.
String pools and registration calls are discovered from actual AST arrays and
call arguments, so inert nested-template text cannot become a registration.

## Shuffle-key layouts

- Recover consistent digit mappings from the 32-bit, 23-bit and 16-bit numeric
  anchors, including split pools.
- Support complete value-preserving key initializers: a bound lookup,
  `parseInt` with optional radix 10, parentheses, and supported equal-valued
  conditional branches. Unknown transformations and arithmetic are rejected.
- Match observed registered getter-pool headers in both forms:
  `[u32_max, multiplier, mask, key]` and `[u32_max, multiplier, key]`.
  The mask is never selected as a key.
- Use a unique `8888888` sentinel pair for the constrained lazy-accessor layout
  only when stronger evidence is unavailable. Conflicting registrations are
  rejected before any bound initializer can override them. Single registered
  and bound candidates must agree; a registration cannot rescue a transformed
  operand referring to its key.

## Version layouts

- Bind numeric delta tables, factories, decoder functions and apply calls by
  semantic identity. A returned table requires the supported direct switch-case
  return shape and a call to that same dispatcher. Unrelated factories and
  transformed return/call suffixes reject.
- Stream nested function spans to locate a returned array's dispatcher, require
  its direct switch/case prefix and index factory calls by resolved callee ID.
  Unrelated sibling functions and returned arrays no longer trigger repeated
  whole-function or whole-token scans.
- Share parsed delta-table data across factory owners and decoder candidates.
  Cache function-escape facts, index candidates by resolved callee and bound
  candidate matching to two million body-token work units; exhaustion rejects
  the general version result without partial acceptance.
  Sorted per-binding reference indexes preserve outside-read/unique-push rules
  and locate each apply call without repeated full reference-list scans.
- Require the complete supported accumulator/output/return/index-progression
  body, correct argument positions, stable helper/table identities, and one
  numeric seed push in the same AST statement container before the apply call.
  The push must be a direct statement, without an intervening conditional,
  logical expression or loop; stack and table initialization must precede use.
  Replaced decoders, async/generator helpers, changed output expressions and
  extra stack operations reject.
- For wrappers with global numeric data, require an earlier numeric initializer
  and permit only the recognized decoder's own table-selection write.
- Preserve the constrained XOR-table fallback with a unique canonical
  44-character Base64 token. AST syntax/dynamic-scope rejection happens before
  this general fallback too.
- Precompute XOR candidate sets and cap search at 25 million work units (each
  candidate comparison and decoded byte consumes one). Exhaustion rejects the
  entire result, including a token found before uniqueness was established.
  The fresh full saved capture is independently verified to use this route.

## Exact-source legacy compatibility

Conservative fallback rejection can reject a sentinel-only lazy layout if any
two-element array reads a pool-derived value, including a string-table anchor
or an unproven negated seed. The minimized lazy fixture passes, but neither
full saved build exercises this fallback route: the fresh build uses
registration and the legacy build uses exact-source compatibility. Dynamic
array construction via push/member writes/Array.of is outside the literal-pair
evidence model. Parser/semantic traversal depth is not bounded by the separate
two-million dependency-read visit budget.

One previously verified full script has opaque conversion and dispatcher
layouts outside the general recognizers. Its compatibility route requires
exact SHA256:

`217bb3efd6f16f02ef7bb2336f21593e6eb8af15ccdf59c1f764a75b1b9f8e29`

It still computes the key from the full LCG signature and the version from
numeric delta tables; neither result is cached or hard-coded. Its previously
verified dispatcher-depth arithmetic and seed handling stay isolated from the
AST route. Any source-byte change disables this exception. This does not grant
unknown scripts the historical name-based proof.

## Offline validation

All 85 existing codec tests are retained, with thirty-nine additional AST precision,
binding and bounded-search tests (124 passing, plus one private-source test
ignored by default and executed separately). Positive cases cover independent same-name bindings,
blocks, IIFEs and escaped identifiers. Negative cases cover the accumulated
Copilot/Claude mutations, malformed syntax, escaped builtin writes and invalid
helper kinds. Additional cases verify initializer ordering, function hoisting,
complete seed argument expressions, declaration forms, Annex B, template decoys,
conditional pushes, bounded XOR success/exhaustion and actual AST arrays for key-pair evidence. A synthetic version-table fixture was wrapped in a function to
make its formerly top-level `return` valid JavaScript.

Complete private saved scripts retain fresh key 8,682,175 and legacy key
4,881,225 and the expected versions; modified legacy sources reject. Four
executable/localhost integration tests validate captured-template construction,
identity headers, source-template preservation and the unchanged SBSD guard.
The local Windows harness uses the separate platform build fixes; codec sources
are synchronized from this branch. Dependencies are cached before offline
checks. No raw traffic, full scripts, environment dumps or credentials are
committed. Saved-build success does not establish live sensor acceptance,
GraphQL access, or support for every rotated script build.

## Performance comparison

See [recorded results](crates/codec/benchmarks/AD-11054-ast-scope.json). The baseline is
the preserved 85-test pre-AST working tree, based on b1e4621 plus local review
fixes, not untouched upstream. Baseline and updated code were linked into one
release executable with the same Rust 1.98.1/MSVC target, native dependency and
SHA features. Execution order alternated for each sample. Outputs were asserted
identical, including full version strings. Two full captures used 15 samples
per mode after warm-up; four minimized fixtures used 200.

| Full saved script / mode | Baseline median | AST median | Change |
| --- | ---: | ---: | ---: |
| fresh_full / key_then_version | 126.90 ms | 151.16 ms | +19.1% |
| fresh_full / analyze_script | 133.00 ms | 102.97 ms | -22.6% |
| fresh_full / sdk_hash_lookup | 131.44 ms | 0.26 ms | -99.8% |
| legacy_full / key_then_version | 123.73 ms | 64.17 ms | -48.1% |
| legacy_full / analyze_script | 118.78 ms | 65.32 ms | -45.0% |
| legacy_full / sdk_hash_lookup | 123.19 ms | 0.27 ms | -99.8% |

The existing template-construction path uses `analyze_script`: both full scripts
improved in this run. Separate key/version calls repeat AST work and have the
additional cost shown above on the fresh full script. Hash-only lookup now
avoids analysis entirely; its comparison is the previous SDK caller expression
versus `source_sha256`, not a faster hashing algorithm. These are local
latency measurements, not a production guarantee or memory benchmark. Early
debug measurements were also retained locally and showed larger AST overhead;
optimized results are the primary comparison.

The separate `span_index_comparison` in the same JSON preserves the AST version
before the read-position, anchor-search, dispatcher and initializer optimizations. With identical release settings and
outputs, nested-pair cases (depth 48 / 8,192 unrelated reads; depth 96 / 32,768
reads) use 15 alternating samples per mode. Pool-derived reads are indexed once;
each evidence span now uses a binary search instead of repeating its contained
reads, and pool facts are shared across digit maps. These bounded cases do not
test parser behavior at arbitrary nesting depths.
An additional Annex B case combines 8,192 poisoned references with 512 repeated
key-pair initializers, checking the final poison index/prepared-fact behavior
against that preserved AST implementation with the same output assertions.

For a single-checkout probe, use `crates/codec/examples/bench_script.rs` with a
local JSON manifest of `{label, path, iterations}` entries and mode `report` or
`pair`. Copy that same example into a baseline checkout when reproducing a
comparison; build settings must match. The private capture manifest and the
duplicated baseline source are deliberately not committed.

## Separate work and limitations

The Windows `DeviceProfile.os_platform` fix and native dependency/build changes
remain separate in PR #1 / `AD-11054/windows-device-os`. This branch includes the
`.vs/` ignore rule. The SDK hash-only cache lookup is the only SDK source change;
request workflow, interpreter and SBSD policy are unchanged. Exact Chrome155 transport and the native fallback memory-field
discrepancy are separate unresolved limitations. Live browser/proxy tests remain
paused. Independent review reports and per-finding resolutions are retained
separately; publication remains subject to a clean verification.

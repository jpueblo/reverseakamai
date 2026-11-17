# AD-11054 SDK integration validation

This branch combines draft PR #1 (Windows device OS correction) and draft PR #2
(HomeDepot key/version extraction), including the Copilot follow-up fixes.
The independent implementation details remain in WINDOWS_PROFILE_FIX.md and
CODEC_PARSER_FIXES.md. The SDK and SBSD submission guard are unchanged.

## Complete Chrome155 Windows11 capture

A fresh Kameleo profile loaded the homepage once with a 30-second passive
observation window. Three direct browser snapshots recorded the complete
required environment, including colorDepth 24, pixelDepth 24, devicePixelRatio 1,
outerHeight 1440, and platformVersion 19.0.0. This supersedes the earlier partial
environment preparation; no synthetic geometry was supplied.

The matching captured script SHA256 is
5d7b4b2851c18a689d1a72085d95590167f3312b151acebab778ac9b0a3e2c29.
Two captured sensor stages agree with the directly observed UA, client hints,
geometry, language, timezone, CPU count, memory, and WebGL. The mature stage
contains 28 worker fields. Both ciphertexts decode and re-encode exactly, and
three offline template encodings round-trip successfully. Rust profile/template
validation passes with no network requests. Captured clocks and measurements
remain historical data, not freshly synthesized browser telemetry.

## Static extraction and loopback restoration

Static extraction now supports the fresh Chrome155 registration ABI
[u32_max, multiplier, key] as well as [u32_max, multiplier, mask, key]. The fresh
key is selected from the registration rather than an unrelated sentinel.
Malformed/conflicting registrations and transformed key operands fail closed.
Both captured ciphertexts round-trip with static extraction only; recovery input
is no longer required. The fresh captured-template SDK loopback solve passes.

The unchanged original legacy loopback test is restored through exact-build
compatibility for SHA256
217bb3efd6f16f02ef7bb2336f21593e6eb8af15ccdf59c1f764a75b1b9f8e29.
Its key and version are computed from their parameter/delta tables, not cached.
The opaque conversion/dispatcher layout is deliberately not generalized to
unknown scripts. Any source-byte change disables that compatibility route.

All 51 codec tests and four Python executable/loopback tests pass. Full fresh
and legacy saved scripts were checked, including mutated legacy rejection.
Copilot regressions and the SBSD guard remain intact.

## Remaining limits

Direct measurement shows deviceMemory 16 and captured worker s035 16. The
native fallback constructor currently multiplies device memory by four; that
separate telemetry issue is not corrected by the shuffle extraction fix.

Exact Chrome155+ TLS/HTTP2 profiles are still unavailable in the pinned transport
library. Chrome154 approximation remains an explicit, experimental local-harness
option. No live SDK solve is authorized by this validation.

## Capture results and evidence handling

The homepage and six automatic GraphQL responses returned HTTP200. All six
GraphQL response bodies were retained. Browser traffic nevertheless recorded
98 tunnel failures and nine capture gaps; healthy before/after probes and a
consistent exit hash do not negate those errors. A prior setup attempt returned
HTTP472 node_tunnel_failed before creating any browser; it is preserved separately.

The one browser profile was stopped/deleted and temporary proxy credentials
removed. Raw environment, traffic, decoded sensor posts and template files remain
in restricted local evidence storage, outside Git. The local harness pointer
chrome155-saved-template-current.json identifies the current private files and
redacted reports. The earlier Chrome154 template pointer is preserved.

Live SDK tests remain paused. The SBSD guard is unchanged. Both Copilot comments
have reproduction/fix/test replies on PR #2. The .vs/ ignore rule is present on
all three AD-11054 branches.

## Validation incident and offline network gate

A stale installed negative test unexpectedly made one HomeDepot SDK GET, which
returned HTTP403; it submitted no sensor posts. This violated the SDK pause and
is preserved in the private validation evidence. The installed test was
corrected and output directories made unique. The generated SDK adapter now
denies external network traffic by default, with a regression verifying rejection
before request logs. Subsequent checks were localhost-only. Live SDK tests remain
paused; future live use requires an explicit network-gate change and authorization.

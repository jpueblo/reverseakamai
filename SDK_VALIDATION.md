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

## Outstanding blockers

The template uses a key recovered offline from captured framing and verified
against both ciphertexts. Static extraction does not yet support this script's
layout. Before review hardening the parser selected an unrelated sentinel;
it now rejects the layout. This is not reported as static extractor support.

All 48 codec unit tests pass, including the two Copilot regressions. The stricter
complete-initializer rule also rejects an older loopback fixture's complex
initializer. That legacy SDK solve fails closed with a missing shuffle key;
it is not reported as passing. Additional evidence-backed recognition is needed.

Direct measurement also shows deviceMemory 16 and captured worker s035 16.
The earlier assumption of dividing s035 by four was wrong for this capture.
The native fallback template constructor currently multiplies memory by four;
its telemetry is not validated by successful replay of the captured template.

Exact Chrome155+ TLS/HTTP2 profiles are still unavailable in the pinned transport
library. Chrome154 approximation remains an explicit, experimental local-harness
option. No live SDK solve is ready or authorized by this validation.

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

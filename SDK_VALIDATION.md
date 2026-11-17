# AD-11054 SDK integration validation

This integration branch combines two independent draft PRs without merging
either into main:

- [Windows device OS correction, PR #1](https://github.com/jpueblo/reverseakamai/pull/1)
- [HomeDepot codec parser fixes, PR #2](https://github.com/jpueblo/reverseakamai/pull/2)

`WINDOWS_PROFILE_FIX.md` and `CODEC_PARSER_FIXES.md` document their individual
changes. Experimental Chrome155+ transport selection remains in the local
instrumented harness; no exact Chrome155 transport preset is introduced here.

## Saved Chrome155 Windows11 preparation

The source is the second captured Chrome155 browser session from the saved
homepage pair. It contains one matching bmak script and two sensor POST bodies.
The decoded mature stage has 28 worker fields. Runtime identity, outgoing
headers, worker UA, low/high-entropy client hints, and both sensor stages agree
on Chrome155, Windows/Win32, and platformVersion 19.0.0.

The private template is bound to script SHA256
`d30fd6d7aa173a0991be490f904fa59fdd363257a28b507306b8570f81a53fc3`.
Both captured ciphertexts decode and re-encode exactly. Three offline encodings
round-trip successfully; the third SDK post reuses the last captured stage.
Captured measurements, clocks, and non-build fields are preserved. This does
not refresh browser telemetry or establish live acceptance.

The environment JSON uses values supported by saved evidence: runtime UA and
client hints; sensor screen/available-screen dimensions and inner dimensions/
outer width; worker language, timezone, CPU count, memory, and WebGL identity.
The provenance record documents each source and marks unknown fields explicitly.
No synthetic geometry is added to this capture-backed environment.

### Incomplete environment: live tests remain paused

The captures do not contain a complete capture-env snapshot. These fields
remain unverified and are omitted:

- `screen.colorDepth`
- `screen.pixelDepth`
- `screen.devicePixelRatio`
- `window.outerHeight`

The environment therefore has `captureCompleteness.complete=false` and is not
live-ready. The harness rejects it before live requests. Numeric loopback-only
preview is allowed so the unchanged captured template can be validated without
claiming a complete native browser environment. Native fallback defaults are
not evidence for the missing fields. A complete future capture would be needed
before using this environment for live work; none was requested or run here.

Raw sensor bodies, decoded templates, environment JSON, cookies, proxy details,
and traffic logs stay outside Git in restricted local evidence storage. The
local harness pointer is `chrome155-saved-template-current.json`; it identifies
the private files and redacted reports. The earlier Chrome154 template pointer
is preserved.

## Offline checks

- The two saved ciphertexts and three encoded posts round-trip exactly.
- The actual SDK loopback solve submits three sensors, reuses the last captured
  stage, preserves the source template, and skips background SBSD under the
  unchanged guard. UA and language match the saved sensor request headers.
- An incomplete environment is rejected before any live request or traffic log.
- Exact Chrome155+ transport selection is rejected. Loopback preview explicitly
  uses `chrome154-approximation`, without asserting transport equivalence.
- The existing Windows/macOS dependency regression, codec suite, compatibility
  checks, and executable/loopback regressions are retained and run offline.

There are no HomeDepot/proxy requests or browser launches in this validation.
GitHub operations only create the requested draft PRs and integration branch.

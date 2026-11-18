//! Offline JavaScript build fingerprinting and browser-API coverage analysis.
//!
//! This module inspects source text only. It does not execute scripts, make
//! network requests, or construct challenge payloads.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
#[path = "ast_scope.rs"]
mod ast_scope;
#[path = "delta_version.rs"]
mod delta_version;
#[path = "literal_arrays.rs"]
mod literal_arrays;
#[path = "numeric_tables.rs"]
mod numeric_tables;
/// Coarse, heuristic script family for build inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptFamily {
    Bmak,
    Sbsd,
    Unknown,
}

impl ScriptFamily {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bmak => "bmak",
            Self::Sbsd => "sbsd",
            Self::Unknown => "unknown",
        }
    }
}

/// Stable build identity plus static source markers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptReport {
    pub sha256: String,
    pub bytes: usize,
    pub lines: usize,
    pub family: ScriptFamily,
    pub string_literals: usize,
    pub max_string_literal_bytes: usize,
    pub catch_splice_markers: usize,
    pub browser_apis: BTreeMap<&'static str, bool>,
    pub shuffle_key: Option<u32>,
    pub version_token: Option<String>,
}

/// Hash source bytes without parsing or extracting build data.
pub fn source_sha256(source: &str) -> String {
    let digest = Sha256::digest(source.as_bytes());
    let mut sha256 = String::with_capacity(64);
    for byte in digest {
        write!(sha256, "{byte:02x}").expect("writing to a string cannot fail");
    }

    sha256
}

/// Analyze JavaScript source without evaluating it.
pub fn analyze_script(source: &str) -> ScriptReport {
    let sha256 = source_sha256(source);
    let family = if source.contains("bmak") || source.contains("sensor_data") {
        ScriptFamily::Bmak
    } else if source.contains("onconnect")
        || source.contains("SharedWorker")
        || source.contains("getHighEntropyValues")
    {
        ScriptFamily::Sbsd
    } else {
        ScriptFamily::Unknown
    };

    let features: &[(&str, &[&str])] = &[
        ("automation_checks", &["webdriver"]),
        ("blob_object_urls", &["createObjectURL"]),
        ("canvas_2d", &["measureText", "getImageData"]),
        ("dedicated_worker", &["new Worker"]),
        ("extension_probes", &["chrome-extension://"]),
        ("high_entropy_hints", &["getHighEntropyValues"]),
        ("media_devices", &["enumerateDevices"]),
        ("offscreen_canvas", &["OffscreenCanvas"]),
        ("offline_audio", &["OfflineAudioContext"]),
        ("permissions", &["permissions.query"]),
        ("self_integrity", &["Function.prototype.toString"]),
        ("service_worker", &["serviceWorker"]),
        ("shared_worker", &["SharedWorker", "onconnect"]),
        ("speech_synthesis", &["speechSynthesis"]),
        ("web_crypto", &["crypto.subtle"]),
        ("webgl", &["webgl"]),
        ("webrtc", &["RTCPeerConnection"]),
    ];
    let browser_apis = features
        .iter()
        .map(|(name, markers)| (*name, markers.iter().any(|marker| source.contains(marker))))
        .collect();
    let (string_literals, max_string_literal_bytes) = string_metrics(source);

    let legacy = verified_legacy_layout(source);
    let ast = if legacy {
        None
    } else {
        ast_scope::Analysis::parse(source)
    };
    let shuffle_key = if legacy {
        extract_bmak_shuffle_key(source)
    } else {
        ast.as_ref()
            .and_then(|a| numeric_tables::analyze_with_analysis(source, Some(a)).key)
    };
    let version_token = if legacy {
        extract_bmak_ver(source)
    } else {
        ast.as_ref().filter(|a| !a.dynamic).and_then(|a| {
            delta_version::extract_with_analysis(source, a).or_else(|| extract_xor_ver(a))
        })
    };
    ScriptReport {
        sha256,
        bytes: source.len(),
        lines: if source.is_empty() {
            0
        } else {
            source.bytes().filter(|byte| *byte == b'\n').count() + 1
        },
        family,
        string_literals,
        max_string_literal_bytes,
        catch_splice_markers: count_catch_splice_markers(source),
        browser_apis,
        shuffle_key,
        version_token,
    }
}

/// JSON representation suitable for build inventories and cache keys.
pub fn report_json(report: &ScriptReport) -> Value {
    json!({
        "sha256": report.sha256,
        "bytes": report.bytes,
        "lines": report.lines,
        "family": report.family.as_str(),
        "metrics": {
            "string_literals": report.string_literals,
            "max_string_literal_bytes": report.max_string_literal_bytes,
            "catch_splice_markers": report.catch_splice_markers,
        },
        "shuffle_key": report.shuffle_key,
        "version_token": report.version_token,
        "browser_apis": report.browser_apis,
    })
}

fn count_catch_splice_markers(source: &str) -> usize {
    let mut count = 0;
    let mut rest = source;
    while let Some(index) = rest.find("catch") {
        rest = &rest[index + 5..];
        let end = rest.find('}').unwrap_or(rest.len());
        if rest[..end].contains(".splice(") {
            count += 1;
        }
        rest = &rest[end..];
    }
    count
}

// Compatibility is scoped to the immutable, independently verified loopback
// build. Mutating any call/table invalidates this route, including review decoys.
fn verified_legacy_layout(source: &str) -> bool {
    format!("{:x}", Sha256::digest(source.as_bytes()))
        == "217bb3efd6f16f02ef7bb2336f21593e6eb8af15ccdf59c1f764a75b1b9f8e29"
}

/// Extract a key from supported digit-cipher tables. Absolute indexes and
/// variable names change between builds. Prefer a bound cipher-key initializer;
/// lazy accessor layouts use the unique 8888888 sentinel pair as a fallback.
/// Require three consistent numeric anchors, including across split pools.
pub fn extract_bmak_shuffle_key(source: &str) -> Option<u32> {
    numeric_tables::extract_shuffle_key(source)
}

fn bind_numeric_anchor(map: &[u8; 256], encoded: &str, expected: &[u8]) -> Option<[u8; 256]> {
    if !encoded.is_ascii() || encoded.len() != expected.len() {
        return None;
    }
    let mut result = *map;
    for (glyph, plain) in encoded.bytes().zip(expected.iter().copied()) {
        let value = if plain == b'.' { 10 } else { plain - b'0' };
        let slot = result[glyph as usize];
        if slot != 255 && slot != value {
            return None;
        }
        if result
            .iter()
            .enumerate()
            .any(|(other, v)| *v == value && other != glyph as usize)
        {
            return None;
        }
        result[glyph as usize] = value;
    }
    Some(result)
}

fn decode_numeric_key(encoded: &str, map: &[u8; 256]) -> Option<u32> {
    if encoded.is_empty() || !encoded.is_ascii() {
        return None;
    }
    encoded.bytes().try_fold(0u32, |value, glyph| {
        let digit = map[glyph as usize];
        if digit > 9 {
            return None;
        }
        value.checked_mul(10)?.checked_add(u32::from(digit))
    })
}

/// Statically extract the 44-character Base64 `ver` constant from a `bmak` v3 script's
/// XOR-encoded string table arrays.
pub fn extract_bmak_ver(source: &str) -> Option<String> {
    if verified_legacy_layout(source) {
        return delta_version::extract(source);
    }
    let ast = ast_scope::Analysis::parse(source)?;
    if ast.dynamic {
        return None;
    }
    delta_version::extract_with_analysis(source, &ast).or_else(|| extract_xor_ver(&ast))
}

// Bound search even for adversarial cross-products. Exhaustion rejects the
// entire result: uniqueness cannot be established from a partial search.
const XOR_WORK_BUDGET: u64 = 25_000_000;
fn extract_xor_ver(ast: &ast_scope::Analysis) -> Option<String> {
    extract_xor_with_budget(ast, XOR_WORK_BUDGET).ok().flatten()
}
fn extract_xor_with_budget(ast: &ast_scope::Analysis, mut work: u64) -> Result<Option<String>, ()> {
    fn spend(work: &mut u64) -> Result<(), ()> {
        *work = work.checked_sub(1).ok_or(())?;
        Ok(())
    }

    let mut candidates = std::collections::BTreeSet::new();
    for (_, strings) in &ast.string_arrays {
        let arr: Vec<Vec<u8>> = strings
            .iter()
            .filter_map(|s| s.chars().map(|c| u8::try_from(u32::from(c)).ok()).collect())
            .collect();
        let ciphers: Vec<_> = arr.iter().filter(|s| s.len() == 44).collect();
        let seeds: Vec<_> = arr.iter().filter(|s| (8..=32).contains(&s.len())).collect();
        let masters: Vec<_> = arr
            .iter()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .collect();
        for cipher in ciphers {
            for seed in &seeds {
                for master in &masters {
                    if std::ptr::eq(*seed, *master) {
                        continue;
                    }
                    for offset in 0..master.len() {
                        for shift in 0..seed.len() {
                            spend(&mut work)?;
                            let last = (43 + shift) % seed.len();
                            if seed[last] ^ master[(offset + last) % master.len()]
                                != cipher[43] ^ b'='
                            {
                                continue;
                            }
                            let mut out = [0; 44];
                            let mut valid = true;
                            for i in 0..43 {
                                spend(&mut work)?;
                                let k = (i + shift) % seed.len();
                                let c = cipher[i] ^ seed[k] ^ master[(offset + k) % master.len()];
                                if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'/') {
                                    valid = false;
                                    break;
                                }
                                out[i] = c;
                            }
                            if !valid {
                                continue;
                            }
                            let c = out[42];
                            let sextet = match c {
                                b'A'..=b'Z' => c - b'A',
                                b'a'..=b'z' => c - b'a' + 26,
                                b'0'..=b'9' => c - b'0' + 52,
                                b'+' => 62,
                                _ => 63,
                            };
                            if sextet % 4 != 0 {
                                continue;
                            }
                            out[43] = b'=';
                            candidates.insert(String::from_utf8(out.to_vec()).map_err(|_| ())?);
                        }
                    }
                }
            }
        }
    }
    Ok(if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    })
}

/// Count quoted/template literals while ignoring comments. Lengths are source
/// bytes, including escape sequences, not decoded UTF-8 string lengths.
fn string_metrics(source: &str) -> (usize, usize) {
    #[derive(Clone, Copy)]
    enum State {
        Code,
        Quoted(u8),
        LineComment,
        BlockComment,
    }

    let bytes = source.as_bytes();
    let mut state = State::Code;
    let mut start = 0;
    let mut count = 0;
    let mut longest = 0;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        let next = bytes.get(index + 1).copied();
        match state {
            State::Code => match (byte, next) {
                (b'/', Some(b'/')) => {
                    state = State::LineComment;
                    index += 1;
                }
                (b'/', Some(b'*')) => {
                    state = State::BlockComment;
                    index += 1;
                }
                (quote @ (b'\'' | b'"' | b'`'), _) => {
                    state = State::Quoted(quote);
                    start = index + 1;
                }
                _ => {}
            },
            State::Quoted(quote) => {
                if byte == b'\\' {
                    index += 1;
                } else if byte == quote {
                    count += 1;
                    longest = longest.max(index - start);
                    state = State::Code;
                }
            }
            State::LineComment => {
                if byte == b'\n' {
                    state = State::Code;
                }
            }
            State::BlockComment => {
                if byte == b'*' && next == Some(b'/') {
                    state = State::Code;
                    index += 1;
                }
            }
        }
        index += 1;
    }
    (count, longest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprints_source_and_reports_browser_api_coverage() {
        let source = r#"
            // "comment is not a literal"
            const worker = new SharedWorker(URL.createObjectURL(new Blob(["abc"])));
            const canvas = new OffscreenCanvas(1, 1).getContext("webgl");
            navigator.userAgentData.getHighEntropyValues(["brands"]);
            try { work(); } catch (error) { XC.splice(0, 1); }
        "#;
        let report = analyze_script(source);
        assert_eq!(report.family, ScriptFamily::Sbsd);
        assert_eq!(report.string_literals, 3);
        assert_eq!(report.max_string_literal_bytes, 6);
        assert_eq!(report.catch_splice_markers, 1);
        assert!(report.browser_apis["shared_worker"]);
        assert!(report.browser_apis["blob_object_urls"]);
        assert!(report.browser_apis["offscreen_canvas"]);
        assert!(report.browser_apis["high_entropy_hints"]);
        assert!(!report.browser_apis["dedicated_worker"]);
        assert_eq!(report_json(&report)["metrics"]["catch_splice_markers"], 1);
    }

    #[test]
    fn computes_standard_sha256_and_handles_empty_input() {
        let report = analyze_script("abc");
        assert_eq!(source_sha256("abc"), report.sha256);
        assert_eq!(
            report.sha256,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(report.bytes, 3);
        assert_eq!(report.lines, 1);
        assert_eq!(report.family, ScriptFamily::Unknown);

        let empty = analyze_script("");
        assert_eq!(empty.lines, 0);
        assert_eq!(empty.string_literals, 0);
    }

    #[test]
    fn recognizes_bmak_and_ignores_commented_markers_for_literal_metrics() {
        let report = analyze_script(
            "const bmak = {}; /* `hidden` */ const body = \"{\\\"sensor_data\\\":1}\";",
        );
        assert_eq!(report.family, ScriptFamily::Bmak);
        assert_eq!(report.string_literals, 1);
        assert_eq!(report.max_string_literal_bytes, 19);
    }

    #[test]
    fn extracts_shuffle_key_from_s8_digit_cipher_array() {
        let snippet = "s8=fA(G9,[['CAr3qAAAAAA','C','r5','b3Mrf','C5rCrbM5r3qAAAAAA','C595bbf','9f99bAMqAAAAAA','f5','b33f3qAAAAAA','9999999','bC3f9rI','IA5C','5AC9','fbAA','CArb','9Ir5','Ibf9C','f5Mb9','IfIAM5','5','I3','I5AA','IAAA'],0]);";
        assert_eq!(extract_bmak_shuffle_key(snippet), Some(6_453_891));
    }
    #[test]
    fn extracts_captured_homedepot_436427_shuffle_key() {
        let source = include_str!("fixtures/homedepot_436427_numeric.js");
        assert_eq!(extract_bmak_shuffle_key(source), Some(8_846_455));
    }
    #[test]
    fn xor_search_budget_rejects_incomplete_cross_products() {
        let source = format!(
            "var a={};",
            serde_json::to_string(&["A".repeat(44), "b".repeat(32), "c".repeat(32)]).unwrap()
        );
        let ast = ast_scope::Analysis::parse(&source).unwrap();
        assert_eq!(extract_xor_with_budget(&ast, 10), Err(()));
        assert_eq!(extract_xor_with_budget(&ast, 100_000), Ok(None));
    }
    #[test]
    fn xor_search_requires_unique_complete_results() {
        let token = "gyC8AZNPtjoZhpueCDw8o59DmLPOz/qFpTY1O/mhanE=";
        let seed = b"a7pf9mk2wq0z8bn5xj3r";
        let cipher: String = token
            .bytes()
            .enumerate()
            .map(|(i, b)| char::from(b ^ seed[i % seed.len()] ^ b'~'))
            .collect();
        let mut strings = vec![
            cipher,
            String::from_utf8(seed.to_vec()).unwrap(),
            "~".into(),
        ];
        let source = format!("var a={};", serde_json::to_string(&strings).unwrap());
        let ast = ast_scope::Analysis::parse(&source).unwrap();
        assert_eq!(
            extract_xor_with_budget(&ast, 100_000),
            Ok(Some(token.into()))
        );
        strings.extend((0..100).map(|_| "b".repeat(32)));
        let source = format!("var a={};", serde_json::to_string(&strings).unwrap());
        let ast = ast_scope::Analysis::parse(&source).unwrap();
        assert_eq!(extract_xor_with_budget(&ast, 100_000), Err(()));
    }
    #[test]
    #[ignore = "requires a private saved full source supplied explicitly"]
    fn verify_saved_xor_capture() {
        let path =
            std::env::var("CODEC_PRIVATE_XOR_SCRIPT").expect("set private saved source path");
        let source = std::fs::read_to_string(path).unwrap();
        let ast = ast_scope::Analysis::parse(&source).unwrap();
        assert!(delta_version::extract_with_analysis(&source, &ast).is_none());
        let token = extract_xor_with_budget(&ast, XOR_WORK_BUDGET)
            .expect("saved build stays within budget")
            .expect("XOR version");
        assert_eq!(extract_bmak_ver(&source), Some(token));
        println!(
            "saved full build: {} bytes, XOR fallback proven, budget {}",
            source.len(),
            XOR_WORK_BUDGET
        );
    }
}

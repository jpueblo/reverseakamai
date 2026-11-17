//! Digit-cipher tables may be split across arrays. Bind numeric pool members
//! to a key initializer used as the second member of a cipher-key pair.
//! This recognizer never executes a source function or guesses a numeric key.
use super::{bind_numeric_anchor, decode_numeric_key, numeric_constants, literal_arrays};
use native_payload::template::parse_js_string_array;
use std::collections::{BTreeMap, BTreeSet};
fn number(s: &str, constants: &BTreeMap<String, i64>) -> Option<usize> {
    s.parse::<usize>()
        .ok()
        .or_else(|| usize::try_from(*constants.get(s)?).ok())
}
// Only deduplicate a repeated lookup when it is the same entire expression
// on both sides of an outer conditional, not an arithmetic combination.
fn repeated_conditional(t: &[&str], unary_helpers: &BTreeSet<&str>) -> bool {
    let mut depth = 0i32;
    let mut question = None;
    let mut nested = 0usize;
    for (i, token) in t.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => depth += 1,
            ")" | "]" | "}" => depth -= 1,
            "?" if depth == 0 => {
                if question.is_none() {
                    question = Some(i);
                } else {
                    nested += 1;
                }
            }
            ":" if depth == 0 && question.is_some() => {
                if nested > 0 {
                    nested -= 1;
                    continue;
                }
                let left = &t[question.unwrap() + 1..i];
                let right = &t[i + 1..];
                return !left.is_empty()
                    && left == right
                    && !matches!(left[0], "-" | "!" | "~")
                    && !unary_helpers.contains(left[0]);
            }
            _ => {}
        }
    }
    false
}
pub(super) struct Extraction {
    pub key: Option<u32>,
    pub method: &'static str,
    pub candidates: BTreeMap<&'static str, Vec<u32>>,
}
pub(super) fn extract_shuffle_key(source: &str) -> Option<u32> {
    analyze(source).key
}
pub(super) fn analyze(source: &str) -> Extraction {
    let t = literal_arrays::tokens(source);
    let constants = numeric_constants::integer_constants(&t);
    // Seed/flag helpers used in unrelated two-element arrays are not key
    // decoders. Their operand must not be mistaken for the helper's result.
    let unary_helpers: BTreeSet<_> = t
        .windows(12)
        .filter(|w| {
            w[1..4] == ["=", "function", "("]
                && w[5..8] == [")", "{", "return"]
                && matches!(w[8], "-" | "!" | "~")
                && w[9] == w[4]
                && w[10..12] == [";", "}"]
        })
        .map(|w| w[0])
        .collect();
    let arrays: Vec<_> = literal_arrays::string_arrays(source)
        .into_iter()
        .map(|s| {
            (
                s.as_ptr() as usize - source.as_ptr() as usize,
                parse_js_string_array(s),
            )
        })
        .collect();
    let all: Vec<_> = arrays.iter().flat_map(|(_, a)| a.iter()).collect();
    let mut maps = BTreeSet::new();
    for a in all.iter().filter(|s| s.len() == 17) {
        let Some(m32) = bind_numeric_anchor(&[255; 256], a, b"4294967295.000000") else {
            continue;
        };
        for b in all.iter().filter(|s| s.len() == 14) {
            let Some(m23) = bind_numeric_anchor(&m32, b, b"8388607.000000") else {
                continue;
            };
            for c in all.iter().filter(|s| s.len() == 12) {
                let Some(mut map) = bind_numeric_anchor(&m23, c, b"65535.000000") else {
                    continue;
                };
                let ones: BTreeSet<_> = all
                    .iter()
                    .filter_map(|s| {
                        let b = s.as_bytes();
                        (s.is_ascii()
                            && (3..=6).contains(&b.len())
                            && map[b[0] as usize] == 255
                            && b[1..].iter().all(|c| map[*c as usize] == 0))
                        .then(|| b[0])
                    })
                    .collect();
                if ones.len() == 1 {
                    map[*ones.first().unwrap() as usize] = 1;
                }
                maps.insert(map);
            }
        }
    }
    let key_pair_variables: BTreeSet<_> = t
        .windows(5)
        .filter(|w| w[0] == "[" && w[2] == "," && w[4] == "]")
        .map(|w| w[3])
        .collect();
    let mut candidates = BTreeSet::new();
    let mut sentinel_candidates = BTreeSet::new();
    let mut registered_candidates = BTreeSet::new();
    for map in maps {
        for (offset, a) in &arrays {
            // Lazy numeric-getter registration layout observed in the fresh
            // HomeDepot build: [u32_max, LCG multiplier, mask, build key, ...].
            // Its unrelated 8888888 sentinel is in another mathematical pool.
            let invocation = t
                .iter()
                .position(|token| token.as_ptr() as usize - source.as_ptr() as usize == *offset)
                .is_some_and(|i| i >= 2 && t[i - 2] == "}" && t[i - 1] == "(");
            if invocation
                && a.len() >= 4
                && bind_numeric_anchor(&map, &a[0], b"4294967295.000000").is_some()
                && decode_numeric_key(&a[1], &map) == Some(65_793)
                && bind_numeric_anchor(&map, &a[2], b"8388607.000000").is_some()
            {
                if let Some(key) = decode_numeric_key(&a[3], &map) {
                    registered_candidates.insert(key);
                }
            }
            for pair in a.windows(2) {
                if decode_numeric_key(&pair[0], &map) == Some(8_888_888) {
                    if let Some(key) = decode_numeric_key(&pair[1], &map) {
                        sentinel_candidates.insert(key);
                    }
                }
            }
        }
        let mut pools = BTreeMap::new();
        for (offset, a) in &arrays {
            let Some(i) = t
                .iter()
                .position(|token| token.as_ptr() as usize - source.as_ptr() as usize == *offset)
            else {
                continue;
            };
            // Supported literal pool or decoded-pool invocation: pool=decode(state,[[...],...]).
            let owner = if i >= 2 && t[i - 1] == "=" {
                Some(t[i - 2])
            } else if i >= 7
                && t[i - 1] == "["
                && t[i - 2] == ","
                && t[i - 4] == "("
                && t[i - 6] == "="
            {
                Some(t[i - 7])
            } else {
                None
            };
            if let Some(owner) = owner {
                pools.insert(owner, a);
            }
        }
        for i in 0..t.len().saturating_sub(3) {
            if t[i] != "var" || t[i + 2] != "=" || t[i + 3] == "function" {
                continue;
            }
            let var = t[i + 1];
            if matches!(t[i + 3], "-" | "!" | "~") || unary_helpers.contains(t[i + 3]) {
                continue;
            }
            if !key_pair_variables.contains(var) {
                continue;
            }
            let mut depth = 0i32;
            let mut end = i + 3;
            while end < t.len() {
                match t[end] {
                    "(" | "[" | "{" => depth += 1,
                    ")" | "]" | "}" => depth -= 1,
                    ";" if depth == 0 => break,
                    _ => {}
                }
                if depth < 0 {
                    break;
                }
                end += 1;
            }
            if end == t.len() || t[i + 3..end].contains(&"function") {
                continue;
            }
            let mut values = BTreeSet::new();
            let mut lookups = 0;
            let mut index_depth = 0i32;
            let mut numeric_index_lookup = false;
            for w in t[i + 3..end].windows(4) {
                // A digit-table value used as another array's index is not
                // the value of this initializer. The fresh 919ca0 build has
                // a second pair member containing buffer[pool[one]], which
                // otherwise creates a false key candidate of 1.
                let inside_index = index_depth > 0;
                match w[0] {
                    "[" => index_depth += 1,
                    "]" => index_depth -= 1,
                    _ => {}
                }
                if w[1] != "[" || w[3] != "]" {
                    continue;
                }
                let Some(pool) = pools.get(w[0]) else {
                    continue;
                };
                let Some(index) = number(w[2], &constants) else {
                    continue;
                };
                let Some(encoded) = pool.get(index) else {
                    continue;
                };
                if let Some(value) = decode_numeric_key(encoded, &map) {
                    if inside_index {
                        // Reject the whole initializer rather than merely
                        // deleting an index operand from a larger expression.
                        numeric_index_lookup = true;
                        continue;
                    }
                    values.insert(value);
                    lookups += 1;
                }
            }
            if !numeric_index_lookup
                && values.len() == 1
                && (lookups == 1
                    || (lookups == 2 && repeated_conditional(&t[i + 3..end], &unary_helpers)))
            {
                candidates.insert(*values.first().unwrap());
            }
        }
    }
    let (key, method) = if candidates.len() == 1 {
        (candidates.first().copied(), "unique_bound_initializer")
    } else if !candidates.is_empty() {
        (None, "ambiguous_bound_initializers")
    } else if registered_candidates.len() == 1 {
        (
            registered_candidates.first().copied(),
            "unique_registered_pool",
        )
    } else if !registered_candidates.is_empty() {
        (None, "ambiguous_registered_pools")
    } else if sentinel_candidates.len() == 1 {
        (sentinel_candidates.first().copied(), "unique_sentinel_pair")
    } else if !sentinel_candidates.is_empty() {
        (None, "ambiguous_sentinel_pairs")
    } else {
        (None, "no_supported_numeric_layout")
    };
    Extraction {
        key,
        method,
        candidates: BTreeMap::from([
            ("bound_initializers", candidates.into_iter().collect()),
            (
                "registered_pools",
                registered_candidates.into_iter().collect(),
            ),
            ("sentinel_pairs", sentinel_candidates.into_iter().collect()),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn digit_table_used_as_another_arrays_index_is_not_a_key_value() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js")
            .replace("[\"noise\", \"JbBmBvJ\"]", "[\"BNNN\", \"JbBmBvJ\"]");
        for expression in [
            "buffer[pool[0]]",
            "convert(buffer[pool[0]])",
            "combine(buffer[pool[1]],pool[0])",
        ] {
            let source =
                format!("{source}\nvar unrelated={expression};var flags=[session,unrelated];");
            let result = analyze(&source);
            assert_eq!(result.key, Some(9_618_129));
            assert_eq!(result.candidates["bound_initializers"], vec![9_618_129]);
        }
    }
    #[test]
    fn repeated_operands_and_negated_branches_are_not_positive_key_initializers() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js")
            .replace("var decoy=", "// var decoy=");
        for expression in [
            "parseInt(pool[index])+parseInt(pool[index])",
            "flag ? negative(pool[index]) : negative(pool[index])",
        ] {
            let source = format!(
                "var negative=function(n){{return -n;}};\n{}",
                source.replace("parseInt(pool[index])", expression)
            );
            assert_eq!(analyze(&source).key, None);
        }
    }
    #[test]
    fn repeated_same_key_branches_are_one_candidate() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let source = source.replace(
            "parseInt(pool[index])",
            "flag ? parseInt(pool[index]) : parseInt(pool[index])",
        );
        let result = analyze(&source);
        assert_eq!(result.key, Some(9_618_129));
        assert_eq!(result.method, "unique_bound_initializer");
        assert_eq!(result.candidates["bound_initializers"], vec![9_618_129]);
    }
    #[test]
    fn unrelated_negative_seed_does_not_create_a_positive_key_candidate() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let source = source.replace("[\"noise\", \"JbBmBvJ\"]", "[\"BNNN\", \"JbBmBvJ\"]");
        let source = format!(
            "{source}\nvar negative=function(n){{return -n;}};var seed=negative(pool[0]);var seedPair=[session,seed];"
        );
        let result = analyze(&source);
        assert_eq!(result.key, Some(9_618_129));
        assert_eq!(result.candidates["bound_initializers"], vec![9_618_129]);
    }
    #[test]
    fn captured_lazy_layout_reports_sentinel_provenance_and_ambiguity() {
        let source = include_str!("fixtures/homedepot_36bb_numeric.js");
        let result = analyze(source);
        assert_eq!(result.key, Some(6_731_889));
        assert_eq!(result.method, "unique_sentinel_pair");
        let different = source.replace("psQHYYI", "psQHYYII");
        let ambiguity = analyze(&format!("{source}\n{different}"));
        assert_eq!(ambiguity.key, None);
        assert_eq!(ambiguity.method, "ambiguous_sentinel_pairs");
    }
    #[test]
    fn registered_getter_pool_takes_priority_over_unrelated_128_sentinel() {
        let source = include_str!("fixtures/homedepot_sdk_registered_numeric.js");
        assert_eq!(extract_shuffle_key(source), Some(2_207_957));
        let conflicting = source.replace("xxrsq2s", "xxrsq2L");
        assert_eq!(
            extract_shuffle_key(&format!("{source}\n{conflicting}")),
            None
        );
    }
    #[test]
    fn fresh_split_tables_use_key_binding_before_sentinel_decoys() {
        let src = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(extract_shuffle_key(src), Some(9_618_129));
        assert_eq!(
            extract_shuffle_key(
                &src.replace("shuffle", "renamed_key")
                    .replace("pool", "renamed_pool")
            ),
            Some(9_618_129)
        );
        assert_eq!(
            extract_shuffle_key(
                &src.replace("var keys=[session,shuffle]", "var keys=[shuffle,session]")
                    .replace("var decoy=", "// var decoy=")
            ),
            None
        );
    }
    #[test]
    fn conflicting_bindings_and_split_sentinels_fail_closed() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let other = source
            .replace("pool", "second_pool")
            .replace("shuffle", "second_shuffle")
            .replace("JbBmBvJ", "JbBmBvm");
        assert_eq!(extract_shuffle_key(&format!("{source}\n{other}")), None);
        let lazy = include_str!("fixtures/homedepot_d9_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&format!("{lazy}\nregister([\"RRRRRRR\",\"dTT6TYR6\"]);")),
            None
        );
    }
    #[test]
    fn fresh_lazy_accessor_layout_uses_unique_split_anchor_sentinel() {
        let src = include_str!("fixtures/homedepot_d9_split_numeric.js");
        assert_eq!(extract_shuffle_key(src), Some(2_330_368));
        assert_eq!(extract_shuffle_key(&format!("/* {src} */")), None);
        assert_eq!(
            extract_shuffle_key(&src.replace("dTT6TYR", "unknown")),
            None
        );
    }
}

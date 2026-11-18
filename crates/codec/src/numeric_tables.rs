//! Digit-cipher tables may be split across arrays. Bind numeric pool members
//! to a key initializer used as the second member of a cipher-key pair.
//! This recognizer never executes a source function or guesses a numeric key.
use super::verified_legacy_layout;
use super::{
    ast_scope::{Analysis, Numbers},
    bind_numeric_anchor, decode_numeric_key, literal_arrays,
};
use native_payload::template::parse_js_string_array;
use oxc_syntax::symbol::SymbolId;
use std::collections::{BTreeMap, BTreeSet};
// Charge worst-case glyph-slot visits and map copies, not just loop counts.
const NUMERIC_MAP_WORK_BUDGET: usize = 25_000_000;
const NUMERIC_MAP_LIMIT: usize = 256;
fn digit_maps<'a>(
    values: impl IntoIterator<Item = &'a str>,
    mut remaining: usize,
) -> Option<BTreeSet<[u8; 256]>> {
    let values: BTreeSet<_> = values.into_iter().collect();
    let mut maps = BTreeSet::from([[255; 256]]);
    for expected in [
        b"4294967295.000000".as_slice(),
        b"8388607.000000".as_slice(),
        b"65535.000000".as_slice(),
    ] {
        let candidates: Vec<_> = values
            .iter()
            .copied()
            .filter(|s| s.len() == expected.len())
            .collect();
        let mut next = BTreeSet::new();
        for map in &maps {
            for encoded in &candidates {
                remaining = remaining.checked_sub(256 + encoded.len() * 256)?;
                if let Some(bound) = bind_numeric_anchor(map, encoded, expected) {
                    next.insert(bound);
                    if next.len() > NUMERIC_MAP_LIMIT {
                        return None;
                    }
                }
            }
        }
        maps = next;
        if maps.is_empty() {
            break;
        }
    }
    let powers: Vec<_> = values
        .iter()
        .copied()
        .filter(|s| (3..=6).contains(&s.len()) && s.is_ascii())
        .collect();
    let mut completed = BTreeSet::new();
    for mut map in maps {
        let mut ones = BTreeSet::new();
        for s in &powers {
            remaining = remaining.checked_sub(1 + s.len())?;
            let bytes = s.as_bytes();
            if map[bytes[0] as usize] == 255 && bytes[1..].iter().all(|c| map[*c as usize] == 0) {
                ones.insert(bytes[0]);
            }
        }
        if ones.len() == 1 {
            map[*ones.first().unwrap() as usize] = 1;
        }
        completed.insert(map);
    }
    Some(completed)
}
fn number(s: &str, constants: &Numbers) -> Option<usize> {
    literal_arrays::decimal_integer(s)
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| usize::try_from(*constants.get(s)?).ok())
}
// Validate the complete value-producing initializer, not merely a lookup
// operand somewhere inside it. Unknown calls and arithmetic are not transparent.
const LOOKUP_PLAN_WORK_BUDGET: usize = 2_000_000;
struct PlanWork {
    remaining: usize,
    exhausted: bool,
}
impl PlanWork {
    fn charge(&mut self, count: usize) -> Option<()> {
        if let Some(remaining) = self.remaining.checked_sub(count) {
            self.remaining = remaining;
            Some(())
        } else {
            self.exhausted = true;
            None
        }
    }
}
type DecodedValues = BTreeMap<(usize, usize), Option<u32>>;
fn decode_cached(encoded: &str, map: &[u8; 256], values: &mut DecodedValues) -> Option<u32> {
    // Identity keys avoid hashing/comparing the bytes of a large leaf again.
    *values
        .entry((encoded.as_ptr() as usize, encoded.len()))
        .or_insert_with(|| decode_numeric_key(encoded, map))
}
enum LookupPlan<'a> {
    Encoded(&'a str),
    Equal(Box<LookupPlan<'a>>, Box<LookupPlan<'a>>),
}
impl LookupPlan<'_> {
    fn decode(&self, map: &[u8; 256], values: &mut DecodedValues) -> Option<u32> {
        match self {
            Self::Encoded(encoded) => decode_cached(encoded, map, values),
            Self::Equal(a, b) => {
                let a = a.decode(map, values)?;
                let b = b.decode(map, values)?;
                (a == b).then_some(a)
            }
        }
    }
}
fn compile_lookup_plan<'a>(
    t: &[&str],
    pools: &BTreeMap<SymbolId, &'a Vec<String>>,
    constants: &Numbers,
    ast: &Analysis,
    depth: usize,
    work: &mut PlanWork,
) -> Option<LookupPlan<'a>> {
    work.charge(t.len())?;
    if depth > 32 {
        return None;
    }
    // Pair parentheses once instead of rescanning the shrinking slice.
    let mut closes = vec![usize::MAX; t.len()];
    let mut opens = Vec::new();
    for (index, token) in t.iter().enumerate() {
        match *token {
            "(" => opens.push(index),
            ")" => {
                let open = opens.pop()?;
                closes[open] = index;
            }
            _ => {}
        }
    }
    if !opens.is_empty() {
        return None;
    }
    let (mut start, mut end) = (0, t.len());
    while start < end && t[start] == "(" && closes[start] == end - 1 {
        start += 1;
        end -= 1;
    }
    let t = &t[start..end];
    let mut level = 0i32;
    let mut question = None;
    for (i, token) in t.iter().enumerate() {
        match *token {
            "(" | "[" | "{" => level += 1,
            ")" | "]" | "}" => level -= 1,
            "?" if level == 0 => {
                if question.is_some() {
                    return None;
                }
                question = Some(i);
            }
            ":" if level == 0 => {
                let q = question?;
                // Only a simple, side-effect-free condition is supported.
                if q != 1 {
                    return None;
                }
                let a = compile_lookup_plan(&t[q + 1..i], pools, constants, ast, depth + 1, work)?;
                let b = compile_lookup_plan(&t[i + 1..], pools, constants, ast, depth + 1, work)?;
                return Some(LookupPlan::Equal(Box::new(a), Box::new(b)));
            }
            _ => {}
        }
    }
    if t.len() >= 4 && t[..2] == ["parseInt", "("] && t.last() == Some(&")") {
        if !ast.builtin(t[0], "parseInt") {
            return None;
        }
        let args = &t[2..t.len() - 1];
        let lookup = if args.len() == 6 && args[4..] == [",", "10"] {
            &args[..4]
        } else {
            args
        };
        // parseInt of one supported pool lookup is value-preserving here.
        if lookup.len() != 4 {
            return None;
        }
        return compile_lookup_plan(lookup, pools, constants, ast, depth + 1, work);
    }
    if t.len() != 4 || t[1] != "[" || t[3] != "]" {
        return None;
    }
    if !ast.available(t[0]) {
        return None;
    }
    let pool = *pools.get(&ast.symbol(t[0])?)?;
    Some(LookupPlan::Encoded(
        pool.get(number(t[2], constants)?)?.as_str(),
    ))
}
/// Lookup operand of a recognized global conversion chain `G[a](b[c](pool[i]))`.
/// The pool may be a proven-stable or write-once binding, and the index may be a
/// proven or write-once constant. The conversion itself is opaque (its names are
/// runtime-decoded), so results from this plan are reported separately.
fn compile_conversion_lookup<'a>(
    t: &[&str],
    pools: &BTreeMap<SymbolId, &'a Vec<String>>,
    constants: &Numbers,
    ast: &Analysis,
) -> Option<LookupPlan<'a>> {
    if t.len() != 4 || t[1] != "[" || t[3] != "]" || !ast.write_once_or_stable(t[0]) {
        return None;
    }
    let pool = *pools.get(&ast.symbol(t[0])?)?;
    let index = literal_arrays::decimal_integer(t[2])
        .or_else(|| constants.get_write_once(t[2]).copied())
        .and_then(|n| usize::try_from(n).ok())?;
    Some(LookupPlan::Encoded(pool.get(index)?.as_str()))
}
struct PreparedKeyInitializer<'a> {
    plan: Option<LookupPlan<'a>>,
    conversion: Option<LookupPlan<'a>>,
    conversion_binding_invalid: bool,
    reads_pool: bool,
    invalid_binding: bool,
    unsafe_pool: bool,
    lookup_operands: Vec<&'a str>,
}
// A bounded compatibility route for the independently verified legacy loopback
// script. Unknown changes must not turn an opaque initializer into a trusted
// conversion. The key is read from its full LCG parameter signature, never cached.
fn legacy_parameter_key(array: &[String], map: &[u8; 256]) -> Option<u32> {
    let mut keys = BTreeSet::new();
    for a in array.windows(6) {
        if decode_numeric_key(&a[0], map) == Some(65_793)
            && bind_numeric_anchor(map, &a[1], b"4294967295.000000").is_some()
            && decode_numeric_key(&a[2], map) == Some(4_282_663)
            && bind_numeric_anchor(map, &a[3], b"8388607.000000").is_some()
            && decode_numeric_key(&a[4], map) == Some(8_888_888)
        {
            if let Some(key) = decode_numeric_key(&a[5], map).filter(|k| *k < 1 << 24) {
                keys.insert(key);
            }
        }
    }
    if keys.len() == 1 {
        keys.first().copied()
    } else {
        None
    }
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
    analyze_with_analysis(source, None)
}
pub(super) fn analyze_with_analysis(source: &str, provided: Option<&Analysis>) -> Extraction {
    analyze_with_budget(source, provided, NUMERIC_MAP_WORK_BUDGET)
}
fn analyze_with_budget(source: &str, provided: Option<&Analysis>, map_work: usize) -> Extraction {
    analyze_with_limits(source, provided, map_work, LOOKUP_PLAN_WORK_BUDGET)
}
fn analyze_with_limits(
    source: &str,
    provided: Option<&Analysis>,
    map_work: usize,
    plan_work: usize,
) -> Extraction {
    let allow_legacy = verified_legacy_layout(source);
    let owned = if !allow_legacy && provided.is_none() {
        Analysis::parse(source)
    } else {
        None
    };
    let analysis = if allow_legacy {
        None
    } else {
        provided.or(owned.as_ref())
    };
    if !allow_legacy && analysis.as_ref().is_none_or(|a| a.dynamic) {
        return Extraction {
            key: None,
            method: "invalid_or_dynamic_javascript",
            candidates: BTreeMap::new(),
        };
    }
    let empty = Numbers::default();
    let constants = analysis.as_ref().map_or(&empty, |a| &a.numbers);
    let t = literal_arrays::tokens(source);
    let legacy_arrays: Vec<_> = if allow_legacy {
        literal_arrays::string_arrays(source)
            .into_iter()
            .map(|s| {
                (
                    s.as_ptr() as usize - source.as_ptr() as usize,
                    parse_js_string_array(s),
                )
            })
            .collect()
    } else {
        Vec::new()
    };
    let arrays = analysis
        .as_ref()
        .map_or(legacy_arrays.as_slice(), |a| a.string_arrays.as_slice());
    let Some(maps) = digit_maps(
        arrays
            .iter()
            .flat_map(|(_, values)| values.iter().map(String::as_str)),
        map_work,
    ) else {
        return Extraction {
            key: None,
            method: "numeric_map_budget_exhausted",
            candidates: BTreeMap::new(),
        };
    };
    if maps.is_empty() {
        return Extraction {
            key: None,
            method: "no_supported_numeric_layout",
            candidates: BTreeMap::new(),
        };
    }
    let mut unsupported_bound_lookup = false;
    let mut unsupported_lookup_values = BTreeSet::new();
    let key_pair_variables: BTreeSet<_> = analysis
        .as_ref()
        .into_iter()
        .flat_map(|a| a.key_pairs.iter().map(|p| p.0))
        .collect();
    let unavailable_pairs: BTreeSet<_> = analysis
        .as_ref()
        .into_iter()
        .flat_map(|a| a.key_pairs.iter().filter(|p| !p.1).map(|p| p.0))
        .collect();
    // The opaque-conversion route also tolerates deferred closure reads of a
    // stable key binding; a straight-line read before its initializer still
    // makes the binding unusable.
    let unusable_conversion_pairs: BTreeSet<_> = analysis
        .as_ref()
        .into_iter()
        .flat_map(|a| a.key_pairs.iter().filter(|p| !p.1 && !p.2).map(|p| p.0))
        .collect();
    let mut candidates = BTreeSet::new();
    let mut conversion_candidates = BTreeSet::new();
    let mut sentinel_candidates = BTreeSet::new();
    let mut registered_candidates = BTreeSet::new();
    let mut malformed_registration = false;
    let mut unsafe_pool = false;
    let mut unsafe_key = false;
    let mut legacy_candidates = BTreeSet::new();
    // Pool ownership/provenance is independent of the candidate digit map.
    // Build its sorted read index once; nested evidence spans use binary search.
    let pool_facts = analysis.as_ref().map(|ast| {
        let arrays_by_offset: BTreeMap<_, _> = arrays
            .iter()
            .map(|(offset, values)| (*offset, values))
            .collect();
        let mut pools = BTreeMap::new();
        let mut unsafe_owners = BTreeSet::new();
        for (id, offset) in &ast.pool_owners {
            if let Some(array) = arrays_by_offset.get(offset) {
                if !ast.pool_intact_symbol(*id) {
                    unsafe_owners.insert(*id);
                }
                pools.insert(*id, *array);
            }
        }
        let read_positions = ast.derived_read_positions(pools.keys().copied());
        let owner_positions = ast.owner_read_positions(pools.keys().copied());
        let unproven_pool_read = ast
            .unproven_key_writes
            .iter()
            .any(|span| Analysis::has_indexed_read(&read_positions, *span));
        (
            pools,
            unsafe_owners,
            read_positions,
            unproven_pool_read,
            owner_positions,
        )
    });
    unsupported_bound_lookup |= pool_facts.as_ref().is_some_and(|facts| facts.3);
    // Tokenization, binding/poison checks and pool reads do not depend on the
    // candidate digit mapping. Prepare them once before decoding candidates.
    let mut prepared_initializers = Vec::new();
    let mut work = PlanWork {
        remaining: plan_work,
        exhausted: false,
    };
    if let (Some(ast), Some((pools, unsafe_owners, read_positions, _, owner_positions))) =
        (analysis.as_ref(), pool_facts.as_ref())
    {
        for initializer in &ast.initializers {
            if !key_pair_variables.contains(&initializer.symbol) || initializer.callable {
                continue;
            }
            let var = &source[initializer.binding.start as usize..initializer.binding.end as usize];
            let reads_pool = Analysis::has_indexed_read(read_positions, initializer.expression);
            if initializer.seed_operator {
                unsupported_bound_lookup |= reads_pool;
                continue;
            }
            let source_expression =
                &source[initializer.expression.start as usize..initializer.expression.end as usize];
            // Charge before tokenization. The temporary RHS tokens are dropped
            // after compiling a plan; nested spans never retain duplicate views.
            if work.charge(source_expression.len()).is_none() {
                break;
            }
            let expression_tokens = literal_arrays::tokens(source_expression);
            let expression = expression_tokens.as_slice();
            if expression
                .first()
                .is_some_and(|token| ast.unary_seed(token))
            {
                unsupported_bound_lookup |= reads_pool;
                continue;
            }
            if work.charge(expression.len()).is_none() {
                break;
            }
            unsafe_key |= ast.poisoned_in(initializer.expression);
            let invalid_binding = !ast.stable(var)
                || ast
                    .symbol(var)
                    .is_some_and(|id| unavailable_pairs.contains(&id));
            let unsafe_pool = expression.windows(2).any(|w| {
                w[1] == "["
                    && ast
                        .symbol(w[0])
                        .is_some_and(|id| unsafe_owners.contains(&id))
            });
            let lookup_operands = expression
                .windows(4)
                .filter_map(|w| {
                    if w[1] != "[" || w[3] != "]" {
                        return None;
                    }
                    pools
                        .get(&ast.symbol(w[0])?)
                        .and_then(|pool| number(w[2], constants).and_then(|index| pool.get(index)))
                        .map(String::as_str)
                })
                .collect();
            let plan = if initializer.simple {
                compile_lookup_plan(expression, pools, constants, ast, 0, &mut work)
            } else {
                None
            };
            if work.exhausted {
                break;
            }
            // The chain must read a pool owner exactly once, at its lookup
            // operand (the opaque name expressions may not read a pool), and be
            // called through a global-object alias.
            let conversion_binding_invalid = !ast.stable(var)
                || ast
                    .symbol(var)
                    .is_some_and(|id| unusable_conversion_pairs.contains(&id));
            let conversion = initializer
                .conversion
                .filter(|_| plan.is_none() && initializer.simple)
                .filter(|(global, _)| ast.global_object_alias_at(*global))
                .filter(|(_, lookup)| {
                    Analysis::indexed_reads_in(owner_positions, initializer.expression) == 1
                        && Analysis::indexed_reads_in(owner_positions, *lookup) == 1
                })
                .and_then(|(_, lookup)| {
                    let lookup_tokens =
                        literal_arrays::tokens(&source[lookup.start as usize..lookup.end as usize]);
                    compile_conversion_lookup(&lookup_tokens, pools, constants, ast)
                });
            prepared_initializers.push(PreparedKeyInitializer {
                plan,
                conversion,
                conversion_binding_invalid,
                reads_pool,
                invalid_binding,
                unsafe_pool,
                lookup_operands,
            });
        }
    }
    if work.exhausted {
        return Extraction {
            key: None,
            method: "initializer_plan_budget_exhausted",
            candidates: BTreeMap::new(),
        };
    }
    for map in maps {
        let mut decoded_values = DecodedValues::new();
        for (offset, a) in arrays {
            // Lazy numeric-getter registration layout observed in the fresh
            // HomeDepot build: [u32_max, LCG multiplier, mask, build key, ...].
            // Its unrelated 8888888 sentinel is in another mathematical pool.
            let invocation = if allow_legacy {
                let i = t.partition_point(|token| {
                    (token.as_ptr() as usize - source.as_ptr() as usize) < *offset
                });
                i >= 2
                    && t.get(i).is_some_and(|token| {
                        token.as_ptr() as usize - source.as_ptr() as usize == *offset
                    })
                    && t[i - 2] == "}"
                    && t[i - 1] == "("
            } else {
                analysis
                    .as_ref()
                    .is_some_and(|a| a.is_registered_array(*offset))
            };
            // The getter ABI starts with the u32 anchor. A head shifted by one
            // slot ([x, u32, 65793, ...]) is a corrupted registration and fails
            // closed. Rotated constant arrays that merely hold the anchor at
            // index 1 without the multiplier after it are not registrations.
            let anchor = |v: &String| bind_numeric_anchor(&map, v, b"4294967295.000000").is_some();
            let registration_head = a.first().is_some_and(anchor)
                || (a.get(1).is_some_and(anchor)
                    && a.get(2).and_then(|v| decode_numeric_key(v, &map)) == Some(65_793));
            if invocation && registration_head {
                if a.len() < 3
                    || bind_numeric_anchor(&map, &a[0], b"4294967295.000000").is_none()
                    || decode_numeric_key(&a[1], &map) != Some(65_793)
                {
                    malformed_registration = true;
                } else {
                    // Supported ABIs differ only by the explicit mask slot.
                    let key_index = if bind_numeric_anchor(&map, &a[2], b"8388607.000000").is_some()
                    {
                        3
                    } else {
                        2
                    };
                    if let Some(key) = a
                        .get(key_index)
                        .and_then(|v| decode_numeric_key(v, &map))
                        .filter(|k| *k < 1 << 24)
                    {
                        registered_candidates.insert(key);
                    } else {
                        malformed_registration = true;
                    }
                }
            }
            if allow_legacy {
                if let Some(key) = legacy_parameter_key(a, &map) {
                    legacy_candidates.insert(key);
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
        for initializer in &prepared_initializers {
            let proven = initializer
                .plan
                .as_ref()
                .and_then(|plan| plan.decode(&map, &mut decoded_values));
            let converted = initializer
                .conversion
                .as_ref()
                .and_then(|plan| plan.decode(&map, &mut decoded_values))
                .filter(|key| *key < 1 << 24);
            if let Some(value) = proven {
                unsafe_key |= initializer.invalid_binding;
                unsafe_pool |= initializer.unsafe_pool;
                candidates.insert(value);
            } else if let Some(value) = converted {
                unsafe_key |= initializer.conversion_binding_invalid;
                unsafe_pool |= initializer.unsafe_pool;
                conversion_candidates.insert(value);
            } else if initializer.reads_pool {
                unsupported_bound_lookup = true;
                for encoded in &initializer.lookup_operands {
                    if let Some(value) = decode_cached(encoded, &map, &mut decoded_values) {
                        unsupported_lookup_values.insert(value);
                    }
                }
            }
        }
    }
    let (key, method) = if unsafe_key {
        (None, "mutable_key_binding")
    } else if unsafe_pool {
        (None, "mutable_or_aliased_pool")
    } else if malformed_registration {
        (None, "malformed_registered_pool")
    } else if registered_candidates.len() > 1 {
        (None, "ambiguous_registered_pools")
    } else if candidates.len() == 1
        && registered_candidates.len() == 1
        && candidates.first() != registered_candidates.first()
    {
        (None, "conflicting_extraction_strategies")
    } else if candidates.len() == 1
        && !conversion_candidates.is_empty()
        && conversion_candidates != candidates
    {
        (None, "conflicting_extraction_strategies")
    } else if candidates.len() == 1 {
        (candidates.first().copied(), "unique_bound_initializer")
    } else if !candidates.is_empty() {
        (None, "ambiguous_bound_initializers")
    } else if conversion_candidates.len() == 1
        && registered_candidates.len() == 1
        && conversion_candidates != registered_candidates
    {
        (None, "conflicting_extraction_strategies")
    } else if conversion_candidates.len() == 1 {
        (
            conversion_candidates.first().copied(),
            "opaque_global_conversion",
        )
    } else if !conversion_candidates.is_empty() {
        (None, "ambiguous_conversion_initializers")
    } else if registered_candidates.len() == 1 {
        let key = *registered_candidates.first().unwrap();
        if unsupported_lookup_values.contains(&key) {
            (None, "transformed_registered_key")
        } else {
            (Some(key), "unique_registered_pool")
        }
    } else if !registered_candidates.is_empty() {
        (None, "ambiguous_registered_pools")
    } else if legacy_candidates.len() == 1 {
        (
            legacy_candidates.first().copied(),
            "verified_legacy_parameter_pool",
        )
    } else if !legacy_candidates.is_empty() {
        (None, "ambiguous_legacy_parameter_pools")
    } else if unsupported_bound_lookup {
        (None, "unsupported_bound_initializer")
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
                "conversion_initializers",
                conversion_candidates.into_iter().collect(),
            ),
            (
                "registered_pools",
                registered_candidates.into_iter().collect(),
            ),
            ("sentinel_pairs", sentinel_candidates.into_iter().collect()),
            (
                "verified_legacy_pools",
                legacy_candidates.into_iter().collect(),
            ),
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
    fn rejects_transformed_lookup_initializers_from_copilot_review() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for expression in [
            "parseInt(pool[index])+1",
            "parseInt(pool[index])-1",
            "transform(pool[index])",
            "parseInt(transform(pool[index]))",
            "flag ? transform(pool[index]) : transform(pool[index])",
        ] {
            assert_eq!(
                extract_shuffle_key(&source.replace("parseInt(pool[index])", expression)),
                None,
                "{expression}"
            );
        }
        assert_eq!(
            extract_shuffle_key(
                &source.replace("parseInt(pool[index])", "(parseInt(pool[index],10))")
            ),
            Some(9_618_129)
        );
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
    fn fresh_chrome155_short_registration_uses_key_not_sentinel() {
        let source = include_str!("fixtures/homedepot_5d7_short_registration.js");
        assert_eq!(extract_shuffle_key(source), Some(8_682_175));
        assert_eq!(
            extract_shuffle_key(&source.replace("NINSUtG", "NINSUtk")),
            Some(8_682_173)
        );
        assert_eq!(extract_shuffle_key(&source.replace("IGtsk", "IGtsU")), None);
        assert_eq!(
            extract_shuffle_key(&source.replace("}([", "}(['bad',")),
            None
        );
        let other = source.replace("NINSUtG", "NINSUtk");
        assert_eq!(extract_shuffle_key(&format!("{source}\n{other}")), None);
    }
    #[test]
    fn transformed_registered_key_is_not_rescued_by_registration() {
        let source = include_str!("fixtures/homedepot_5d7_short_registration.js");
        for expression in ["parseInt(pool[index])+1", "transform(pool[index])"] {
            let added = format!(
                "{source}var pool=decode(state,[[\"noise\",\"NINSUtG\"],[]]);var index=1;var key={expression};var pair=[session,key];"
            );
            assert_eq!(extract_shuffle_key(&added), None);
        }
    }
    #[test]
    fn prefix_mutated_indexes_are_not_constants() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in ["++index;", "--index;", "index++;", "index--;", "index%=7;"] {
            let changed = source.replace("var shuffle=", &format!("{mutation}var shuffle="));
            assert_eq!(extract_shuffle_key(&changed), None, "{mutation}");
        }
    }
    #[test]
    fn bound_initializer_cannot_override_conflicting_registrations() {
        let source = include_str!("fixtures/homedepot_5d7_short_registration.js");
        let other = source.replace("NINSUtG", "NINSUtk");
        let full = format!(
            "{source}\n{other}\nvar pool=['NINSUtG'];var key=parseInt(pool[0]);var pair=[session,key];"
        );
        assert_eq!(extract_shuffle_key(&full), None);
    }
    #[test]
    fn changed_legacy_sources_are_not_whitelisted() {
        assert!(!verified_legacy_layout("unknown source"));
        let source = include_str!("fixtures/homedepot_217_legacy_parameter_pool.js");
        // The opaque initializer is not trusted merely because a pool has a
        // recognizable signature. Only the exact verified full build may use it.
        assert_eq!(extract_shuffle_key(source), None);
        assert_eq!(
            extract_shuffle_key(&source.replace("opaque(pool[index])", "parseInt(pool[index])+1")),
            None
        );
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

    #[test]
    fn unsupported_index_mutations_fail_closed() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "index&&=0;",
            "index||=0;",
            "index**=2;",
            "index>>>=1;",
            "[index]=[0];",
            "for(index of [0]){}",
        ] {
            assert_eq!(
                extract_shuffle_key(
                    &source.replace("var shuffle=", &format!("{mutation}var shuffle="))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn bound_key_must_agree_with_single_registered_key() {
        let source = include_str!("fixtures/homedepot_5d7_short_registration.js");
        assert_eq!(
            extract_shuffle_key(&format!(
                "{source}var pool=['NINSUtk'];var key=parseInt(pool[0]);var pair=[session,key];"
            )),
            None
        );
    }

    #[test]
    fn mutable_or_aliased_pool_and_deep_index_write_are_unsupported() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "pool.reverse();",
            "pool[1]=pool[0];",
            "pool=[];",
            "var alias=pool;alias.reverse();",
            "function alter(p){p.reverse();}alter(pool);",
            "function a(){function b(){function c(){index=0;}c();}b();}a();",
        ] {
            assert_eq!(
                extract_shuffle_key(
                    &source.replace("var shuffle=", &format!("{mutation}var shuffle="))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn legacy_octal_pool_index_is_unsupported() {
        let source = include_str!("fixtures/homedepot_436427_numeric.js");
        assert_eq!(
            extract_shuffle_key(&source.replace("nW[11]", "nW[011]")),
            None
        );
    }

    #[test]
    fn key_indexes_must_resolve_in_their_actual_scope() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let lookup = "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];";
        for wrapped in [
            format!("function f(index){{{lookup}}}f(0);"),
            format!("{{let index;{lookup}}}"),
            format!("({{run(index){{{lookup}}}}}).run(0);"),
            format!("(function(index){{{lookup}}})(0);"),
        ] {
            assert_eq!(
                extract_shuffle_key(&source.replace(lookup, &wrapped)),
                None,
                "{wrapped}"
            );
        }
    }
    #[test]
    fn key_binding_cannot_change_before_its_pair() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "shuffle=shuffle+1;",
            "shuffle++;",
            "shuffle=0;",
            "[shuffle]=[0];",
            "let shuffle;",
        ] {
            assert_eq!(
                extract_shuffle_key(&source.replace("var keys=", &format!("{mutation}var keys="))),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn destructuring_loop_and_delete_pool_writes_are_unsupported() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "[pool[1]]=[pool[0]];",
            "for(pool[1] of ['noise']){}",
            "({a:pool[1]}={a:'noise'});",
            "delete pool[1];",
        ] {
            assert_eq!(
                extract_shuffle_key(
                    &source.replace("var shuffle=", &format!("{mutation}var shuffle="))
                ),
                None,
                "{mutation}"
            );
        }
    }

    #[test]
    fn additional_destructuring_and_global_writes_are_unsupported() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "x=[index]=[0];",
            "x=[pool[1]]=['noise'];",
            "0?0:[index]=[0];",
            "globalThis.index=0;",
            "window['index']=0;",
        ] {
            assert_eq!(
                extract_shuffle_key(
                    &source.replace("var shuffle=", &format!("{mutation}var shuffle="))
                ),
                None,
                "{mutation}"
            );
        }
        assert_eq!(
            extract_shuffle_key(&source.replace("var keys=", "x=[shuffle]=[0];var keys=")),
            None
        );
    }
    #[test]
    fn bare_arrow_and_reserved_method_parameters_shadow_indexes() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let lookup = "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];";
        for body in [
            format!("(index=>{{{lookup}}})(0);"),
            format!("(index=>(function(){{{lookup}}})())(0);"),
            format!("({{while(index){{{lookup}}}}}).while(0);"),
            format!("class C{{while(index){{{lookup}}}}}new C().while(0);"),
        ] {
            assert_eq!(
                extract_shuffle_key(&source.replace(lookup, &body)),
                None,
                "{body}"
            );
        }
    }
    #[test]
    fn parse_int_requires_an_unmodified_builtin_binding() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let lookup = "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];";
        for prefix in [
            "var parseInt=function(s){return 0;};",
            "function parseInt(){return 0;}",
            "window['parseInt']=function(){return 0;};",
        ] {
            assert_eq!(
                extract_shuffle_key(&format!("{prefix}{source}")),
                None,
                "{prefix}"
            );
        }
        assert_eq!(
            extract_shuffle_key(&source.replace(
                lookup,
                &format!("function f(parseInt){{{lookup}}}f(function(){{return 0;}});")
            )),
            None
        );
        assert_eq!(
            extract_shuffle_key(&format!(
                "var parseInt=function(){{return 0;}};{}",
                source.replace("parseInt(pool[index])", "pool[index]")
            )),
            Some(9_618_129)
        );
    }

    #[test]
    fn expression_arrows_operator_methods_and_with_cannot_supply_index_constants() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let lookup = "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];";
        for body in [
            format!("((index)=>(function(){{{lookup}}})())(0);"),
            format!("0||{{while(index){{{lookup}}}}}.while(0);"),
            format!("with({{index:0}}){{{lookup}}}"),
            format!("with(o){{{lookup}}}"),
        ] {
            assert_eq!(
                extract_shuffle_key(&source.replace(lookup, &body)),
                None,
                "{body}"
            );
        }
    }
    #[test]
    fn parenthesized_loop_and_chained_prefix_property_targets_are_writes() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for mutation in [
            "for(globalThis.index of [0]){}",
            "(globalThis.index)=0;",
            "++(window.index);",
            "--window.self.index;",
            "++((window['index']));",
        ] {
            assert_eq!(
                extract_shuffle_key(
                    &source.replace("var shuffle=", &format!("{mutation}var shuffle="))
                ),
                None,
                "{mutation}"
            );
        }
    }

    #[test]
    fn ast_keeps_independent_bindings_with_the_same_spelling() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for source in [
            format!("function unrelated(index){{return index;}}{source}"),
            format!("{{let index;}}{source}"),
            format!("const other={{while(index){{return index;}}}};{source}"),
            format!("function run(){{{source}window['index']=0;}}run();"),
        ] {
            assert_eq!(extract_shuffle_key(&source), Some(9_618_129));
        }
    }
    #[test]
    fn ast_resolves_escaped_names_and_escaped_builtin_property_writes() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&source.replace("var index=", "var \\u0069ndex=")),
            Some(9_618_129)
        );
        assert_eq!(
            extract_shuffle_key(&format!(
                "window['parse\\u0049nt']=function(){{return 0;}};{source}"
            )),
            None
        );
        assert_eq!(
            extract_shuffle_key(&format!("{source}const broken = ;")),
            None
        );
    }
    #[test]
    fn ast_numeric_bindings_follow_block_scope_and_iife_scope() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&format!("(function(){{{source}}})();")),
            Some(9_618_129)
        );
        assert_eq!(
            extract_shuffle_key(
                &source
                    .replace("var index=+!![];", "{let index=1;")
                    .replace("var decoy=", "}var decoy=")
            ),
            Some(9_618_129)
        );
    }

    #[test]
    fn ast_requires_initializers_before_values_are_consumed() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(
                &(source.replace("var index=+!![];", "var index=late;") + "var late=1;")
            ),
            None
        );
        let pool = "var pool=decode(state,[[\"noise\", \"JbBmBvJ\"],[]]);";
        assert!(source.contains(pool));
        assert_eq!(
            extract_shuffle_key(&(source.replace(pool, "") + pool)),
            None
        );
        assert_eq!(
            extract_shuffle_key(&source.replace(
                "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];",
                "var keys=[session,shuffle];var shuffle=parseInt(pool[index]);"
            )),
            None
        );
    }

    #[test]
    fn ast_cipher_key_pairs_are_arrays_not_computed_comma_indexes() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let changed = source
            .replace(
                "var keys=[session,shuffle];",
                "var other=buffer[session,shuffle];",
            )
            .replace("var decoy=", "// var decoy=");
        assert_eq!(extract_shuffle_key(&changed), None);
    }

    #[test]
    fn ast_key_declarators_include_comma_let_and_const_initializers() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for changed in [
            source.replace(";var shuffle=", ",shuffle="),
            source.replace("var shuffle=", "let shuffle="),
            source.replace("var shuffle=", "const shuffle="),
        ] {
            assert_eq!(extract_shuffle_key(&changed), Some(9_618_129));
        }
        assert_eq!(
            extract_shuffle_key(&source.replace(
                "var shuffle=parseInt(pool[index])",
                "const shuffle=transform(pool[index])"
            )),
            None
        );
        assert_eq!(
            extract_shuffle_key(&source.replace("var shuffle=", "const [shuffle]=")),
            None
        );
    }
    #[test]
    fn ast_ignores_registration_text_inside_nested_templates() {
        let source = include_str!("fixtures/homedepot_d9_split_numeric.js");
        let decoy = r#"var s=`${`}(["sd$s$YUd$MZ666666","YMU$T","dTsMYUR"])`}`;"#;
        assert_eq!(
            extract_shuffle_key(&format!("{source}{decoy}")),
            Some(2_330_368)
        );
    }
    #[test]
    fn ast_block_function_redeclarations_invalidate_builtin_assumptions() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&format!("{{function parseInt(){{return 0;}}}}{source}")),
            None
        );
        assert_eq!(
            extract_shuffle_key(&source.replace("var index=+!![];", "if(0){var index=+!![];}")),
            None
        );
        assert_eq!(
            extract_shuffle_key(&source.replace("var index=+!![];", "if(0)var index=+!![];")),
            None
        );
        assert_eq!(
            extract_shuffle_key(&format!(
                "\"use strict\";{{function parseInt(){{return 0;}}}}{source}"
            )),
            Some(9_618_129)
        );
    }
    #[test]
    fn ast_assigned_key_bindings_cannot_fall_back_to_sentinels() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for binding in [
            "var shuffle;shuffle=parseInt(pool[index]);",
            "shuffle=parseInt(pool[index]);",
            "var shuffle;[shuffle]=[parseInt(pool[index])];",
        ] {
            let changed = source.replace("var shuffle=parseInt(pool[index]);", binding);
            assert_eq!(extract_shuffle_key(&changed), None, "{binding}");
        }
    }
    #[test]
    fn ast_negated_pool_key_cannot_fall_back_to_sentinel() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&source.replace("shuffle=parseInt", "shuffle=-parseInt")),
            None
        );
    }
    #[test]
    fn ast_pattern_defaults_and_loop_sources_block_weak_sentinels() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for binding in [
            "var [shuffle=parseInt(pool[index])]=[];",
            "var {key:shuffle=parseInt(pool[index])}={};",
            "var shuffle;[shuffle=parseInt(pool[index])]=[];",
            "var shuffle;({shuffle=parseInt(pool[index])}={});",
            "var shuffle;for(shuffle of [parseInt(pool[index])]);",
            "for(var shuffle of [parseInt(pool[index])]);",
            "var shuffle;for(shuffle in {[parseInt(pool[index])]:true});",
            "for(var shuffle in {[parseInt(pool[index])]:true});",
            "for(var [shuffle=parseInt(pool[index])] of [[]]);",
            "var shuffle;for([shuffle=parseInt(pool[index])] of [[]]);",
        ] {
            let changed = source.replace("var shuffle=parseInt(pool[index]);", binding);
            assert_eq!(extract_shuffle_key(&changed), None, "{binding}");
        }
    }
    #[test]
    fn ast_parameter_defaults_cannot_become_weak_sentinel_keys() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let prefix = source
            .replace("var shuffle=parseInt(pool[index]);", "")
            .replace("var keys=[session,shuffle];", "");
        for declaration in [
            "function use(shuffle=parseInt(pool[index])){var keys=[session,shuffle];}use();",
            "function use([shuffle=parseInt(pool[index])]=[]){var keys=[session,shuffle];}use();",
            "var use=(shuffle=parseInt(pool[index]))=>{var keys=[session,shuffle];};use();",
        ] {
            assert_eq!(
                extract_shuffle_key(&format!("{prefix}{declaration}")),
                None,
                "{declaration}"
            );
        }
    }
    #[test]
    fn ast_pool_dependencies_through_local_bindings_block_sentinels() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for binding in [
            "var alias=pool;var shuffle=parseInt(alias[index]);",
            "var t=parseInt(pool[index]);var shuffle=t;",
            "var t=parseInt(pool[index]);var shuffle;shuffle=t;",
            "var alias;alias=pool;var shuffle=parseInt(alias[index]);",
            "var a=parseInt(pool[index]);var b=a;var c=b;var shuffle=c;",
            "var a,b; b=a; a=parseInt(pool[index]);var shuffle=b;",
            "var [t=parseInt(pool[index])]=[];var shuffle=t;",
            "var t;[t=parseInt(pool[index])]=[];var shuffle=t;",
            "var t;for(t of [parseInt(pool[index])]);var shuffle=t;",
            "function use(t=parseInt(pool[index])){var shuffle=t;var keys=[session,shuffle];}use();",
        ] {
            let changed = source.replace("var shuffle=parseInt(pool[index]);", binding);
            assert_eq!(extract_shuffle_key(&changed), None, "{binding}");
        }
    }
    #[test]
    fn ast_dependency_closure_handles_cycles_and_independent_bindings() {
        let source = include_str!("fixtures/homedepot_36bb_numeric.js");
        let changed = format!("{source}var a,b;a=b;b=a;var pair=[session,a];");
        assert_eq!(extract_shuffle_key(&changed), Some(6_731_889));
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let changed =
            format!("{source}function unrelated(){{var pool=[],t=pool;var pair=[session,t];}}");
        assert_eq!(extract_shuffle_key(&changed), Some(9_618_129));
    }
    #[test]
    fn ast_inline_and_wrapped_pair_values_cannot_use_weak_sentinels() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let inline = source.replace(
            "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];",
            "var keys=[session,parseInt(pool[index])];",
        );
        assert_eq!(extract_shuffle_key(&inline), None);
        for pair in [
            "[session,+shuffle]",
            "[session,(0,shuffle)]",
            "[session,shuffle|0]",
            "[session,parseInt(pool[index])]",
            "[getSession(),shuffle]",
            "[session,...[shuffle]]",
            "[session,{key:shuffle}]",
        ] {
            let changed = source.replace("[session,shuffle]", pair);
            assert_eq!(extract_shuffle_key(&changed), None, "{pair}");
        }
    }
    #[test]
    fn ast_nested_pair_evidence_ignores_non_pool_reads() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js").replace(
            "var shuffle=parseInt(pool[index]);var keys=[session,shuffle];",
            "",
        );
        let mut nested = format!("function(){{{}}}", "a;".repeat(4096));
        for _ in 0..48 {
            nested = format!("[0,{nested}]");
        }
        let changed = format!("{source}var a;{nested};");
        assert_eq!(extract_shuffle_key(&changed), Some(4096));
    }
    #[test]
    fn duplicate_numeric_anchors_do_not_expand_search() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let anchors = ["3vJ3JbHvJM7NNNNNN", "mrmmbNH7NNNNNN", "bMMrM7NNNNNN"];
        let repeated: Vec<_> = anchors.iter().cycle().take(900).collect();
        let changed = format!(
            "{source}var repeated={};",
            serde_json::to_string(&repeated).unwrap()
        );
        assert_eq!(extract_shuffle_key(&changed), Some(9_618_129));
    }
    #[test]
    fn numeric_map_work_exhaustion_never_returns_a_partial_candidate() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        for budget in [0, 5000] {
            let result = analyze_with_budget(source, None, budget);
            assert_eq!(result.key, None);
            assert_eq!(result.method, "numeric_map_budget_exhausted");
        }
        assert_eq!(extract_shuffle_key(source), Some(9_618_129));
    }

    #[test]
    fn excessive_distinct_anchor_maps_fail_closed() {
        let mut values: Vec<String> = (0..300)
            .map(|n| {
                "4294967295.000000"
                    .chars()
                    .map(|c| match c {
                        '4' => char::from(b'A' + (n % 26) as u8),
                        '2' => char::from(b'a' + (n / 26) as u8),
                        other => other,
                    })
                    .collect()
            })
            .collect();
        values.extend(["8388607.000000".into(), "65535.000000".into()]);
        let source = format!("var values={};", serde_json::to_string(&values).unwrap());
        let result = analyze(&source);
        assert_eq!(result.key, None);
        assert_eq!(result.method, "numeric_map_budget_exhausted");
    }
    #[test]
    fn unrelated_annex_references_and_repeated_initializers_keep_valid_key() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let changed = format!(
            "{{function x(){{}}}}{}{source}{}var pair=[0,k];",
            "x;".repeat(2048),
            "var k=0;".repeat(256)
        );
        assert_eq!(extract_shuffle_key(&changed), Some(9_618_129));
    }
    #[test]
    fn deeply_parenthesized_key_lookup_remains_supported() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let expression = format!("{}parseInt(pool[index]){}", "(".repeat(64), ")".repeat(64));
        assert_eq!(
            extract_shuffle_key(&source.replace("parseInt(pool[index])", &expression)),
            Some(9_618_129)
        );
    }
    #[test]
    fn nested_sequence_function_initializers_keep_bounded_plans() {
        let mut nested = "0;".repeat(4096);
        for _ in 0..12 {
            nested = format!("var a=(0,function(){{{nested}}});var p=[0,a];");
        }
        assert_eq!(extract_shuffle_key(&nested), None);
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        assert_eq!(
            extract_shuffle_key(&format!("{source}{nested}")),
            Some(9_618_129)
        );
    }
    #[test]
    fn lookup_plan_budget_exhaustion_rejects_partial_results() {
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let result = analyze_with_limits(source, None, NUMERIC_MAP_WORK_BUDGET, 0);
        assert_eq!(result.key, None);
        assert_eq!(result.method, "initializer_plan_budget_exhausted");
        assert_eq!(extract_shuffle_key(source), Some(9_618_129));
    }

    #[test]
    fn overlapping_initializer_work_hits_default_plan_limit() {
        let mut nested = "0;".repeat(32768);
        for _ in 0..40 {
            nested = format!("var a=(0,function(){{{nested}}});var p=[0,a];");
        }
        let source = include_str!("fixtures/homedepot_491_split_numeric.js");
        let result = analyze(&format!("{source}{nested}"));
        assert_eq!(result.key, None);
        assert_eq!(result.method, "initializer_plan_budget_exhausted");
    }

    const ROTATED: &str = include_str!("fixtures/homedepot_rotated_global_conversion.js");
    const ROTATED_KEY: &str = "var keyValue=";

    #[test]
    fn rotated_global_conversion_chain_reads_the_bound_pool_element() {
        let result = analyze(ROTATED);
        assert_eq!(result.key, Some(9_618_129));
        assert_eq!(result.method, "opaque_global_conversion");
        assert_eq!(
            result.candidates["conversion_initializers"],
            vec![9_618_129]
        );
        assert!(result.candidates["bound_initializers"].is_empty());
    }

    #[test]
    fn rotated_layout_survives_renaming_reordering_and_index_rotation() {
        // Rename every identifier, move the key to another pool slot with a
        // different write-once index expression, and swap the alias order.
        let renamed = [
            ("gAlias", "zM"),
            ("cOne", "z9"),
            ("cTwo", "Sl"),
            ("cIdx", "Z9"),
            ("numPool", "xL"),
            ("tblRead", "qV"),
            ("tblKeys", "R8f"),
            ("tblNames", "Z5"),
            ("tblHelp", "sK"),
            ("keyValue", "VPf"),
            ("keyPair", "znf"),
            ("useKey", "cnf"),
            ("cmpSame", "Ax"),
            ("session", "Lgf"),
        ]
        .iter()
        .fold(ROTATED.to_owned(), |s, (a, b)| s.replace(a, b));
        assert_eq!(extract_shuffle_key(&renamed), Some(9_618_129));
        let moved = renamed
            .replace(
                "[\"noise\",\"JbBmBvJ\"]",
                "[\"x9\",\"noise\",\"y7\",\"JbBmBvJ\"]",
            )
            .replace("Z9=Sl-z9;", "Z9=Sl*Sl-z9;");
        assert_eq!(extract_shuffle_key(&moved), Some(9_618_129));
        let swapped = moved.replace(
            "if(typeof window!=='undefined'){zM=window;}else if(typeof global!=='undefined'){zM=global;}else{zM=this;}",
            "zM=typeof globalThis!=='undefined'?globalThis:this;",
        );
        // A conditional right-hand side is not a plain global-object write.
        assert_eq!(extract_shuffle_key(&swapped), None);
        let self_alias = moved.replace(
            "if(typeof window!=='undefined'){zM=window;}else if(typeof global!=='undefined'){zM=global;}else{zM=this;}",
            "if(typeof self!=='undefined'){zM=self;}else{zM=globalThis;}",
        );
        assert_eq!(extract_shuffle_key(&self_alias), Some(9_618_129));
        // Nested writes into objects reached through the global do not
        // replace the alias's own properties.
        let nested = moved.replacen(
            "return cnf;}",
            "zM[z9][Sl]=z9;zM[z9].bmak[Sl]=z9;return cnf;}",
            1,
        );
        assert_ne!(nested, moved);
        assert_eq!(extract_shuffle_key(&nested), Some(9_618_129));
    }

    #[test]
    fn rotated_conversion_rejects_transformed_or_unverifiable_shapes() {
        let key = ROTATED.find(ROTATED_KEY).unwrap() + ROTATED_KEY.len();
        let end = ROTATED[key..].find(";return useKey").unwrap() + key;
        let initializer = &ROTATED[key..end];
        for (label, changed) in [
            ("arithmetic around the chain", format!("{initializer}+1")),
            ("unary around the chain", format!("-{initializer}")),
            (
                "extra outer argument",
                initializer.replacen("](tblHelp()", "](0,tblHelp()", 1),
            ),
            (
                "two-argument decoy",
                initializer.replace("(numPool[cIdx]))", "(numPool[cIdx]),cOne)"),
            ),
            (
                "table index instead of value",
                initializer.replace("(numPool[cIdx])", "(lookup[numPool[cIdx]])"),
            ),
            (
                "name reads the pool",
                initializer.replacen("tblRead(cOne)", "numPool[cOne]", 1),
            ),
            (
                "name has a side effect",
                initializer.replacen("tblRead(cOne)", "(cOne=cTwo)", 1),
            ),
            (
                "name defines a function",
                initializer.replacen("tblRead(cOne)", "function(){}", 1),
            ),
            (
                "non-global receiver",
                initializer.replacen("gAlias[", "local[", 1),
            ),
        ] {
            let source = ROTATED.replacen(initializer, &changed, 1);
            assert_eq!(extract_shuffle_key(&source), None, "{label}");
        }
    }

    #[test]
    fn rotated_conversion_requires_write_once_global_facts() {
        for (label, from, to) in [
            (
                "index written twice",
                "cIdx=cTwo-cOne;",
                "cIdx=cTwo-cOne;cIdx=cOne;",
            ),
            (
                "index compound write",
                "cIdx=cTwo-cOne;",
                "cIdx=cTwo;cIdx-=cOne;",
            ),
            ("index updated", "cIdx=cTwo-cOne;", "cIdx=cTwo-cOne;cIdx++;"),
            (
                "index loop target",
                "cIdx=cTwo-cOne;",
                "cIdx=cTwo-cOne;for(cIdx in power){}",
            ),
            (
                "pool written twice",
                "numPool=decode",
                "numPool=[];numPool=decode",
            ),
            (
                "pool escapes",
                "return useKey;}",
                "consume(numPool);return useKey;}",
            ),
            (
                "pool member write",
                "return useKey;}",
                "numPool[cOne]='JbBmBvJ';return useKey;}",
            ),
            (
                "alias also assigned elsewhere",
                "gAlias=this;}",
                "gAlias=this;}gAlias=other;",
            ),
            (
                "alias member written",
                "return useKey;}",
                "gAlias[cOne]=cTwo;return useKey;}",
            ),
            (
                "alias member deleted",
                "return useKey;}",
                "delete gAlias[cOne];return useKey;}",
            ),
            (
                "alias replaced on global",
                "return useKey;}",
                "window.gAlias=cOne;return useKey;}",
            ),
            (
                "index declared with initializer",
                "var gAlias,cOne,cTwo,cIdx,numPool;",
                "var gAlias,cOne,cTwo,numPool;var cIdx=0;",
            ),
        ] {
            let source = ROTATED.replacen(from, to, 1);
            assert_ne!(source, ROTATED, "{label}");
            assert_eq!(extract_shuffle_key(&source), None, "{label}");
        }
    }

    #[test]
    fn rotated_conversion_key_binding_order_and_stability() {
        // A straight-line read of the key before its initializer, in the same
        // function, is not a deferred closure read.
        let straight = ROTATED.replacen(
            "var useKey=function(){var keyPair=[session,keyValue];return keyPair;};",
            "var keyPair=[session,keyValue];var useKey=function(){return keyPair;};",
            1,
        );
        assert_eq!(extract_shuffle_key(&straight), None);
        let reassigned = ROTATED.replacen(";return useKey;}", ";keyValue=cOne;return useKey;}", 1);
        assert_eq!(extract_shuffle_key(&reassigned), None);
        // The proven bound-initializer route keeps its strict ordering rule.
        let proven = ROTATED.replacen(
            ROTATED_KEY,
            "var keyValue=parseInt(numPool[cIdx]);var unused=",
            1,
        );
        assert_ne!(analyze(&proven).method, "unique_bound_initializer");
    }

    #[test]
    fn rotated_conversion_candidates_must_agree() {
        let key = ROTATED.find(ROTATED_KEY).unwrap() + ROTATED_KEY.len();
        let end = ROTATED[key..].find(";return useKey").unwrap() + key;
        let initializer = &ROTATED[key..end];
        // A second decodable element ("bJBmBvJ") read by a second chain.
        let second = format!(
            ";var otherValue={};var otherPair=[session,otherValue]",
            initializer.replace("numPool[cIdx]", "numPool[0]")
        );
        let ambiguous = ROTATED.replace("\"noise\"", "\"bJBmBvJ\"").replacen(
            ";return useKey",
            &format!("{second};return useKey"),
            1,
        );
        assert_eq!(extract_shuffle_key(&ambiguous), None);
        assert_eq!(
            analyze(&ambiguous).method,
            "ambiguous_conversion_initializers"
        );
        let agreeing = ambiguous.replace("numPool[0]", "numPool[cIdx]");
        assert_eq!(extract_shuffle_key(&agreeing), Some(9_618_129));
        // A single registered getter pool must agree with the conversion.
        let registered = include_str!("fixtures/homedepot_5d7_short_registration.js");
        let conflicting = analyze(&format!("{registered}\n{ROTATED}"));
        assert_eq!(conflicting.key, None);
    }

    #[test]
    fn rotated_constant_arrays_are_not_registrations() {
        // An immediately-invoked constant array with the u32 anchor at index 1
        // and no multiplier after it is a rotated constant table.
        let anchors = "var power=[\"BNNN\"];var anchors_a=[\"3vJ3JbHvJM7NNNNNN\"];var anchors_b=[\"mrmmbNH7NNNNNN\", \"bMMrM7NNNNNN\"];";
        let table = format!(
            "{anchors}(function(t){{return t;}}([\"J\",\"3vJ3JbHvJM7NNNNNN\",\"mrmmbNH7NNNNNN\"]));"
        );
        let result = analyze(&format!("{table}\n{ROTATED}"));
        assert_eq!(result.key, Some(9_618_129));
        // A head shifted by one slot before the multiplier still fails closed.
        let source = include_str!("fixtures/homedepot_5d7_short_registration.js");
        assert_eq!(
            analyze(&source.replace("}([", "}(['bad',")).method,
            "malformed_registered_pool"
        );
    }
}

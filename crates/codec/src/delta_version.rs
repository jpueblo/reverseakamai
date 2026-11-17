//! Static support for the numeric delta-string layout in recovered bmak builds.
//! This is a constrained recognizer, not a JavaScript interpreter. It reads only
//! integer expressions, unary-minus helpers, tables, and numeric stack seeds.
use super::literal_arrays::tokens;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
enum Expr {
    Num(i64),
    Array,
    Object,
    Ref(String),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Copy)]
enum Value {
    Num(i64),
    Array,
    Object,
}
impl Value {
    fn number(self) -> i64 {
        match self {
            Self::Num(n) => n,
            Self::Array | Self::Object => 0,
        }
    }
    fn truth(self) -> bool {
        match self {
            Self::Num(n) => n != 0,
            Self::Array | Self::Object => true,
        }
    }
}
impl Expr {
    fn value(&self, k: &BTreeMap<String, i64>) -> Option<Value> {
        Some(match self {
            Self::Num(n) => Value::Num(*n),
            Self::Array => Value::Array,
            Self::Object => Value::Object,
            Self::Ref(n) => Value::Num(*k.get(n)?),
            Self::Unary(op, a) => {
                let a = a.value(k)?;
                if matches!(a, Value::Object) && op != "!" {
                    return None;
                }
                Value::Num(match op.as_str() {
                    "+" => a.number(),
                    "-" => a.number().checked_neg()?,
                    "!" => i64::from(!a.truth()),
                    _ => return None,
                })
            }
            Self::Binary(op, a, b) => {
                let av = a.value(k)?;
                let bv = b.value(k)?;
                if matches!(av, Value::Object) || matches!(bv, Value::Object) {
                    return None;
                }
                if op == "+" && (matches!(av, Value::Array) || matches!(bv, Value::Array)) {
                    return None;
                }
                let a = av.number();
                let b = bv.number();
                Value::Num(match op.as_str() {
                    "+" => a.checked_add(b)?,
                    "-" => a.checked_sub(b)?,
                    "*" => a.checked_mul(b)?,
                    "/" if b != 0 && a.checked_rem(b)? == 0 => a.checked_div(b)?,
                    "%" => a.checked_rem(b)?,
                    _ => return None,
                })
            }
        })
    }
}
fn ident(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().enumerate().all(|(i, c)| {
            c.is_ascii_alphabetic() || c == b'_' || c == b'$' || (i > 0 && c.is_ascii_digit())
        })
}
struct Parser<'a, 'b> {
    t: &'a [&'a str],
    i: usize,
    minus: &'b BTreeSet<String>,
    budget: usize,
}
impl Parser<'_, '_> {
    fn expr(&mut self, precedence: u8, depth: usize) -> Option<Expr> {
        if depth > 32 || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        let token = *self.t.get(self.i)?;
        self.i += 1;
        let mut left = match token {
            "+" | "-" | "!" => Expr::Unary(token.into(), Box::new(self.expr(3, depth + 1)?)),
            "(" => {
                let e = self.expr(0, depth + 1)?;
                if self.t.get(self.i) != Some(&")") {
                    return None;
                }
                self.i += 1;
                e
            }
            "[" if self.t.get(self.i) == Some(&"]") => {
                self.i += 1;
                Expr::Array
            }
            "{" if self.t.get(self.i) == Some(&"}") => {
                self.i += 1;
                Expr::Object
            }
            s if (self.minus.contains(s) || self.minus.contains(&format!("!{s}")))
                && self.t.get(self.i) == Some(&"(") =>
            {
                self.i += 1;
                let e = self.expr(0, depth + 1)?;
                if self.t.get(self.i) != Some(&")") {
                    return None;
                }
                self.i += 1;
                Expr::Unary(
                    if self.minus.contains(s) { "-" } else { "!" }.into(),
                    Box::new(e),
                )
            }
            s if ident(s) => Expr::Ref(s.into()),
            s => Expr::Num(s.parse().ok()?),
        };
        loop {
            let Some(&op) = self.t.get(self.i) else { break };
            let p = match op {
                "+" | "-" => 1,
                "*" | "/" | "%" => 2,
                _ => 0,
            };
            if p == 0 || p <= precedence {
                break;
            }
            self.i += 1;
            left = Expr::Binary(
                op.into(),
                Box::new(left),
                Box::new(self.expr(p, depth + 1)?),
            );
        }
        Some(left)
    }
}
fn expression(t: &[&str], i: usize, minus: &BTreeSet<String>) -> Option<(Expr, usize)> {
    let mut p = Parser {
        t,
        i,
        minus,
        budget: 256,
    };
    let e = p.expr(0, 0)?;
    Some((e, p.i))
}
fn constants(t: &[&str], minus: &BTreeSet<String>) -> BTreeMap<String, i64> {
    let mut assignments: BTreeMap<String, Vec<Expr>> = BTreeMap::new();
    let mut blocked = BTreeSet::new();
    // The supported layout initializes numbers in the outer script or its
    // direct dispatchers. Embedded programs have independent, reused names.
    let mut opens = BTreeSet::new();
    for (i, token) in t.iter().enumerate() {
        if *token != "function" {
            continue;
        }
        let mut j = i + 1;
        if t.get(j).is_some_and(|s| ident(s)) {
            j += 1
        }
        if t.get(j) != Some(&"(") {
            continue;
        }
        while j < t.len() && t[j] != ")" {
            j += 1
        }
        if t.get(j + 1) == Some(&"{") {
            opens.insert(j + 1);
        }
    }
    let mut braces = Vec::new();
    let mut depth = 0;
    for i in 0..t.len().saturating_sub(2) {
        if t[i] == "{" {
            let f = opens.contains(&i);
            braces.push(f);
            if f {
                depth += 1
            }
        }
        if t[i] == "}" && braces.pop() == Some(true) {
            depth -= 1
        }
        if depth > 2 || !ident(t[i]) || (i > 0 && t[i - 1] == ".") {
            continue;
        }
        if matches!(t[i + 1], "+=" | "-=" | "++" | "--" | "*=" | "/=") {
            blocked.insert(t[i].to_owned());
            continue;
        }
        if t[i + 1] != "=" {
            continue;
        }
        if let Some((e, end)) = expression(t, i + 2, minus) {
            if matches!(t.get(end), Some(&";" | &",")) {
                assignments.entry(t[i].into()).or_default().push(e);
                continue;
            }
        }
        blocked.insert(t[i].to_owned());
    }
    let mut known = BTreeMap::new();
    for _ in 0..32 {
        let mut added = false;
        for (name, exprs) in &assignments {
            if blocked.contains(name) || known.contains_key(name) {
                continue;
            }
            let values: Option<Vec<_>> = exprs
                .iter()
                .map(|e| match e.value(&known)? {
                    Value::Num(n) => Some(n),
                    Value::Array | Value::Object => None,
                })
                .collect();
            if let Some(values) = values {
                if values.iter().all(|n| *n == values[0]) {
                    known.insert(name.clone(), values[0]);
                    added = true
                } else {
                    blocked.insert(name.clone());
                }
            }
        }
        if !added {
            break;
        }
    }
    known
}
fn array(
    t: &[&str],
    start: usize,
    k: &BTreeMap<String, i64>,
    minus: &BTreeSet<String>,
    depth: usize,
) -> Option<(Vec<Option<i64>>, usize)> {
    if depth > 32 || t.get(start) != Some(&"[") {
        return None;
    }
    let mut a = Vec::new();
    let mut i = start + 1;
    while t.get(i) != Some(&"]") {
        if a.len() > 100_000 {
            return None;
        }
        if t.get(i) == Some(&"[") {
            let (_, end) = array(t, i, k, minus, depth + 1)?;
            i = end;
            a.push(None)
        } else {
            let (e, end) = expression(t, i, minus)?;
            a.push(Some(e.value(k)?.number()));
            i = end
        }
        if t.get(i) == Some(&",") {
            i += 1
        } else if t.get(i) != Some(&"]") {
            return None;
        }
    }
    Some((a, i + 1))
}
fn decode(table: &[Option<i64>], start: i64, seed: i64) -> Option<String> {
    let start = usize::try_from(start).ok()?;
    let a = table.get(start..start.checked_add(44)?)?;
    let mut code = seed;
    let mut out = String::new();
    for (i, delta) in a.iter().enumerate() {
        let c = u8::try_from(code).ok()?;
        if (i < 43 && !(c.is_ascii_alphanumeric() || c == b'+' || c == b'/'))
            || (i == 43 && c != b'=')
        {
            return None;
        }
        out.push(char::from(c));
        code = code.checked_add((*delta)?)?;
    }
    // A canonical 32-byte Base64 value ends with two zero padding bits.
    let c = out.as_bytes()[42];
    let sextet = match c {
        b'A'..=b'Z' => c - b'A',
        b'a'..=b'z' => c - b'a' + 26,
        b'0'..=b'9' => c - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        _ => return None,
    };
    (sextet % 4 == 0).then_some(out)
}
/// Pure arithmetic constants from supported outer/direct dispatcher scopes.
pub(super) fn integer_constants(t: &[&str]) -> BTreeMap<String, i64> {
    let mut minus = BTreeSet::new();
    for w in t.windows(12) {
        if ident(w[0])
            && w[1..4] == ["=", "function", "("]
            && ident(w[4])
            && w[5..9] == [")", "{", "return", "-"]
            && w[9] == w[4]
            && w[10..12] == [";", "}"]
        {
            minus.insert(w[0].into());
        }
    }
    constants(t, &minus)
}

struct DecoderFunction<'a> {
    name: &'a str,
    params: Vec<&'a str>,
    begin: usize,
    end: usize,
}
fn decoder_functions<'a>(t: &[&'a str]) -> Vec<DecoderFunction<'a>> {
    let mut functions = Vec::new();
    for i in 0..t.len() {
        if t[i] != "function" {
            continue;
        }
        let assigned = if i >= 2 && t[i - 1] == "=" && ident(t[i - 2]) {
            Some(t[i - 2])
        } else {
            None
        };
        let (name, open) = if t.get(i + 1) == Some(&"(") {
            let Some(name) = assigned else { continue };
            (name, i + 1)
        } else if t.get(i + 2) == Some(&"(") && t.get(i + 1).is_some_and(|v| ident(v)) {
            (assigned.unwrap_or(t[i + 1]), i + 2)
        } else {
            continue;
        };
        let Some(close) = (open + 1..t.len()).find(|&j| t[j] == ")") else {
            continue;
        };
        let parts = &t[open + 1..close];
        if parts.is_empty()
            || !parts
                .iter()
                .enumerate()
                .all(|(j, v)| if j % 2 == 0 { ident(v) } else { *v == "," })
        {
            continue;
        }
        if t.get(close + 1) != Some(&"{") {
            continue;
        }
        let mut level = 1i32;
        let mut end = close + 2;
        while end < t.len() {
            match t[end] {
                "{" => level += 1,
                "}" => level -= 1,
                _ => {}
            }
            if level == 0 {
                break;
            }
            end += 1;
        }
        if end == t.len() {
            continue;
        }
        functions.push(DecoderFunction {
            name,
            params: parts.iter().step_by(2).copied().collect(),
            begin: close + 2,
            end,
        });
    }
    functions
}

pub(super) fn extract(source: &str) -> Option<String> {
    let t = tokens(source);
    let functions = decoder_functions(&t);
    let mut minus = BTreeSet::new();
    let mut subtract = BTreeSet::new();
    for w in t.windows(12) {
        if ident(w[0])
            && w[1..4] == ["=", "function", "("]
            && ident(w[4])
            && w[5..9] == [")", "{", "return", "-"]
            && w[9] == w[4]
            && w[10..12] == [";", "}"]
        {
            minus.insert(w[0].into());
        }
    }
    for w in t.windows(15) {
        if ident(w[0])
            && w[1..4] == ["=", "function", "("]
            && w[5] == ","
            && w[7..10] == [")", "{", "return"]
            && w[10] == w[4]
            && w[11] == "-"
            && w[12] == w[6]
            && w[13..15] == [";", "}"]
        {
            subtract.insert(w[0]);
        }
    }
    for w in t.windows(12) {
        if ident(w[0])
            && w[1..4] == ["=", "function", "("]
            && ident(w[4])
            && w[5..9] == [")", "{", "return", "!"]
            && w[9] == w[4]
            && w[10..12] == [";", "}"]
        {
            minus.insert(format!("!{}", w[0]));
        }
    }
    let known = constants(&t, &minus);
    let mut tables = Vec::new();
    let allow_legacy = super::verified_legacy_layout(source);
    let mut bound_tables = Vec::new();
    for i in 0..t.len().saturating_sub(2) {
        let (array_start, owners) = if ident(t[i]) && t[i + 1] == "=" && t[i + 2] == "[" {
            (i + 2, vec![t[i]])
        } else if t[i] == "return" && t[i + 1] == "[" {
            let label = t[..i]
                .windows(3)
                .rev()
                .find(|w| w[0] == "case" && w[2] == ":")
                .map(|w| w[1]);
            let Some(label) = label else { continue };
            let owners = t
                .windows(9)
                .filter(|w| {
                    ident(w[0])
                        && w[1] == "="
                        && w[3] == "("
                        && w[4] == label
                        && w[5..9] == [",", "[", "]", ")"]
                })
                .map(|w| w[0])
                .collect();
            (i + 1, owners)
        } else {
            continue;
        };
        let Some((a, _)) = array(&t, array_start, &known, &minus, 0) else {
            continue;
        };
        if a.len() >= 44 {
            for owner in owners {
                bound_tables.push((owner, a.clone()));
            }
        }
    }
    // The historical full build uses dispatcher wrappers that are outside the
    // general function-binding recognizer. Retain its statically decoded table
    // and stack only after exact source verification; never on unknown scripts.
    let mut legacy_tables = Vec::new();
    if allow_legacy {
        for (owner, a) in &bound_tables {
            for fallback in t
                .windows(4)
                .filter(|w| w[1] == "=" && w[2] == *owner && w[3] == ";")
            {
                for step in t.windows(7).filter(|w| {
                    w[1] == "+=" && w[2] == fallback[0] && w[3] == "[" && w[5..7] == ["]", ";"]
                }) {
                    for seed in t.windows(19) {
                        if seed[0] == step[0]
                            && seed[1] == "="
                            && subtract.contains(seed[2])
                            && seed[3] == "("
                            && seed[5] == ","
                            && seed[7] == "["
                            && seed[8] == seed[2]
                            && seed[9] == "("
                            && seed[10] == seed[6]
                            && seed[11..14] == [".", "length", ","]
                            && known.get(seed[14]) == Some(&1)
                            && seed[15..19] == [")", "]", ")", ";"]
                        {
                            legacy_tables.push((a.clone(), seed[6]));
                        }
                    }
                }
            }
        }
    }
    for (owner, a) in bound_tables {
        // Bind each table to its delta accumulator and seed stack. Arbitrary
        // arrays and unrelated push() calls cannot serve as decoders.
        for function in &functions {
            if functions.iter().filter(|f| f.name == function.name).count() != 1 {
                continue;
            }
            let body = &t[function.begin..function.end];
            for fallback in body
                .windows(4)
                .filter(|w| w[1] == "=" && w[2] == owner && w[3] == ";")
            {
                for step in body.windows(7).filter(|w| {
                    w[1] == "+=" && w[2] == fallback[0] && w[3] == "[" && w[5..7] == ["]", ";"]
                }) {
                    for seed in body.windows(19) {
                        if seed[0] != step[0]
                            || seed[1] != "="
                            || !subtract.contains(seed[2])
                            || seed[3] != "("
                            || seed[5] != ","
                            || seed[7] != "["
                            || seed[8] != seed[2]
                            || seed[9] != "("
                            || seed[10] != seed[6]
                            || seed[11..14] != [".", "length", ","]
                            || known.get(seed[14]) != Some(&1)
                            || seed[15..19] != [")", "]", ")", ";"]
                        {
                            continue;
                        }
                        let Some(initial_arg) = function.params.iter().position(|p| *p == seed[4])
                        else {
                            continue;
                        };
                        let Some(index_arg) = function.params.iter().position(|p| *p == step[4])
                        else {
                            continue;
                        };
                        for length in body
                            .windows(7)
                            .filter(|w| w[..2] == ["while", "("] && w[3..] == ["--", ">", "0", ")"])
                        {
                            let Some(length_arg) =
                                function.params.iter().position(|p| *p == length[2])
                            else {
                                continue;
                            };
                            tables.push((
                                a.clone(),
                                seed[6],
                                function.name,
                                function.end,
                                initial_arg,
                                index_arg,
                                length_arg,
                                function.params.len(),
                            ));
                        }
                    }
                }
            }
        }
    }
    let mut scope: Vec<Option<(&str, Option<i64>)>> = vec![None];
    let mut candidates = BTreeSet::new();
    for i in 0..t.len() {
        match t[i] {
            "{" => scope.push(None),
            "}" => {
                scope.pop();
                if scope.is_empty() {
                    scope.push(None)
                }
            }
            _ => {}
        }
        if i > 0 && t.get(i..i + 4) == Some(&[".", "pop", "(", ")"]) {
            *scope.last_mut()? = Some((t[i - 1], None));
        }
        if i > 0 && t.get(i..i + 3) == Some(&[".", "push", "("]) {
            *scope.last_mut()? = Some((t[i - 1], None));
            if let Some((e, end)) = expression(&t, i + 3, &minus) {
                if t.get(end) == Some(&")") {
                    *scope.last_mut()? = Some((t[i - 1], e.value(&known).map(Value::number)));
                }
            }
        }
        if t.get(i..i + 6) != Some(&[".", "apply", "(", "null", ",", "["]) {
            continue;
        }
        let Some((args, end)) = array(&t, i + 5, &known, &minus, 0) else {
            continue;
        };
        if t.get(end) != Some(&")") {
            continue;
        }
        let Some((stack, Some(seed))) = scope.iter().rev().find_map(|n| *n) else {
            continue;
        };
        if allow_legacy {
            if let [Some(44), Some(initial), Some(index), _] = args.as_slice() {
                if let Some(initial) = initial.checked_sub(seed) {
                    for (table, decoder_stack) in &legacy_tables {
                        if *decoder_stack == stack {
                            if let Some(token) = decode(table, *index, initial) {
                                candidates.insert(token);
                            }
                        }
                    }
                }
            }
        }
        for (
            table,
            decoder_stack,
            callee,
            definition_end,
            initial_arg,
            index_arg,
            length_arg,
            arity,
        ) in &tables
        {
            if i == 0
                || t[i - 1] != *callee
                || (i >= 2 && t[i - 2] == ".")
                || i <= *definition_end
                || *decoder_stack != stack
                || args.len() != *arity
            {
                continue;
            }
            // Reassignment would invalidate the retained function identity.
            if t[*definition_end..i]
                .windows(2)
                .any(|w| w[0] == *callee && w[1] == "=")
            {
                continue;
            }
            if args[*length_arg] != Some(44) {
                continue;
            }
            let (Some(initial), Some(start)) = (args[*initial_arg], args[*index_arg]) else {
                continue;
            };
            let Some(initial) = initial.checked_sub(seed) else {
                continue;
            };
            if let Some(token) = decode(table, start, initial) {
                candidates.insert(token);
            }
        }
    }
    if candidates.len() == 1 {
        candidates.into_iter().next()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const FIXTURE: &str = include_str!("fixtures/homedepot_version_delta.js");
    const TOKEN: &str = "O91KTKjRETUBo+9luCR4019FedhiMI42HlCgTUnpxQ0=";
    #[test]
    fn fresh_returned_delta_table_with_reordered_wrapper_arguments() {
        let source = include_str!("fixtures/homedepot_fresh_return_delta.js");
        assert_eq!(
            extract(source).as_deref(),
            Some("vD40pxY+STMfPT/VUMjl+Z5rRUfb2ud56hOLQuAHHnI=")
        );
        assert_eq!(
            extract(&source.replace("table=factory(state,[])", "table=factory(unknown,[])")),
            None
        );
        assert_eq!(extract(&source.replace("seed=739", "seed=740")), None);
    }

    #[test]
    fn extracts_recovered_delta_version_with_renamed_symbols_and_whitespace() {
        assert_eq!(extract(FIXTURE).as_deref(), Some(TOKEN));
        let renamed = FIXTURE
            .replace("negative", "minus_number")
            .replace("stack", "seeds")
            .replace("table", "deltas");
        assert_eq!(extract(&renamed).as_deref(), Some(TOKEN));
    }
    #[test]
    fn rejects_wrong_seed_unknown_values_references_and_truncated_tables() {
        for source in [
            FIXTURE.replace("seed=866", "seed=867"),
            FIXTURE.replace("negative(22)", "unknown(22)"),
            FIXTURE.replace("negative(22)", "[22]"),
            FIXTURE.replace(",negative(65)]", "]"),
            FIXTURE.replace("code+=data[index]", "code-=data[index]"),
            FIXTURE.replace("stack.push(seed)", "unrelated.push(seed)"),
            FIXTURE.replace("stack.push(seed);", "stack.push(seed);stack.pop();"),
            FIXTURE.replace(
                "stack.push(seed);",
                "stack.push(seed);stack.push(dynamic());",
            ),
        ] {
            assert_eq!(extract(&source), None);
        }
    }
    #[test]
    fn ignores_literal_comment_regex_decoys_and_rejects_conflicting_decoders() {
        let decoys = format!(
            "/* {FIXTURE} */\nconst text={:?};const regex=/stack.push(seed)/;",
            FIXTURE
        );
        assert_eq!(extract(&decoys), None);
        let other = FIXTURE.replace("negative(22),negative(8)", "negative(23),negative(7)");
        assert!(extract(&other).is_some());
        assert_eq!(extract(&format!("{FIXTURE}{{{other}}}")), None);
    }
    #[test]
    fn bounds_untrusted_expression_and_array_recursion() {
        let nested = format!("a={}0{};", "[".repeat(100), "]".repeat(100));
        assert_eq!(extract(&nested), None);
        assert_eq!(extract(&format!("a={}1;", "!".repeat(100))), None);
        assert_eq!(extract(&format!("a={}1;", "1+".repeat(1000))), None);
        assert_eq!(extract("const Ã¦â€”Â¥Ã¦Å“Â¬Ã¨ÂªÅ¾ = 1;"), None);
    }
    #[test]
    fn captured_four_argument_version_lookup() {
        let source = include_str!("fixtures/homedepot_217bb3_four_args.js");
        assert_eq!(
            extract(source).as_deref(),
            Some("gyC8AZNPtjoZhpueCDw8o59DmLPOz/qFpTY1O/mhanE=")
        );
        let renamed = source.replace("stack", "seeds").replace("table", "deltas");
        assert_eq!(extract(&renamed), extract(source));
    }
    #[test]
    fn binds_apply_callee_and_argument_positions_from_copilot_review() {
        assert!(extract(FIXTURE).is_some());
        let unrelated = FIXTURE.replace("decoder.apply", "unrelated.apply")
            + "function unrelated(){return 'not a version';}";
        assert_eq!(extract(&unrelated), None);
        assert_eq!(
            extract(&FIXTURE.replace("decoder.apply", "object.decoder.apply")),
            None
        );
        assert_eq!(
            extract(&FIXTURE.replace(
                "var version=decoder.apply",
                "decoder=unrelated;var version=decoder.apply"
            )),
            None
        );
        assert_eq!(
            extract(&FIXTURE.replace("[size,0,0,initial,0,start]", "[initial,0,0,size,0,start]")),
            None
        );
        assert_eq!(
            extract(&FIXTURE.replace("[size,0,0,initial,0,start]", "[size,initial,0,0,0,start]")),
            None
        );
        let renamed = FIXTURE.replace("decoder", "renamed_decoder");
        assert_eq!(extract(&renamed), extract(FIXTURE));
    }
    #[test]
    fn rejects_invalid_four_argument_layout_or_seed() {
        let source = include_str!("fixtures/homedepot_217bb3_four_args.js");
        for broken in [
            source.replace("seed=596", "seed=597"),
            source.replace("initial=699", "initial=700"),
            source.replace("count=44", "count=43"),
            source.replace("start=0", "start=1"),
            source.replace("[count,initial,start,0]", "[count,initial,0,start,0]"),
            source.replace("stack.push(seed);", "stack.pop();"),
            source.replace("[count,initial,start,0]", "[count,dynamic(),start,0]"),
            format!("/* {source} */"),
        ] {
            assert_eq!(extract(&broken), None);
        }
    }
}

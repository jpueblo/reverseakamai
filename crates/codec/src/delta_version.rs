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

pub(super) fn extract(source: &str) -> Option<String> {
    let t = tokens(source);
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
    for (owner, a) in bound_tables {
        // Bind each table to its delta accumulator and seed stack. Arbitrary
        // arrays and unrelated push() calls cannot serve as decoders.
        for fallback in t
            .windows(4)
            .filter(|w| w[1] == "=" && w[2] == owner && w[3] == ";")
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
                        tables.push((a.clone(), seed[6]));
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
        if t.get(end) != Some(&")") { continue; }
        // The captured 217bb3 wrapper takes (length, initial, index, unused).
        // Six-argument wrappers retain their original final-index layout.
        let (start, initials): (i64, Vec<i64>) = match args.as_slice() {
            [Some(44), Some(initial), Some(index), _] => (*index, vec![*initial]),
            [_, _, _, _, _, Some(index)] if args.contains(&Some(44)) => (
                *index, args[..5].iter().flatten().copied().filter(|v| *v != 44).collect()
            ),
            _ => continue,
        };
        let Some((stack, Some(seed))) = scope.iter().rev().find_map(|n| *n) else {
            continue;
        };
        for initial in initials {
            let Some(initial) = initial.checked_sub(seed) else {
                continue;
            };
            for (table, decoder_stack) in &tables {
                if *decoder_stack == stack {
                    if let Some(token) = decode(table, start, initial) {
                        candidates.insert(token);
                    }
                }
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
        assert_eq!(extract("const æ—¥æœ¬èªž = 1;"), None);
    }
    #[test]
    fn captured_four_argument_version_lookup() {
        let source=include_str!("fixtures/homedepot_217bb3_four_args.js");
        assert_eq!(extract(source).as_deref(),Some("gyC8AZNPtjoZhpueCDw8o59DmLPOz/qFpTY1O/mhanE="));
        let renamed=source.replace("stack","seeds").replace("table","deltas");
        assert_eq!(extract(&renamed),extract(source));
    }
    #[test]
    fn rejects_invalid_four_argument_layout_or_seed() {
        let source=include_str!("fixtures/homedepot_217bb3_four_args.js");
        for broken in [source.replace("seed=596","seed=597"),
            source.replace("initial=699","initial=700"),
            source.replace("count=44","count=43"),
            source.replace("start=0","start=1"),
            source.replace("[count,initial,start,0]","[count,initial,0,start,0]"),
            source.replace("stack.push(seed);","stack.pop();"),
            source.replace("[count,initial,start,0]","[count,dynamic(),start,0]"),
            format!("/* {source} */"),
        ] { assert_eq!(extract(&broken),None); }
    }

}

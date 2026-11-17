//! Static support for the numeric delta-string layout in recovered bmak builds.
//! This is a constrained recognizer, not a JavaScript interpreter. It reads only
//! integer expressions, unary-minus helpers, tables, and numeric stack seeds.
use super::ast_scope::{Analysis, Numbers};
use super::literal_arrays::tokens;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
const DELTA_CANDIDATE_WORK_BUDGET: usize = 2_000_000;

#[derive(Clone)]
enum Expr {
    Num(i64),
    Array,
    Object,
    Ref(String, usize),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Copy)]
enum Value {
    Num(i64),
    Bool(bool),
    Array,
    Object,
}
impl Value {
    fn number(self) -> Option<i64> {
        match self {
            Self::Num(n) => Some(n),
            Self::Bool(b) => Some(i64::from(b)),
            Self::Array => Some(0),
            Self::Object => None,
        }
    }
    fn truth(self) -> bool {
        match self {
            Self::Num(n) => n != 0,
            Self::Bool(b) => b,
            Self::Array | Self::Object => true,
        }
    }
}
trait NumberLookup {
    fn lookup(&self, name: &str, address: usize) -> Option<i64>;
}
impl NumberLookup for BTreeMap<String, i64> {
    fn lookup(&self, name: &str, _: usize) -> Option<i64> {
        self.get(name).copied()
    }
}
impl NumberLookup for Numbers {
    fn lookup(&self, name: &str, address: usize) -> Option<i64> {
        self.get_ref(name, address).copied()
    }
}
impl<T: NumberLookup + ?Sized> NumberLookup for &T {
    fn lookup(&self, name: &str, address: usize) -> Option<i64> {
        (**self).lookup(name, address)
    }
}
impl Expr {
    fn value(&self, k: &impl NumberLookup) -> Option<Value> {
        Some(match self {
            Self::Num(n) => Value::Num(*n),
            Self::Array => Value::Array,
            Self::Object => Value::Object,
            Self::Ref(n, at) => Value::Num(k.lookup(n, *at)?),
            Self::Unary(op, a) => {
                let a = a.value(k)?;
                if matches!(a, Value::Object) && op != "!" {
                    return None;
                }
                match op.as_str() {
                    "+" => Value::Num(a.number()?),
                    "-" => Value::Num(a.number()?.checked_neg()?),
                    "!" => Value::Bool(!a.truth()),
                    _ => return None,
                }
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
                let a = av.number()?;
                let b = bv.number()?;
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
            s if ident(s) => Expr::Ref(s.into(), s.as_ptr() as usize),
            s => Expr::Num(super::literal_arrays::decimal_integer(s)?),
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
fn legacy_constants(t: &[&str], minus: &BTreeSet<String>) -> BTreeMap<String, i64> {
    let mut assignments: BTreeMap<String, Vec<Expr>> = BTreeMap::new();
    let mutations = super::literal_arrays::legacy_mutation_indices(t);
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
        if mutations.contains(&i) {
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
                    Value::Bool(_) | Value::Array | Value::Object => None,
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
    k: &Numbers,
    minus: &BTreeSet<String>,
    depth: usize,
) -> Option<(Vec<Option<i64>>, usize)> {
    if depth > 32 || t.get(start) != Some(&"[") {
        return None;
    }
    if !k.is_legacy() {
        let values = k.array(t[start])?;
        let mut level = 1usize;
        let mut end = start + 1;
        while end < t.len() {
            match t[end] {
                "[" => level += 1,
                "]" => {
                    level -= 1;
                    if level == 0 {
                        return Some((values.clone(), end + 1));
                    }
                }
                _ => {}
            }
            end += 1;
        }
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
            a.push(match e.value(k)? {
                Value::Num(n) => Some(n),
                _ => None,
            });
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
struct DecoderFunction<'a> {
    name: &'a str,
    params: Vec<&'a str>,
    begin: usize,
    end: usize,
}
fn decoder_functions<'a>(t: &[&'a str], ast: &Analysis, source: &str) -> Vec<DecoderFunction<'a>> {
    let at = |offset: u32| {
        t.partition_point(|token| {
            (token.as_ptr() as usize - source.as_ptr() as usize) < offset as usize
        })
    };
    ast.functions
        .iter()
        .filter_map(|f| {
            let name = *t.get(at(f.name))?;
            let params = f
                .params
                .iter()
                .map(|p| t.get(at(*p)).copied())
                .collect::<Option<Vec<_>>>()?;
            Some(DecoderFunction {
                name,
                params,
                begin: at(f.begin),
                end: at(f.end),
            })
        })
        .collect()
}

fn supported_decoder_body(
    body: &[&str],
    owner: &str,
    data: &str,
    seed: &[&str],
    step: &[&str],
    length: &str,
) -> bool {
    for out in body
        .windows(5)
        .filter(|w| w[0] == "var" && w[2] == "=" && matches!(w[3], "''" | "\"\"") && w[4] == ";")
    {
        let output = out[1];
        let acc = seed[0];
        let sub = seed[2];
        let initial = seed[4];
        let stack = seed[6];
        let one = seed[14];
        let index = step[4];
        let roles = [output, acc, data, index, length, initial, stack];
        if roles.iter().any(|n| !ident(n))
            || roles.iter().copied().collect::<BTreeSet<_>>().len() != roles.len()
        {
            continue;
        }
        let expected = format!(
            "if(typeof {data}==='number'){{{data}={owner};}}var {acc}={sub}({initial},{stack}[{sub}({stack}.length,{one})]);var {output}='';while({length}-->0){{{output}+=String.fromCharCode({acc});{acc}+={data}[{index}];{index}++;}}return {output};"
        );
        let expected = tokens(&expected);
        if expected.len() == body.len()
            && expected.iter().zip(body).all(|(a, b)| {
                a == b || matches!((*a, *b), ("''", "\"\"") | ("'number'", "\"number\""))
            })
        {
            return true;
        }
    }
    false
}

fn direct_return_dispatcher(
    t: &[&str],
    f: &DecoderFunction<'_>,
    array_start: usize,
    array_end: usize,
    label: &str,
) -> bool {
    if f.params.len() != 2 || f.params[0] == f.params[1] {
        return false;
    }
    for braced in [true, false] {
        let prefix = format!(
            "switch({}){{case {label}:{}return",
            f.params[0],
            if braced { "{" } else { "" }
        );
        let mut expected = tokens(&prefix);
        expected.extend_from_slice(&t[array_start..array_end]);
        let suffix = if braced { ";}}" } else { ";}" };
        let end = tokens(suffix);
        expected.extend_from_slice(&end);
        if expected == t[f.begin..f.end] {
            return true;
        }
    }
    false
}

pub(super) fn extract(source: &str) -> Option<String> {
    let ast = if super::verified_legacy_layout(source) {
        Analysis::default()
    } else {
        Analysis::parse(source)?
    };
    extract_with_analysis(source, &ast)
}
pub(super) fn extract_with_analysis(source: &str, ast: &Analysis) -> Option<String> {
    extract_with_budget(source, ast, DELTA_CANDIDATE_WORK_BUDGET)
}
fn extract_with_budget(source: &str, ast: &Analysis, mut candidate_work: usize) -> Option<String> {
    let allow_legacy = super::verified_legacy_layout(source);
    if ast.dynamic || (!allow_legacy && !ast.builtin_methods_intact()) {
        return None;
    }
    let t = tokens(source);
    let functions = decoder_functions(&t, &ast, source);
    let mut minus = BTreeSet::new();
    let mut subtract = BTreeSet::new();
    if allow_legacy {
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
    }
    let legacy_numbers = if allow_legacy {
        Numbers::legacy(legacy_constants(&t, &minus))
    } else {
        Numbers::default()
    };
    let known = if allow_legacy {
        &legacy_numbers
    } else {
        &ast.numbers
    };
    // Stream properly nested function spans instead of rescanning all functions
    // for every returned array. Index potential factory calls by semantic ID.
    let mut function_order: Vec<_> = (0..functions.len()).collect();
    function_order.sort_by_key(|index| {
        (
            functions[*index].begin,
            std::cmp::Reverse(functions[*index].end),
        )
    });
    let mut next_function = 0;
    let mut active_functions: Vec<usize> = Vec::new();
    let mut factory_calls: BTreeMap<_, Vec<(usize, &[&str])>> = BTreeMap::new();
    for (call, w) in t.windows(9).enumerate() {
        if ident(w[0]) && w[1] == "=" && ident(w[2]) && w[3] == "(" {
            if let Some(id) = ast.symbol(w[2]) {
                factory_calls.entry(id).or_default().push((call, w));
            }
        }
    }
    let mut tables: BTreeMap<_, Vec<_>> = BTreeMap::new();
    let mut bound_tables = Vec::new();
    for i in 0..t.len().saturating_sub(2) {
        while active_functions
            .last()
            .is_some_and(|index| functions[*index].end <= i)
        {
            active_functions.pop();
        }
        while next_function < function_order.len()
            && functions[function_order[next_function]].begin <= i
        {
            let index = function_order[next_function];
            next_function += 1;
            while active_functions
                .last()
                .is_some_and(|outer| functions[*outer].end <= functions[index].begin)
            {
                active_functions.pop();
            }
            if functions[index].end > i {
                active_functions.push(index);
            }
        }

        let (array_start, owners) = if ident(t[i]) && t[i + 1] == "=" && t[i + 2] == "[" {
            (i + 2, vec![t[i]])
        } else if t[i] == "return" && t[i + 1] == "[" {
            let Some(dispatcher) = active_functions.last().map(|index| &functions[*index]) else {
                continue;
            };
            // A supported dispatcher has its one return immediately after
            // switch(param){case label:[{]. No growing prefix scan is needed.
            if dispatcher.params.len() != 2
                || !matches!(i.checked_sub(dispatcher.begin), Some(8 | 9))
                || t.get(dispatcher.begin..dispatcher.begin + 6)
                    != Some(&["switch", "(", dispatcher.params[0], ")", "{", "case"])
                || t.get(dispatcher.begin + 7) != Some(&":")
            {
                continue;
            }
            let Some(label) = t.get(dispatcher.begin + 6).copied() else {
                continue;
            };
            let Some((_, literal_end)) = array(&t, i + 1, &known, &minus, 0) else {
                continue;
            };
            if !direct_return_dispatcher(&t, dispatcher, i + 1, literal_end, label)
                || !ast.stable(dispatcher.name)
            {
                continue;
            }
            let owners = ast
                .symbol(dispatcher.name)
                .and_then(|id| factory_calls.get(&id))
                .into_iter()
                .flatten()
                .copied()
                .filter(|(call, w)| {
                    if *call <= dispatcher.end
                        || !ident(w[0])
                        || w[1] != "="
                        || !ast.same(w[2], dispatcher.name)
                        || w[3] != "("
                        || (!ident(w[4]) && w[4].parse::<i64>().is_err())
                        || (ident(w[4]) && !known.get(w[4]).is_some())
                        || (label != dispatcher.params[0]
                            && w[4] != label
                            && (known.get(label).is_none() || known.get(w[4]) != known.get(label)))
                        || w[5..9] != [",", "[", "]", ")"]
                        || !matches!(t.get(call + 9), Some(&";" | &","))
                    {
                        return false;
                    }
                    true
                })
                .map(|(_, w)| w[0])
                .collect();
            (i + 1, owners)
        } else {
            continue;
        };
        let Some((a, array_end)) = array(&t, array_start, &known, &minus, 0) else {
            continue;
        };
        if !matches!(t.get(array_end), Some(&";" | &"," | &"}")) {
            continue;
        }
        if a.len() >= 44 && (allow_legacy || a.windows(44).any(|w| w.iter().all(Option::is_some))) {
            // Many owner bindings may refer to one literal table. Share data.
            let a = Rc::new(a);
            for owner in owners {
                if !allow_legacy && !ast.stable(owner) {
                    continue;
                }
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
    let decoder_body_len=tokens("if(typeof d==='number'){d=t;}var c=s(i,k[s(k.length,o)]);var r='';while(l-->0){r+=String.fromCharCode(c);c+=d[x];x++;}return r;").len();
    let candidate_functions: Vec<_> = functions
        .iter()
        .filter(|function| {
            function.end - function.begin == decoder_body_len && ast.stable(function.name)
        })
        .collect();
    for (owner, a) in bound_tables {
        if allow_legacy {
            break;
        } // Exact-build compatibility uses only the verified legacy path.
        // Bind each table to its delta accumulator and seed stack. Arbitrary
        // arrays and unrelated push() calls cannot serve as decoders.
        for function in &candidate_functions {
            candidate_work = candidate_work.checked_sub(decoder_body_len)?;
            let body = &t[function.begin..function.end];
            for fallback in body
                .windows(4)
                .filter(|w| w[1] == "=" && ast.same(w[2], owner) && w[3] == ";")
            {
                for step in body.windows(7).filter(|w| {
                    w[1] == "+=" && w[2] == fallback[0] && w[3] == "[" && w[5..7] == ["]", ";"]
                }) {
                    for seed in body.windows(19) {
                        if seed[0] != step[0]
                            || seed[1] != "="
                            || !ast.subtract(seed[2])
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
                            let begin = ast.offset(t[function.begin])?;
                            let end = ast.offset(t[function.end])?;
                            if !ast.dependency(owner, begin, end, false)
                                || !ast.dependency(seed[6], begin, end, true)
                            {
                                continue;
                            }
                            if !body.windows(4).any(|w| {
                                w[1..] == [".", "fromCharCode", "("] && ast.builtin(w[0], "String")
                            }) {
                                continue;
                            }
                            if !supported_decoder_body(
                                body,
                                owner,
                                fallback[0],
                                seed,
                                step,
                                length[2],
                            ) {
                                continue;
                            }
                            let data_arg = function.params.iter().position(|p| *p == fallback[0]);
                            if data_arg.is_none() && !ast.numeric_data(fallback[0], begin, end) {
                                continue;
                            }
                            if function.params.iter().collect::<BTreeSet<_>>().len()
                                != function.params.len()
                            {
                                continue;
                            }
                            tables.entry(ast.symbol(function.name)?).or_default().push((
                                a.clone(),
                                seed[6],
                                function.name,
                                owner,
                                function.end,
                                initial_arg,
                                index_arg,
                                length_arg,
                                function.params.len(),
                                data_arg,
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
        if allow_legacy {
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
                        *scope.last_mut()? = Some((
                            t[i - 1],
                            e.value(&known)
                                .and_then(|v| if let Value::Num(n) = v { Some(n) } else { None }),
                        ));
                    }
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
        if allow_legacy {
            let Some((stack, Some(seed))) = scope.iter().rev().find_map(|n| *n) else {
                continue;
            };
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
            continue;
        }
        let Some(matches) = i
            .checked_sub(1)
            .and_then(|index| ast.symbol(t[index]))
            .and_then(|id| tables.get(&id))
        else {
            continue;
        };
        for (
            table,
            decoder_stack,
            callee,
            owner,
            definition_end,
            initial_arg,
            index_arg,
            length_arg,
            arity,
            data_arg,
        ) in matches
        {
            if i == 0
                || !ast.same(t[i - 1], callee)
                || (i >= 2 && t[i - 2] == ".")
                || i <= *definition_end
                || args.len() != *arity
            {
                continue;
            }
            if !ast.available_before(owner, t[i - 1]) {
                continue;
            }
            let Some(seed) = ast.push_seed(decoder_stack, t[i - 1]) else {
                continue;
            };
            if data_arg.is_some_and(|p| args[p].is_none()) {
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
    fn rejects_prefix_mutated_delta_constants() {
        for mutation in [
            "++seed;",
            "--seed;",
            "seed++;",
            "seed--;",
            "++initial;",
            "initial%=7;",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn returned_table_requires_its_own_dispatcher() {
        let source = include_str!("fixtures/homedepot_fresh_return_delta.js");
        let unrelated = source.replace("table=factory(state,[])", "table=unrelated(state,[])")
            + "function unrelated(){return [];}";
        assert_eq!(extract(&unrelated), None);
        let renamed = source.replace("factory", "renamed_factory");
        assert_eq!(extract(&renamed), extract(source));
    }
    #[test]
    fn delta_decoder_requires_supported_output_and_progression() {
        for (from, to) in [
            ("return out;", "return 'not a version';"),
            ("fromCharCode(code)", "fromCharCode(code+1)"),
            ("index++;", "index--;"),
            ("code+=data[index]", "code+=data[index+1]"),
            ("out='';", "out='prefix';"),
            ("return out;", "out='different';return out;"),
        ] {
            assert_eq!(extract(&FIXTURE.replace(from, to)), None, "{to}");
        }
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

    #[test]
    fn unsupported_mutations_invalidate_constants_at_any_depth() {
        for mutation in [
            "start||=1;",
            "seed&&=867;",
            "seed**=2;",
            "seed>>>=1;",
            "[seed]=[867];",
            "({seed}={seed:867});",
            "for(seed of [867]){}",
            "for(seed in {}){}",
            "function a(){function b(){function c(){seed++;}}}",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn runtime_numbers_are_required_for_decoder_roles() {
        for args in [
            "[size,!1,0,initial,0,start]",
            "[size,0,0,initial,0,{}]",
            "[size,0,0,initial,0,!1]",
            "[size,[],0,initial,0,start]",
        ] {
            assert_eq!(
                extract(&FIXTURE.replace("[size,0,0,initial,0,start]", args)),
                None,
                "{args}"
            );
        }
        let fresh = include_str!("fixtures/homedepot_fresh_return_delta.js");
        for changed in [
            fresh.replace("var data=0;", "var data=!0;"),
            fresh.replace("7,0,38", "7,{},38"),
        ] {
            assert_eq!(extract(&changed), None);
        }
        assert_eq!(
            extract(&FIXTURE.replace("[size,0,0,initial,0,start]", "[size,+!1,0,initial,0,start]"))
                .as_deref(),
            Some(TOKEN)
        );
    }
    #[test]
    fn global_data_source_must_be_stable_and_unshadowed() {
        let fresh = include_str!("fixtures/homedepot_fresh_return_delta.js");
        for mutation in ["data||=[];", "[data]=[[]];", "data&&=[];"] {
            assert_eq!(
                extract(&fresh.replace("var data=0;", &format!("var data=0;{mutation}"))),
                None,
                "{mutation}"
            );
        }
        let wrapped = fresh.replace(
            "function decoder(",
            "function outer(data){function decoder(",
        ) + "}outer([]);";
        assert_eq!(extract(&wrapped), None);
    }
    #[test]
    fn decoder_dependencies_cannot_be_replaced_or_transformed() {
        for mutation in [
            "decoder=unrelated;",
            "sub=function(a,b){return b-a;};",
            "negative=function(n){return n;};",
            "table=[];",
            "table.reverse();",
            "stack=[];",
            "function alter(){stack.pop();}alter();",
            "String.fromCharCode=function(){return 65;};",
            "function String(){}",
            "decoder.apply=function(){return 'x';};",
            "Function.prototype.apply=function(){return 'x';};",
        ] {
            let source =
                FIXTURE.replace("stack.push(seed);", &format!("stack.push(seed);{mutation}"));
            assert_eq!(extract(&source), None, "{mutation}");
        }
        assert_eq!(extract(&format!("decoder=unrelated;{FIXTURE}")), None);
        assert_eq!(
            extract(&FIXTURE.replace("negative(65)];", "negative(65)].reverse();")),
            None
        );
        let wrapped = FIXTURE.replace(
            "stack.push(seed);",
            "function run(decoder){stack.push(seed);",
        ) + "}run(unrelated);";
        assert_eq!(extract(&wrapped), None);
    }
    #[test]
    fn dispatcher_must_directly_return_matching_case() {
        let fresh = include_str!("fixtures/homedepot_fresh_return_delta.js");
        for source in [
            fresh.replace("factory(state,args)", "factory(args,state)"),
            fresh.replace("case state:{return", "case state:break;default:{return"),
            fresh
                .replace("case state:{return", "case state:{var f=function(){return")
                .replace(";}}}", ";};}}}"),
            fresh.replace(";}}}", ".slice(1);}}}"),
            fresh.replace("factory(state,[]);", "factory(state,[]).reverse();"),
            format!("factory=unrelated;{fresh}"),
        ] {
            assert_eq!(extract(&source), None);
        }
    }

    #[test]
    fn block_stack_writes_and_deep_plain_writes_fail_closed() {
        for mutation in [
            "if(1){stack.pop();}",
            "if(1){stack.push(1);}",
            "function a(){function b(){function c(){seed=867;}c();}b();}a();",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("stack.push(seed);{mutation}"))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn unknown_binding_scopes_and_table_helper_shadows_fail_closed() {
        for (begin, end) in [
            ("((decoder)=>{", "})(function(){return 'x';});"),
            (
                "function run({decoder}){",
                "}run({decoder:function(){return 'x';}});",
            ),
            ("function run(...decoder){", "}run([]);"),
            ("try{throw 0}catch(decoder){", "}"),
        ] {
            let changed =
                FIXTURE.replace("stack.push(seed);", &format!("{begin}stack.push(seed);")) + end;
            assert_eq!(extract(&changed), None, "{begin}");
        }
        assert_eq!(
            extract(&FIXTURE.replace(
                "stack.push(seed);",
                "stack.push(seed);var f=()=>{stack.pop();};f();"
            )),
            None
        );
        let changed = FIXTURE
            .replace("var table=[", "function init(negative){table=[")
            .replace(
                "negative(65)];",
                "negative(65)];}init(function(n){return n;});",
            );
        assert_eq!(extract(&changed), None);
    }
    #[test]
    fn visible_bracket_apply_and_prototype_overrides_fail_closed() {
        for mutation in [
            "decoder['apply']=function(){return 'x';};",
            "Function.prototype['apply']=function(){return 'x';};",
            "Object.defineProperty(Function.prototype,'apply',{});",
            "Array.prototype.push=function(){};",
            "Array['prototype'].push=function(){};",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn octal_and_late_global_data_are_unsupported() {
        assert_eq!(extract(&FIXTURE.replace("size=one*44", "size=044")), None);
        let fresh = include_str!("fixtures/homedepot_fresh_return_delta.js");
        assert_eq!(
            extract(&(fresh.replace("var data=0;", "") + "var data=0;")),
            None
        );
    }

    #[test]
    fn method_and_class_scopes_are_unsupported() {
        for (begin, end) in [
            ("({run(decoder){", "}}).run(function(){return 'x';});"),
            (
                "class C{static run(decoder){",
                "}}C.run(function(){return 'x';});",
            ),
            ("({set v(decoder){", "}}).v=function(){return 'x';};"),
        ] {
            assert_eq!(
                extract(
                    &(FIXTURE.replace("stack.push(seed);", &format!("{begin}stack.push(seed);"))
                        + end)
                ),
                None,
                "{begin}"
            );
        }
    }
    #[test]
    fn bare_declarations_cannot_shadow_decoder_constants_or_roles() {
        for name in [
            "size", "seed", "start", "initial", "table", "stack", "sub", "decoder",
        ] {
            assert_eq!(
                extract(
                    &(FIXTURE.replace(
                        "stack.push(seed);",
                        &format!("{{let {name};stack.push(seed);")
                    ) + "}")
                ),
                None,
                "{name}"
            );
        }
        let wrapped = FIXTURE.replace(
            "function decoder(",
            "function outer(){var one;function decoder(",
        ) + "}outer();";
        assert_eq!(extract(&wrapped), None);
        assert_eq!(
            extract(
                &(FIXTURE.replace(
                    "stack.push(seed);",
                    "function run(){function initial(){}stack.push(seed);"
                ) + "}run();")
            ),
            None
        );
    }
    #[test]
    fn visible_indirect_prototype_writes_are_unsupported() {
        for mutation in [
            "decoder.__proto__.apply=function(){return 'x';};",
            "[].__proto__.push=function(){};",
            "Object.setPrototypeOf(decoder,{});",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }

    #[test]
    fn chained_and_conditional_destructuring_updates_are_not_constants() {
        for mutation in [
            "x=[seed]=[867];",
            "0?0:[seed]=[867];",
            "x={seed}={seed:867};",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }
    #[test]
    fn reserved_word_method_names_are_not_control_statements() {
        for name in ["if", "while", "for", "switch"] {
            let source = FIXTURE.replace(
                "stack.push(seed);",
                &format!("({{{name}(decoder){{stack.push(seed);"),
            ) + &format!("}}}}).{name}(function(){{return 'x';}});");
            assert_eq!(extract(&source), None, "{name}");
        }
    }
    #[test]
    fn visible_global_property_writes_invalidate_static_constants() {
        for mutation in [
            "globalThis.seed=867;",
            "this.seed=867;",
            "window['seed']=867;",
            "window[\"seed\"]+=1;",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }

    #[test]
    fn reserved_methods_after_operators_are_not_control_headers() {
        for prefix in [
            "0||", "1&&", "null??", "!", "void ", "typeof ", "+", "false?0:",
        ] {
            let source = FIXTURE.replace(
                "stack.push(seed);",
                &format!("{prefix}{{if(decoder){{stack.push(seed);"),
            ) + "}}.if(function(){return 'x';});";
            assert_eq!(extract(&source), None, "{prefix}");
        }
    }
    #[test]
    fn additional_global_property_write_forms_invalidate_the_seed() {
        for mutation in [
            "for(this.seed of [867]){}",
            "(globalThis.seed)=867;",
            "++(window.seed);",
            "--window.self.seed;",
            "++((window['seed']));",
        ] {
            assert_eq!(
                extract(
                    &FIXTURE.replace("stack.push(seed);", &format!("{mutation}stack.push(seed);"))
                ),
                None,
                "{mutation}"
            );
        }
    }

    #[test]
    fn ast_preserves_decoder_when_unrelated_names_are_shadowed() {
        for prefix in [
            "function other(){function decoder(){return 'x';}}",
            "function other(String){return String;}",
            "{let seed;}",
            "obj.seed=867;",
            "String(42);",
        ] {
            assert_eq!(
                extract(&format!("{prefix}{FIXTURE}")).as_deref(),
                Some(TOKEN),
                "{prefix}"
            );
        }
        assert_eq!(
            extract(&format!("(function(){{{FIXTURE}window.seed=867;}})();")).as_deref(),
            Some(TOKEN)
        );
    }
    #[test]
    fn ast_helpers_must_be_synchronous_scalar_functions() {
        for source in [
            FIXTURE.replace("negative=function(", "negative=async function("),
            FIXTURE.replace("negative=function(", "negative=function*("),
            FIXTURE.replace("sub=function(", "sub=async function("),
            FIXTURE.replace("sub=function(", "sub=function*("),
        ] {
            assert_eq!(extract(&source), None);
        }
    }
    #[test]
    fn ast_rejects_visible_constructor_escapes_and_invalid_source() {
        for prefix in [
            "Object.defineProperty(String,'fromCharCode',{value:function(){return 65;}});",
            "var alias=String;alias.fromCharCode=function(){return 65;};",
        ] {
            assert_eq!(extract(&format!("{prefix}{FIXTURE}")), None);
        }
        assert_eq!(
            super::super::extract_bmak_ver(&format!("{FIXTURE}const broken = ;")),
            None
        );
    }

    #[test]
    fn ast_seed_uses_the_complete_push_argument_expression() {
        for expression in ["seed+1", "(seed+1)", "seed*2", "seed-1"] {
            assert_eq!(
                extract(&FIXTURE.replace("stack.push(seed)", &format!("stack.push({expression})"))),
                None,
                "{expression}"
            );
        }
        assert_eq!(
            extract(&FIXTURE.replace("stack.push(seed)", "stack.push(negative(-seed))")).as_deref(),
            Some(TOKEN)
        );
    }
    #[test]
    fn ast_initializer_order_and_function_hoisting_are_distinct() {
        assert_eq!(
            extract(&(FIXTURE.replace("initial=945", "initial=late") + "var late=945;")),
            None
        );
        let moved = FIXTURE.replace("var negative=function(n){return -n;};", "")
            + "var negative=function(n){return -n;};";
        assert_eq!(extract(&moved), None);
        let hoisted = FIXTURE.replace("var negative=function(n){return -n;};", "")
            + "function negative(n){return -n;}";
        assert_eq!(extract(&hoisted).as_deref(), Some(TOKEN));
    }

    #[test]
    fn ast_global_function_properties_do_not_alias_local_functions() {
        for prefix in [
            "window.decoder=function(){return 'x';};",
            "globalThis.decoder=function(){return 'x';};",
            "this.decoder=function(){return 'x';};",
        ] {
            assert_eq!(extract(&format!("{prefix}{FIXTURE}")), None, "{prefix}");
            assert_eq!(
                extract(&format!("(function(){{{prefix}{FIXTURE}}})();")).as_deref(),
                Some(TOKEN)
            );
        }
    }

    #[test]
    fn ast_seed_push_must_be_a_direct_straight_line_statement() {
        for push in [
            "if(0)stack.push(seed);",
            "0&&stack.push(seed);",
            "0?stack.push(seed):0;",
            "while(0)stack.push(seed);",
        ] {
            assert_eq!(
                extract(&FIXTURE.replace("stack.push(seed);", push)),
                None,
                "{push}"
            );
        }
    }
    #[test]
    fn ast_state_dependencies_must_exist_before_the_call() {
        let stack = FIXTURE.replace("var stack=[];", "") + "var stack=[];";
        assert_eq!(extract(&stack), None);
        let table_start = FIXTURE.find("var table=").unwrap();
        let table_end = table_start + FIXTURE[table_start..].find(';').unwrap() + 1;
        let table = &FIXTURE[table_start..table_end];
        let moved = FIXTURE.replace(table, "") + table;
        assert_eq!(extract(&moved), None);
        assert_eq!(
            extract(&format!("{{function negative(n){{return n;}}}}{FIXTURE}")),
            None
        );
    }
    #[test]
    fn dispatcher_lookup_handles_many_siblings_and_nested_functions() {
        let mut noise = String::new();
        for i in 0..1500 {
            noise.push_str(&format!("function unused{i}(){{return [0];}}"));
        }
        assert_eq!(extract(&noise), None);
        let source = include_str!("fixtures/homedepot_fresh_return_delta.js");
        let wrapped = format!("{noise}function container(){{{source}}}container();");
        assert_eq!(extract(&wrapped), extract(source));
        assert!(extract(source).is_some());
    }
    #[test]
    fn many_factory_owners_share_one_supported_table() {
        let source = include_str!("fixtures/homedepot_fresh_return_delta.js");
        let mut changed = source.to_owned();
        for i in 0..512 {
            changed.push_str(&format!("var alias{i}=factory(state,[]);"));
        }
        assert_eq!(extract(&changed), extract(source));
        assert!(extract(source).is_some());
    }
    #[test]
    fn delta_candidate_budget_rejects_partial_matching() {
        let ast = Analysis::parse(FIXTURE).unwrap();
        assert_eq!(extract_with_budget(FIXTURE, &ast, 0), None);
        assert_eq!(extract(FIXTURE).as_deref(), Some(TOKEN));
    }

    #[test]
    fn indexed_outside_dependency_checks_preserve_read_only_and_stack_rules() {
        let read_only = format!("{FIXTURE}{}", "table[0];".repeat(4096));
        assert_eq!(extract(&read_only).as_deref(), Some(TOKEN));
        let extra_stack_reads = format!("{FIXTURE}{}", "stack[0];".repeat(4096));
        assert_eq!(extract(&extra_stack_reads), None);
    }

    #[test]
    fn repeated_valid_apply_calls_use_the_same_proven_seed_and_version() {
        let source = format!(
            "{FIXTURE}{}",
            "decoder.apply(null,[size,0,0,initial,0,start]);".repeat(1024)
        );
        assert_eq!(extract(&source).as_deref(), Some(TOKEN));
    }
}

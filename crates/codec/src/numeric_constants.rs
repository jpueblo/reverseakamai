//! Pure integer expressions used only to resolve shuffle-key pool indexes.
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

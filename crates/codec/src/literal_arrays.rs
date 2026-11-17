//! Locate complete string-only array literals without evaluating JavaScript.
//! This is a constrained lexer, not a general JavaScript parser. Templates are
//! skipped entirely, and only arrays containing quoted strings are returned.

fn quoted_end(bytes: &[u8], start: usize) -> Option<usize> {
    let quote = bytes[start];
    let mut i = start + 1;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            c if c == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn whitespace(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

fn array_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = whitespace(bytes, start + 1);
    if !matches!(bytes.get(i), Some(b'\'' | b'"')) {
        return None;
    }
    loop {
        i = whitespace(bytes, quoted_end(bytes, i)?);
        match bytes.get(i) {
            Some(b']') => return Some(i + 1),
            Some(b',') => {
                i = whitespace(bytes, i + 1);
                if bytes.get(i) == Some(&b']') {
                    return Some(i + 1);
                }
                if !matches!(bytes.get(i), Some(b'\'' | b'"')) {
                    return None;
                }
            }
            _ => return None,
        }
    }
}

fn regex_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut i = start + 1;
    let mut class = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\r' | b'\n' => return None,
            b'[' => {
                class = true;
                i += 1;
            }
            b']' => {
                class = false;
                i += 1;
            }
            b'/' if !class => {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                    i += 1;
                }
                return Some(i);
            }
            _ => i += 1,
        }
    }
    None
}

pub(super) fn tokens(source: &str) -> Vec<&str> {
    let bytes = source.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    let mut regex_allowed = true;
    let mut control_paren = false;
    let mut parens = Vec::new();
    while i < bytes.len() {
        let start = i;
        let mut emit = true;
        match bytes[i] {
            b'/' if bytes.get(i + 1) == Some(&b'/') => {
                emit = false;
                while i < bytes.len() && !matches!(bytes[i], b'\r' | b'\n') {
                    i += 1;
                }
            }
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                emit = false;
                i += 2;
                while i + 1 < bytes.len() && &bytes[i..i + 2] != b"*/" {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
            }
            b'\'' | b'"' | b'`' => {
                i = quoted_end(bytes, i).unwrap_or(bytes.len());
                regex_allowed = false;
            }
            b'/' if regex_allowed => {
                i = regex_end(bytes, i).unwrap_or(i + 1);
                regex_allowed = false;
            }
            c if c.is_ascii_alphabetic() || matches!(c, b'_' | b'$') => {
                let start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$'))
                {
                    i += 1;
                }
                regex_allowed = matches!(
                    &source[start..i],
                    "return"
                        | "throw"
                        | "case"
                        | "typeof"
                        | "void"
                        | "delete"
                        | "yield"
                        | "await"
                        | "new"
                        | "in"
                        | "instanceof"
                );
                control_paren = matches!(
                    &source[start..i],
                    "if" | "while" | "for" | "with" | "switch" | "catch"
                );
            }
            c if c.is_ascii_whitespace() => {
                i += 1;
                emit = false;
            }
            c if c.is_ascii_digit() => {
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                regex_allowed = false;
            }
            b'(' => {
                parens.push(control_paren);
                control_paren = false;
                regex_allowed = true;
                i += 1;
            }
            b')' => {
                regex_allowed = parens.pop().unwrap_or(false);
                i += 1;
            }
            c if c >= 128 => {
                i += source[i..].chars().next().unwrap().len_utf8();
                regex_allowed = false;
            }
            c => {
                regex_allowed = !c.is_ascii_digit() && !matches!(c, b']' | b'.');
                control_paren = false;
                i += 1;
                if i < bytes.len()
                    && bytes[i].is_ascii()
                    && matches!(
                        &source[start..i + 1],
                        "==" | "!="
                            | "<="
                            | ">="
                            | "++"
                            | "--"
                            | "+="
                            | "-="
                            | "*="
                            | "/="
                            | "&&"
                            | "||"
                            | "<<"
                            | ">>"
                            | "=>"
                    )
                {
                    i += 1;
                    if i < bytes.len()
                        && bytes[i] == b'='
                        && matches!(&source[start..i], "==" | "!=")
                    {
                        i += 1;
                    }
                }
            }
        }
        if emit {
            result.push(&source[start..i]);
        }
    }
    result
}

pub(super) fn string_arrays(source: &str) -> Vec<&str> {
    tokens(source)
        .into_iter()
        .filter_map(|token| {
            if token != "[" {
                return None;
            }
            let start = token.as_ptr() as usize - source.as_ptr() as usize;
            let end = array_end(source.as_bytes(), start)?;
            Some(&source[start..end])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ignores_quoted_commented_template_and_regex_decoys() {
        let source = r#"'fake ["x"]'; /* ["y"] */ // ["z"]
            `template ["t"]`; const r = /["r"]/; return ["real", 'array'];"#;
        assert_eq!(string_arrays(source), vec![r#"["real", 'array']"#]);
    }
    #[test]
    fn only_returns_complete_string_arrays() {
        assert!(string_arrays("const a = ['a', expr]; const b = ['unterminated").is_empty());
        assert_eq!(
            string_arrays("outer([ ['one', 'two',], 0])"),
            vec!["['one', 'two',]"]
        );
    }
}

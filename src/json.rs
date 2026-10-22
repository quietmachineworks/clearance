//! JSON without a crate (ADR-0010): a value that keeps the order of object keys, so that the user's
//! `settings.json` is rewritten in place, a parser, and the two layouts of JavaScript's
//! `JSON.stringify` (compact, and indented by two spaces).

use std::fmt::{self, Write};

#[derive(Clone, Debug, PartialEq, Default)]
pub enum Json {
    #[default]
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

static NULL: Json = Json::Null;

impl Json {
    pub fn parse(text: &str) -> Result<Json, String> {
        let mut p = Parser { s: text.as_bytes(), i: 0, depth: 0 };
        p.ws();
        let value = p.value()?;
        p.ws();
        if p.i != p.s.len() {
            return Err(p.err("trailing characters"));
        }
        Ok(value)
    }

    /// The value under `key`, or null: lookups chain without checks, like `?.` in JavaScript.
    pub fn get(&self, key: &str) -> &Json {
        match self {
            Json::Obj(m) => m.iter().find(|(k, _)| k == key).map_or(&NULL, |(_, v)| v),
            _ => &NULL,
        }
    }

    pub fn str(&self) -> Option<&str> {
        if let Json::Str(s) = self { Some(s) } else { None }
    }

    pub fn num(&self) -> Option<f64> {
        if let Json::Num(n) = self { Some(*n) } else { None }
    }

    pub fn bool(&self) -> Option<bool> {
        if let Json::Bool(b) = self { Some(*b) } else { None }
    }

    /// The items of an array, none for anything else.
    pub fn items(&self) -> &[Json] {
        if let Json::Arr(a) = self { a } else { &[] }
    }

    /// The entries of an object, none for anything else.
    pub fn entries(&self) -> &[(String, Json)] {
        if let Json::Obj(m) = self { m } else { &[] }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Json::Null)
    }

    /// Sets `key`, in place when it exists, last otherwise. Anything but an object becomes one.
    pub fn set(&mut self, key: &str, value: Json) {
        if !matches!(self, Json::Obj(_)) {
            *self = Json::Obj(Vec::new());
        }
        if let Json::Obj(m) = self {
            match m.iter_mut().find(|(k, _)| k == key) {
                Some((_, v)) => *v = value,
                None => m.push((key.to_string(), value)),
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<Json> {
        let Json::Obj(m) = self else { return None };
        let i = m.iter().position(|(k, _)| k == key)?;
        Some(m.remove(i).1)
    }

    /// `JSON.stringify(value, null, 2)`.
    pub fn pretty(&self) -> String {
        let mut out = String::new();
        write_value(&mut out, self, Some(0));
        out
    }
}

/// Compact, as `JSON.stringify(value)`.
impl fmt::Display for Json {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut out = String::new();
        write_value(&mut out, self, None);
        f.write_str(&out)
    }
}

impl From<&str> for Json {
    fn from(s: &str) -> Json {
        Json::Str(s.to_string())
    }
}
impl From<String> for Json {
    fn from(s: String) -> Json {
        Json::Str(s)
    }
}
impl From<&String> for Json {
    fn from(s: &String) -> Json {
        Json::Str(s.clone())
    }
}
impl From<bool> for Json {
    fn from(b: bool) -> Json {
        Json::Bool(b)
    }
}
impl From<f64> for Json {
    fn from(n: f64) -> Json {
        Json::Num(n)
    }
}
impl From<u64> for Json {
    fn from(n: u64) -> Json {
        Json::Num(n as f64)
    }
}
impl From<usize> for Json {
    fn from(n: usize) -> Json {
        Json::Num(n as f64)
    }
}
impl From<Vec<Json>> for Json {
    fn from(a: Vec<Json>) -> Json {
        Json::Arr(a)
    }
}
impl<T: Into<Json>> From<Option<T>> for Json {
    fn from(o: Option<T>) -> Json {
        o.map_or(Json::Null, Into::into)
    }
}

/// An object from pairs. Null values are left out, the way `JSON.stringify` leaves out `undefined`.
pub fn obj<const N: usize>(pairs: [(&str, Json); N]) -> Json {
    Json::Obj(pairs.into_iter().filter(|(_, v)| !v.is_null()).map(|(k, v)| (k.to_string(), v)).collect())
}

fn write_value(out: &mut String, v: &Json, indent: Option<usize>) {
    match v {
        Json::Null => out.push_str("null"),
        Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Json::Num(n) => write_num(out, *n),
        Json::Str(s) => write_str(out, s),
        Json::Arr(a) => write_seq(out, indent, ('[', ']'), a.len(), |out, i, inner| write_value(out, &a[i], inner)),
        Json::Obj(m) => write_seq(out, indent, ('{', '}'), m.len(), |out, i, inner| {
            write_str(out, &m[i].0);
            out.push_str(if inner.is_some() { ": " } else { ":" });
            write_value(out, &m[i].1, inner);
        }),
    }
}

fn write_seq(out: &mut String, indent: Option<usize>, (open, close): (char, char), len: usize, mut item: impl FnMut(&mut String, usize, Option<usize>)) {
    out.push(open);
    if len > 0 {
        let inner = indent.map(|n| n + 2);
        for i in 0..len {
            if i > 0 {
                out.push(',');
            }
            if let Some(n) = inner {
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', n));
            }
            item(out, i, inner);
        }
        if let Some(n) = indent {
            out.push('\n');
            out.extend(std::iter::repeat_n(' ', n));
        }
    }
    out.push(close);
}

// Integers without a decimal point, as JavaScript prints them.
fn write_num(out: &mut String, n: f64) {
    if !n.is_finite() {
        out.push_str("null");
    } else if n == 0.0 {
        out.push('0');
    } else if n.fract() == 0.0 && n.abs() < 1e21 {
        let _ = write!(out, "{n:.0}");
    } else {
        let _ = write!(out, "{n}");
    }
}

fn write_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: usize,
}

impl Parser<'_> {
    fn err(&self, what: &str) -> String {
        format!("{what} at byte {}", self.i)
    }

    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn ws(&mut self) {
        while let Some(b' ' | b'\t' | b'\n' | b'\r') = self.peek() {
            self.i += 1;
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek() {
            Some(b'{') => self.nested(Self::object),
            Some(b'[') => self.nested(Self::array),
            Some(b'"') => self.string().map(Json::Str),
            Some(b't') => self.word("true", Json::Bool(true)),
            Some(b'f') => self.word("false", Json::Bool(false)),
            Some(b'n') => self.word("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            _ => Err(self.err("unexpected character")),
        }
    }

    // A depth limit, so that hostile input cannot overflow the stack.
    fn nested(&mut self, f: fn(&mut Self) -> Result<Json, String>) -> Result<Json, String> {
        self.depth += 1;
        if self.depth > 256 {
            return Err(self.err("nested too deep"));
        }
        let v = f(self);
        self.depth -= 1;
        v
    }

    fn word(&mut self, w: &str, v: Json) -> Result<Json, String> {
        if self.s[self.i..].starts_with(w.as_bytes()) {
            self.i += w.len();
            Ok(v)
        } else {
            Err(self.err("unexpected word"))
        }
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.i;
        while let Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') = self.peek() {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse::<f64>().ok()).map(Json::Num).ok_or_else(|| self.err("bad number"))
    }

    fn array(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut items = Vec::new();
        self.ws();
        if self.peek() == Some(b']') {
            self.i += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            self.ws();
            items.push(self.value()?);
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b']') => {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                _ => return Err(self.err("expected , or ]")),
            }
        }
    }

    fn object(&mut self) -> Result<Json, String> {
        self.i += 1;
        let mut m: Vec<(String, Json)> = Vec::new();
        self.ws();
        if self.peek() == Some(b'}') {
            self.i += 1;
            return Ok(Json::Obj(m));
        }
        loop {
            self.ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a key"));
            }
            let key = self.string()?;
            self.ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected :"));
            }
            self.i += 1;
            self.ws();
            let value = self.value()?;
            // ponytail: a linear search per key; objects here have tens of keys, not thousands.
            match m.iter_mut().find(|(k, _)| *k == key) {
                Some(entry) => entry.1 = value, // the last one wins, as in JSON.parse
                None => m.push((key, value)),
            }
            self.ws();
            match self.peek() {
                Some(b',') => self.i += 1,
                Some(b'}') => {
                    self.i += 1;
                    return Ok(Json::Obj(m));
                }
                _ => return Err(self.err("expected , or }")),
            }
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.i += 1;
        let mut out = String::new();
        loop {
            // The run of plain bytes up to the next quote, backslash or control character, at once. It
            // splits the input only on ASCII bytes, so it is valid UTF-8.
            let start = self.i;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.i += 1;
            }
            out.push_str(std::str::from_utf8(&self.s[start..self.i]).map_err(|_| self.err("invalid UTF-8"))?);
            match self.peek() {
                Some(b'"') => {
                    self.i += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.i += 1;
                    let e = self.peek().ok_or_else(|| self.err("end of input"))?;
                    self.i += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let c = if (0xD800..0xDC00).contains(&hi) && self.s[self.i..].starts_with(b"\\u") {
                                self.i += 2;
                                let lo = self.hex4()?;
                                (0xDC00..0xE000).contains(&lo).then(|| char::from_u32(0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00))).flatten()
                            } else {
                                char::from_u32(hi)
                            };
                            out.push(c.unwrap_or('\u{FFFD}'));
                        }
                        _ => return Err(self.err("bad escape")),
                    }
                }
                _ => return Err(self.err("unterminated string")),
            }
        }
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).and_then(|b| std::str::from_utf8(b).ok()).and_then(|t| u32::from_str_radix(t, 16).ok()).ok_or_else(|| self.err("bad \\u escape"))?;
        self.i += 4;
        Ok(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_key_order_and_numbers() {
        let text = r#"{"z":1,"a":[true,false,null],"m":{"k":"v"},"n":-2.5,"big":86400,"e":1e3}"#;
        let v = Json::parse(text).unwrap();
        assert_eq!(v.to_string(), r#"{"z":1,"a":[true,false,null],"m":{"k":"v"},"n":-2.5,"big":86400,"e":1000}"#);
        assert_eq!(v.get("m").get("k").str(), Some("v"));
        assert!(v.get("missing").get("deeper").is_null());
    }

    #[test]
    fn strings_escape_like_json_stringify() {
        let v = Json::parse(r#""a\"b\\c\né😀\u0001/""#).unwrap();
        assert_eq!(v.str(), Some("a\"b\\c\né😀\u{1}/"));
        assert_eq!(v.to_string(), r#""a\"b\\c\né😀\u0001/""#);
    }

    #[test]
    fn pretty_matches_two_space_indent() {
        let v = Json::parse(r#"{"hooks":{"Stop":[{"hooks":[]}]},"empty":{},"n":0}"#).unwrap();
        assert_eq!(v.pretty(), "{\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": []\n      }\n    ]\n  },\n  \"empty\": {},\n  \"n\": 0\n}");
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for bad in ["", "{", "[1,]", r#"{"a" 1}"#, r#""open"#, "tru", "[[[[", "1 2", "\"\u{1}\""] {
            assert!(Json::parse(bad).is_err(), "{bad:?}");
        }
        assert!(Json::parse(&"[".repeat(300)).is_err());
    }

    #[test]
    fn set_remove_and_builder() {
        let mut v = Json::parse(r#"{"a":1,"b":2}"#).unwrap();
        v.set("a", "x".into());
        v.set("c", true.into());
        assert_eq!(v.remove("b"), Some(Json::Num(2.0)));
        assert_eq!(v.to_string(), r#"{"a":"x","c":true}"#);
        assert_eq!(obj([("k", 1u64.into()), ("gone", Json::Null)]).to_string(), r#"{"k":1}"#);
    }
}

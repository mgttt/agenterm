//! Small bounded JSON codec for the fixed `agenterm-con` schemas.

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_DEPTH: usize = 32;
const MAX_NODES: usize = 65_536;
const MAX_OBJECT_FIELDS: usize = 256;
const MAX_STRING_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    pub fn into_object(self, context: &str) -> Result<Vec<(String, Self)>, String> {
        match self {
            Self::Object(fields) => Ok(fields),
            _ => Err(format!("{context} must be a JSON object")),
        }
    }

    pub fn into_array(self, context: &str) -> Result<Vec<Self>, String> {
        match self {
            Self::Array(values) => Ok(values),
            _ => Err(format!("{context} must be a JSON array")),
        }
    }
}

macro_rules! unsigned_value {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for JsonValue {
            fn from(value: $ty) -> Self { Self::Number(value.to_string()) }
        }
    )*};
}

macro_rules! signed_value {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for JsonValue {
            fn from(value: $ty) -> Self { Self::Number(value.to_string()) }
        }
    )*};
}

unsigned_value!(u16, u32, u64, usize);
signed_value!(i16, i32, i64);

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

pub fn nullable<T: Into<JsonValue>>(value: Option<T>) -> JsonValue {
    value.map(Into::into).unwrap_or(JsonValue::Null)
}

pub fn object<const N: usize>(fields: [(&str, JsonValue); N]) -> JsonValue {
    JsonValue::Object(
        fields
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

pub fn parse(bytes: &[u8]) -> Result<JsonValue, String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(format!("JSON exceeds {MAX_INPUT_BYTES} bytes"));
    }
    std::str::from_utf8(bytes).map_err(|_| "JSON is not valid UTF-8".to_owned())?;
    let mut parser = Parser {
        bytes,
        position: 0,
        nodes: 0,
    };
    let value = parser.value(0)?;
    parser.whitespace();
    if parser.position != bytes.len() {
        return Err(format!("trailing JSON data at byte {}", parser.position));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    position: usize,
    nodes: usize,
}

impl Parser<'_> {
    fn value(&mut self, depth: usize) -> Result<JsonValue, String> {
        if depth > MAX_DEPTH {
            return Err(format!("JSON nesting exceeds {MAX_DEPTH}"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("JSON node count exceeds {MAX_NODES}"));
        }
        self.whitespace();
        match self.peek() {
            Some(b'n') => self.keyword(b"null", JsonValue::Null),
            Some(b't') => self.keyword(b"true", JsonValue::Bool(true)),
            Some(b'f') => self.keyword(b"false", JsonValue::Bool(false)),
            Some(b'"') => self.string().map(JsonValue::String),
            Some(b'[') => self.array(depth + 1),
            Some(b'{') => self.object(depth + 1),
            Some(b'-' | b'0'..=b'9') => self.number().map(JsonValue::Number),
            Some(_) => Err(format!("invalid JSON value at byte {}", self.position)),
            None => Err("unexpected end of JSON".to_owned()),
        }
    }

    fn keyword(&mut self, expected: &[u8], value: JsonValue) -> Result<JsonValue, String> {
        if self
            .bytes
            .get(self.position..self.position + expected.len())
            == Some(expected)
        {
            self.position += expected.len();
            Ok(value)
        } else {
            Err(format!("invalid JSON keyword at byte {}", self.position))
        }
    }

    fn array(&mut self, depth: usize) -> Result<JsonValue, String> {
        self.position += 1;
        self.whitespace();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.value(depth)?);
            if values.len() > MAX_NODES {
                return Err("JSON array is too large".to_owned());
            }
            self.whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn object(&mut self, depth: usize) -> Result<JsonValue, String> {
        self.position += 1;
        self.whitespace();
        let mut fields: Vec<(String, JsonValue)> = Vec::new();
        if self.consume(b'}') {
            return Ok(JsonValue::Object(fields));
        }
        loop {
            self.whitespace();
            if self.peek() != Some(b'"') {
                return Err(format!("object key expected at byte {}", self.position));
            }
            let key = self.string()?;
            if fields.iter().any(|(existing, _)| existing == &key) {
                return Err(format!("duplicate JSON object key {key:?}"));
            }
            self.whitespace();
            self.expect(b':')?;
            let value = self.value(depth)?;
            fields.push((key, value));
            if fields.len() > MAX_OBJECT_FIELDS {
                return Err(format!("JSON object exceeds {MAX_OBJECT_FIELDS} fields"));
            }
            self.whitespace();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Object(fields))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect(b'"')?;
        let mut output = String::new();
        loop {
            let byte = self
                .peek()
                .ok_or_else(|| "unterminated JSON string".to_owned())?;
            match byte {
                b'"' => {
                    self.position += 1;
                    return Ok(output);
                }
                b'\\' => {
                    self.position += 1;
                    self.escape(&mut output)?;
                }
                0..=0x1f => {
                    return Err(format!("control byte in JSON string at {}", self.position));
                }
                0x20..=0x7f => {
                    output.push(byte as char);
                    self.position += 1;
                }
                _ => {
                    let tail = std::str::from_utf8(&self.bytes[self.position..])
                        .map_err(|_| "invalid UTF-8 in JSON string".to_owned())?;
                    let ch = tail.chars().next().ok_or("invalid UTF-8 in JSON string")?;
                    output.push(ch);
                    self.position += ch.len_utf8();
                }
            }
            if output.len() > MAX_STRING_BYTES {
                return Err(format!("JSON string exceeds {MAX_STRING_BYTES} bytes"));
            }
        }
    }

    fn escape(&mut self, output: &mut String) -> Result<(), String> {
        let escaped = self
            .next()
            .ok_or_else(|| "unterminated JSON escape".to_owned())?;
        match escaped {
            b'"' => output.push('"'),
            b'\\' => output.push('\\'),
            b'/' => output.push('/'),
            b'b' => output.push('\u{0008}'),
            b'f' => output.push('\u{000c}'),
            b'n' => output.push('\n'),
            b'r' => output.push('\r'),
            b't' => output.push('\t'),
            b'u' => {
                let first = self.hex_quad()?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if self.next() != Some(b'\\') || self.next() != Some(b'u') {
                        return Err("high surrogate is not followed by a low surrogate".to_owned());
                    }
                    let second = self.hex_quad()?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err("invalid low surrogate in JSON escape".to_owned());
                    }
                    0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return Err("isolated low surrogate in JSON escape".to_owned());
                } else {
                    u32::from(first)
                };
                output.push(char::from_u32(scalar).ok_or("invalid Unicode scalar")?);
            }
            _ => return Err(format!("invalid JSON escape \\{}", escaped as char)),
        }
        Ok(())
    }

    fn hex_quad(&mut self) -> Result<u16, String> {
        let mut value = 0u16;
        for _ in 0..4 {
            let digit = self.next().ok_or("truncated Unicode escape")?;
            value = value
                .checked_mul(16)
                .and_then(|value| value.checked_add(hex(digit)?))
                .ok_or("Unicode escape overflow")?;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<String, String> {
        let start = self.position;
        self.consume(b'-');
        match self.peek() {
            Some(b'0') => {
                self.position += 1;
                if matches!(self.peek(), Some(b'0'..=b'9')) {
                    return Err("JSON number has a leading zero".to_owned());
                }
            }
            Some(b'1'..=b'9') => self.digits(),
            _ => return Err(format!("invalid JSON number at byte {start}")),
        }
        if self.consume(b'.') {
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("JSON fraction requires digits".to_owned());
            }
            self.digits();
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.position += 1;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.position += 1;
            }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err("JSON exponent requires digits".to_owned());
            }
            self.digits();
        }
        Ok(std::str::from_utf8(&self.bytes[start..self.position])
            .expect("validated ASCII number")
            .to_owned())
    }

    fn digits(&mut self) {
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.position += 1;
        }
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.position += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), String> {
        self.whitespace();
        if self.consume(byte) {
            Ok(())
        } else {
            Err(format!(
                "expected {:?} at byte {}",
                byte as char, self.position
            ))
        }
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.position += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let byte = self.peek()?;
        self.position += 1;
        Some(byte)
    }
}

fn hex(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some(u16::from(byte - b'0')),
        b'a'..=b'f' => Some(u16::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u16::from(byte - b'A' + 10)),
        _ => None,
    }
}

pub fn to_vec(value: &JsonValue) -> Vec<u8> {
    let mut output = Vec::new();
    write_value(value, &mut output, None, 0);
    output
}

pub fn to_vec_pretty(value: &JsonValue) -> Vec<u8> {
    let mut output = Vec::new();
    write_value(value, &mut output, Some(2), 0);
    output
}

fn write_value(value: &JsonValue, output: &mut Vec<u8>, indent: Option<usize>, depth: usize) {
    match value {
        JsonValue::Null => output.extend_from_slice(b"null"),
        JsonValue::Bool(value) => output.extend_from_slice(if *value { b"true" } else { b"false" }),
        JsonValue::Number(value) => output.extend_from_slice(value.as_bytes()),
        JsonValue::String(value) => write_string(value, output),
        JsonValue::Array(values) => {
            output.push(b'[');
            for (index, value) in values.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                pretty_break(output, indent, depth + 1);
                write_value(value, output, indent, depth + 1);
            }
            if !values.is_empty() {
                pretty_break(output, indent, depth);
            }
            output.push(b']');
        }
        JsonValue::Object(fields) => {
            output.push(b'{');
            for (index, (key, value)) in fields.iter().enumerate() {
                if index != 0 {
                    output.push(b',');
                }
                pretty_break(output, indent, depth + 1);
                write_string(key, output);
                output.push(b':');
                if indent.is_some() {
                    output.push(b' ');
                }
                write_value(value, output, indent, depth + 1);
            }
            if !fields.is_empty() {
                pretty_break(output, indent, depth);
            }
            output.push(b'}');
        }
    }
}

fn pretty_break(output: &mut Vec<u8>, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        output.push(b'\n');
        output.resize(output.len() + width * depth, b' ');
    }
}

fn write_string(value: &str, output: &mut Vec<u8>) {
    output.push(b'"');
    for ch in value.chars() {
        match ch {
            '"' => output.extend_from_slice(b"\\\""),
            '\\' => output.extend_from_slice(b"\\\\"),
            '\u{0008}' => output.extend_from_slice(b"\\b"),
            '\u{000c}' => output.extend_from_slice(b"\\f"),
            '\n' => output.extend_from_slice(b"\\n"),
            '\r' => output.extend_from_slice(b"\\r"),
            '\t' => output.extend_from_slice(b"\\t"),
            '\u{0000}'..='\u{001f}' => {
                output.extend_from_slice(b"\\u00");
                const HEX: &[u8; 16] = b"0123456789abcdef";
                output.push(HEX[(ch as usize >> 4) & 0xf]);
                output.push(HEX[ch as usize & 0xf]);
            }
            _ => {
                let mut buffer = [0u8; 4];
                output.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
            }
        }
    }
    output.push(b'"');
}

pub fn take(fields: &mut Vec<(String, JsonValue)>, key: &str) -> Option<JsonValue> {
    let index = fields.iter().position(|(name, _)| name == key)?;
    Some(fields.swap_remove(index).1)
}

pub fn reject_unknown(fields: Vec<(String, JsonValue)>, context: &str) -> Result<(), String> {
    match fields.first() {
        Some((key, _)) => Err(format!("{context}: unknown field {key:?}")),
        None => Ok(()),
    }
}

pub fn take_string(
    fields: &mut Vec<(String, JsonValue)>,
    key: &str,
) -> Result<Option<String>, String> {
    match take(fields, key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{key} must be a string")),
    }
}

pub fn take_bool(fields: &mut Vec<(String, JsonValue)>, key: &str) -> Result<Option<bool>, String> {
    match take(fields, key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Bool(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{key} must be a boolean")),
    }
}

fn take_number(fields: &mut Vec<(String, JsonValue)>, key: &str) -> Result<Option<String>, String> {
    match take(fields, key) {
        None | Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Number(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{key} must be a number")),
    }
}

macro_rules! number_take {
    ($name:ident, $ty:ty) => {
        pub fn $name(
            fields: &mut Vec<(String, JsonValue)>,
            key: &str,
        ) -> Result<Option<$ty>, String> {
            take_number(fields, key)?
                .map(|value| {
                    value
                        .parse::<$ty>()
                        .map_err(|_| format!("{key} is outside its numeric range"))
                })
                .transpose()
        }
    };
}

number_take!(take_u16, u16);
number_take!(take_u64, u64);
number_take!(take_usize, usize);
number_take!(take_i16, i16);

pub fn take_f64(fields: &mut Vec<(String, JsonValue)>, key: &str) -> Result<Option<f64>, String> {
    let value = take_number(fields, key)?
        .map(|value| {
            value
                .parse::<f64>()
                .map_err(|_| format!("{key} is not a finite number"))
        })
        .transpose()?;
    if value.is_some_and(|value| !value.is_finite()) {
        return Err(format!("{key} is not a finite number"));
    }
    Ok(value)
}

pub fn take_string_array(
    fields: &mut Vec<(String, JsonValue)>,
    key: &str,
) -> Result<Option<Vec<String>>, String> {
    let Some(value) = take(fields, key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    value
        .into_array(key)?
        .into_iter()
        .map(|value| match value {
            JsonValue::String(value) => Ok(value),
            _ => Err(format!("{key} entries must be strings")),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codec_interoperates_with_serde_json_for_unicode_and_escapes() {
        let input = r#"{"text":"中文\n\uD83D\uDE00","array":[null,true,-12.5e2]}"#.as_bytes();
        let value = parse(input).expect("parse valid JSON");
        let encoded = to_vec(&value);
        let oracle: serde_json::Value = serde_json::from_slice(&encoded).expect("oracle decode");
        assert_eq!(oracle["text"], "中文\n😀");
        assert_eq!(oracle["array"][2], -1250.0);
    }

    #[test]
    fn parser_rejects_ambiguous_or_malformed_inputs() {
        for input in [
            br#"{"a":1,"a":2}"#.as_slice(),
            br#""\uD800""#,
            b"01",
            b"1.",
            b"true trailing",
        ] {
            assert!(parse(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn depth_budget_stops_adversarial_nesting() {
        let input = format!(
            "{}0{}",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        assert!(parse(input.as_bytes()).is_err());
    }
}

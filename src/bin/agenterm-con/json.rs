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
    Unsigned(u64),
    Signed(i64),
    #[cfg(test)]
    RawNumber(String),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(&'static str, JsonValue)>),
}

macro_rules! unsigned_value {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for JsonValue {
            fn from(value: $ty) -> Self { Self::Unsigned(value as u64) }
        }
    )*};
}

macro_rules! signed_value {
    ($($ty:ty),* $(,)?) => {$(
        impl From<$ty> for JsonValue {
            fn from(value: $ty) -> Self { Self::Signed(value as i64) }
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

pub fn object(fields: Vec<(&'static str, JsonValue)>) -> JsonValue {
    JsonValue::Object(fields)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConfigValues {
    pub font_size: Option<f64>,
    pub cols: Option<u16>,
    pub rows: Option<u16>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum ValueKind {
    Null,
    Bool,
    Number { start: usize, end: usize },
    String,
    Array,
    Object,
}

#[derive(Clone, Copy)]
struct KeySpan {
    start: usize,
    end: usize,
}

#[derive(Clone, Copy)]
enum ConfigField {
    FontSize,
    Cols,
    Rows,
}

pub fn parse_config(bytes: &[u8]) -> Result<ConfigValues, String> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(format!("JSON exceeds {MAX_INPUT_BYTES} bytes"));
    }
    std::str::from_utf8(bytes).map_err(|_| "JSON is not valid UTF-8".to_owned())?;
    let mut parser = ConfigParser {
        bytes,
        position: 0,
        nodes: 0,
    };
    let mut config = ConfigValues {
        font_size: None,
        cols: None,
        rows: None,
    };
    let kind = parser.value(0, Some(&mut config))?;
    parser.whitespace();
    if parser.position != bytes.len() {
        return Err(format!("trailing JSON data at byte {}", parser.position));
    }
    if kind != ValueKind::Object {
        return Err("configuration must be a JSON object".to_owned());
    }
    Ok(config)
}

struct ConfigParser<'a> {
    bytes: &'a [u8],
    position: usize,
    nodes: usize,
}

impl ConfigParser<'_> {
    fn value(
        &mut self,
        depth: usize,
        config: Option<&mut ConfigValues>,
    ) -> Result<ValueKind, String> {
        if depth > MAX_DEPTH {
            return Err(format!("JSON nesting exceeds {MAX_DEPTH}"));
        }
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(format!("JSON node count exceeds {MAX_NODES}"));
        }
        self.whitespace();
        match self.peek() {
            Some(b'n') => {
                self.keyword(b"null")?;
                Ok(ValueKind::Null)
            }
            Some(b't') => {
                self.keyword(b"true")?;
                Ok(ValueKind::Bool)
            }
            Some(b'f') => {
                self.keyword(b"false")?;
                Ok(ValueKind::Bool)
            }
            Some(b'"') => {
                self.skip_string()?;
                Ok(ValueKind::String)
            }
            Some(b'[') => {
                self.array(depth + 1)?;
                Ok(ValueKind::Array)
            }
            Some(b'{') => self.object(depth + 1, config),
            Some(b'-' | b'0'..=b'9') => {
                let (start, end) = self.number_range()?;
                Ok(ValueKind::Number { start, end })
            }
            Some(_) => Err(format!("invalid JSON value at byte {}", self.position)),
            None => Err("unexpected end of JSON".to_owned()),
        }
    }

    fn object(
        &mut self,
        depth: usize,
        mut config: Option<&mut ConfigValues>,
    ) -> Result<ValueKind, String> {
        self.position += 1;
        self.whitespace();
        let mut keys = Vec::new();
        let mut field_count = 0usize;
        if self.consume(b'}') {
            return Ok(ValueKind::Object);
        }
        loop {
            self.whitespace();
            if self.peek() != Some(b'"') {
                return Err(format!("object key expected at byte {}", self.position));
            }
            let key = self.skip_string()?;
            if keys.iter().any(|existing| self.same_key(*existing, key)) {
                let duplicate = self.decode_string(key)?;
                return Err(format!("duplicate JSON object key {duplicate:?}"));
            }
            let known = config.as_deref().and_then(|_| self.config_field(key));
            self.whitespace();
            self.expect(b':')?;
            let value = self.value(depth, None)?;
            field_count += 1;
            if field_count > MAX_OBJECT_FIELDS {
                return Err(format!("JSON object exceeds {MAX_OBJECT_FIELDS} fields"));
            }
            if let Some(field) = known {
                let config = config.as_deref_mut().expect("root config is present");
                match field {
                    ConfigField::FontSize => {
                        config.font_size = self.font_size_value(value)?;
                    }
                    ConfigField::Cols => {
                        config.cols = self.u16_value(value, "cols")?;
                    }
                    ConfigField::Rows => {
                        config.rows = self.u16_value(value, "rows")?;
                    }
                }
            }
            keys.push(key);
            self.whitespace();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(ValueKind::Object)
    }

    fn array(&mut self, depth: usize) -> Result<(), String> {
        self.position += 1;
        self.whitespace();
        let mut value_count = 0usize;
        if self.consume(b']') {
            return Ok(());
        }
        loop {
            self.value(depth, None)?;
            value_count += 1;
            if value_count > MAX_NODES {
                return Err("JSON array is too large".to_owned());
            }
            self.whitespace();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(())
    }

    fn font_size_value(&self, value: ValueKind) -> Result<Option<f64>, String> {
        match value {
            ValueKind::Null => Ok(None),
            ValueKind::Number { start, end } => {
                let raw = self.number_text(start, end);
                parse_finite_decimal(raw)
                    .ok_or_else(|| "font_size is not a finite number".to_owned())
                    .map(Some)
            }
            _ => Err("font_size must be a number".to_owned()),
        }
    }

    fn u16_value(&self, value: ValueKind, key: &str) -> Result<Option<u16>, String> {
        match value {
            ValueKind::Null => Ok(None),
            ValueKind::Number { start, end } => self
                .number_text(start, end)
                .parse::<u16>()
                .map(Some)
                .map_err(|_| format!("{key} is outside its numeric range")),
            _ => Err(format!("{key} must be a number")),
        }
    }

    fn config_field(&self, key: KeySpan) -> Option<ConfigField> {
        if self.key_equals(key, "font_size") {
            Some(ConfigField::FontSize)
        } else if self.key_equals(key, "cols") {
            Some(ConfigField::Cols)
        } else if self.key_equals(key, "rows") {
            Some(ConfigField::Rows)
        } else {
            None
        }
    }

    fn key_equals(&self, key: KeySpan, expected: &str) -> bool {
        let mut position = key.start;
        for expected in expected.chars() {
            match next_string_char(self.bytes, &mut position, key.end) {
                Ok(Some(actual)) if actual == expected => {}
                _ => return false,
            }
        }
        matches!(
            next_string_char(self.bytes, &mut position, key.end),
            Ok(None)
        )
    }

    fn same_key(&self, left: KeySpan, right: KeySpan) -> bool {
        let mut left_position = left.start;
        let mut right_position = right.start;
        loop {
            let left_char = next_string_char(self.bytes, &mut left_position, left.end);
            let right_char = next_string_char(self.bytes, &mut right_position, right.end);
            match (left_char, right_char) {
                (Ok(Some(left)), Ok(Some(right))) if left == right => {}
                (Ok(None), Ok(None)) => return true,
                _ => return false,
            }
        }
    }

    fn decode_string(&self, key: KeySpan) -> Result<String, String> {
        let mut position = key.start;
        let mut output = String::new();
        while let Some(ch) = next_string_char(self.bytes, &mut position, key.end)? {
            output.push(ch);
        }
        Ok(output)
    }

    fn skip_string(&mut self) -> Result<KeySpan, String> {
        self.expect(b'"')?;
        let start = self.position;
        let mut decoded_bytes = 0usize;
        loop {
            match next_string_char(self.bytes, &mut self.position, self.bytes.len())? {
                Some(ch) => {
                    decoded_bytes += ch.len_utf8();
                    if decoded_bytes > MAX_STRING_BYTES {
                        return Err(format!("JSON string exceeds {MAX_STRING_BYTES} bytes"));
                    }
                }
                None => {
                    return Ok(KeySpan {
                        start,
                        end: self.position,
                    });
                }
            }
        }
    }

    fn keyword(&mut self, expected: &[u8]) -> Result<(), String> {
        if self
            .bytes
            .get(self.position..self.position + expected.len())
            == Some(expected)
        {
            self.position += expected.len();
            Ok(())
        } else {
            Err(format!("invalid JSON keyword at byte {}", self.position))
        }
    }

    fn number_range(&mut self) -> Result<(usize, usize), String> {
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
        Ok((start, self.position))
    }

    fn number_text(&self, start: usize, end: usize) -> &str {
        std::str::from_utf8(&self.bytes[start..end]).expect("validated ASCII number")
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
}

fn next_string_char(
    bytes: &[u8],
    position: &mut usize,
    end: usize,
) -> Result<Option<char>, String> {
    let byte = *bytes
        .get(*position)
        .ok_or_else(|| "unterminated JSON string".to_owned())?;
    if byte == b'"' {
        *position += 1;
        return Ok(None);
    }
    if byte == b'\\' {
        *position += 1;
        let escaped = *bytes
            .get(*position)
            .ok_or_else(|| "unterminated JSON escape".to_owned())?;
        *position += 1;
        return match escaped {
            b'"' => Ok(Some('"')),
            b'\\' => Ok(Some('\\')),
            b'/' => Ok(Some('/')),
            b'b' => Ok(Some('\u{0008}')),
            b'f' => Ok(Some('\u{000c}')),
            b'n' => Ok(Some('\n')),
            b'r' => Ok(Some('\r')),
            b't' => Ok(Some('\t')),
            b'u' => {
                let first = read_hex_quad(bytes, position, end)?;
                let scalar = if (0xd800..=0xdbff).contains(&first) {
                    if bytes.get(*position) != Some(&b'\\') {
                        return Err("high surrogate is not followed by a low surrogate".to_owned());
                    }
                    *position += 1;
                    if bytes.get(*position) != Some(&b'u') {
                        return Err("high surrogate is not followed by a low surrogate".to_owned());
                    }
                    *position += 1;
                    let second = read_hex_quad(bytes, position, end)?;
                    if !(0xdc00..=0xdfff).contains(&second) {
                        return Err("invalid low surrogate in JSON escape".to_owned());
                    }
                    0x1_0000 + ((u32::from(first) - 0xd800) << 10) + (u32::from(second) - 0xdc00)
                } else if (0xdc00..=0xdfff).contains(&first) {
                    return Err("isolated low surrogate in JSON escape".to_owned());
                } else {
                    u32::from(first)
                };
                Ok(Some(
                    char::from_u32(scalar).ok_or("invalid Unicode scalar")?,
                ))
            }
            _ => Err(format!("invalid JSON escape \\{}", escaped as char)),
        };
    }
    if byte <= 0x1f {
        return Err(format!("control byte in JSON string at {}", position));
    }
    if byte <= 0x7f {
        *position += 1;
        return Ok(Some(byte as char));
    }
    let tail = std::str::from_utf8(
        bytes
            .get(*position..end)
            .ok_or_else(|| "invalid UTF-8 in JSON string".to_owned())?,
    )
    .map_err(|_| "invalid UTF-8 in JSON string".to_owned())?;
    let ch = tail
        .chars()
        .next()
        .ok_or_else(|| "invalid UTF-8 in JSON string".to_owned())?;
    *position += ch.len_utf8();
    Ok(Some(ch))
}

fn read_hex_quad(bytes: &[u8], position: &mut usize, end: usize) -> Result<u16, String> {
    let mut value = 0u16;
    for _ in 0..4 {
        let digit = *bytes
            .get(*position)
            .filter(|_| *position < end)
            .ok_or("truncated Unicode escape")?;
        *position += 1;
        value = value
            .checked_mul(16)
            .and_then(|value| value.checked_add(hex(digit)?))
            .ok_or("Unicode escape overflow")?;
    }
    Ok(value)
}

fn hex(byte: u8) -> Option<u16> {
    match byte {
        b'0'..=b'9' => Some(u16::from(byte - b'0')),
        b'a'..=b'f' => Some(u16::from(byte - b'a' + 10)),
        b'A'..=b'F' => Some(u16::from(byte - b'A' + 10)),
        _ => None,
    }
}

#[cfg(test)]
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
        JsonValue::Unsigned(value) => {
            output.extend_from_slice(itoa::Buffer::new().format(*value).as_bytes());
        }
        JsonValue::Signed(value) => {
            output.extend_from_slice(itoa::Buffer::new().format(*value).as_bytes());
        }
        #[cfg(test)]
        JsonValue::RawNumber(value) => output.extend_from_slice(value.as_bytes()),
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

pub fn parse_finite_decimal(raw: &str) -> Option<f64> {
    let bytes = raw.as_bytes();
    let mut index = 0usize;
    let negative = bytes.first() == Some(&b'-');
    if negative {
        index = 1;
    }
    if index == bytes.len() {
        return None;
    }

    let mut significand = 0u64;
    let mut significant_digits = 0u32;
    let mut kept_digits = 0u32;
    let mut fraction_digits = 0u32;
    let mut saw_digit = false;
    let mut fraction = false;
    while let Some(&byte) = bytes.get(index) {
        match byte {
            b'0'..=b'9' => {
                saw_digit = true;
                if fraction {
                    fraction_digits = fraction_digits.saturating_add(1);
                }
                let digit = u64::from(byte - b'0');
                if significant_digits != 0 || digit != 0 {
                    significant_digits = significant_digits.saturating_add(1);
                    if kept_digits < 18 {
                        significand = significand * 10 + digit;
                        kept_digits += 1;
                    }
                }
                index += 1;
            }
            b'.' if !fraction => {
                fraction = true;
                index += 1;
                if !bytes.get(index).is_some_and(u8::is_ascii_digit) {
                    return None;
                }
            }
            _ => break,
        }
    }
    if !saw_digit {
        return None;
    }

    let mut explicit_exponent = 0i32;
    if bytes
        .get(index)
        .is_some_and(|byte| matches!(byte, b'e' | b'E'))
    {
        index += 1;
        let exponent_negative = bytes.get(index) == Some(&b'-');
        if exponent_negative || bytes.get(index) == Some(&b'+') {
            index += 1;
        }
        let exponent_start = index;
        while let Some(&byte) = bytes.get(index) {
            if !byte.is_ascii_digit() {
                break;
            }
            explicit_exponent = explicit_exponent
                .saturating_mul(10)
                .saturating_add(i32::from(byte - b'0'))
                .min(10_000);
            index += 1;
        }
        if index == exponent_start {
            return None;
        }
        if exponent_negative {
            explicit_exponent = -explicit_exponent;
        }
    }
    if index != bytes.len() {
        return None;
    }
    if significand == 0 {
        return Some(if negative { -0.0 } else { 0.0 });
    }

    let dropped_digits = significant_digits.saturating_sub(kept_digits);
    let exponent =
        explicit_exponent as i64 - i64::from(fraction_digits) + i64::from(dropped_digits);
    if exponent > 308 {
        return None;
    }
    if exponent < -400 {
        return Some(if negative { -0.0 } else { 0.0 });
    }
    let mut value = significand as f64;
    if exponent >= 0 {
        for _ in 0..exponent {
            value *= 10.0;
        }
    } else {
        for _ in exponent..0 {
            value /= 10.0;
            if value == 0.0 {
                break;
            }
        }
    }
    if negative {
        value = -value;
    }
    value.is_finite().then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_interoperates_with_serde_json_for_unicode_and_escapes() {
        let value = object(vec![
            ("text", JsonValue::from("中文\n😀")),
            (
                "array",
                JsonValue::Array(vec![
                    JsonValue::Null,
                    JsonValue::Bool(true),
                    JsonValue::RawNumber("-12.5e2".to_owned()),
                ]),
            ),
        ]);
        let encoded = to_vec(&value);
        let oracle: serde_json::Value = serde_json::from_slice(&encoded).expect("oracle decode");
        assert_eq!(oracle["text"], "中文\n😀");
        assert_eq!(oracle["array"][2], -1250.0);
    }

    #[test]
    fn config_parser_extracts_known_fields_and_skips_unknown_values() {
        let config = parse_config(
            r#"{"font\u005fsize":11.25,"cols":132,"rows":null,"future":{"items":[true,"中文",{"x":1}]}}"#
                .as_bytes(),
        )
        .expect("parse config");
        assert_eq!(config.font_size, Some(11.25));
        assert_eq!(config.cols, Some(132));
        assert_eq!(config.rows, None);
    }

    #[test]
    fn config_parser_rejects_ambiguous_malformed_or_wrong_typed_inputs() {
        for input in [
            br#"{"a":1,"a":2}"#.as_slice(),
            br#"{"outer":{"a":1,"\u0061":2}}"#,
            br#"{"font_size":"11"}"#,
            br#"{"cols":1.5}"#,
            br#"{"rows":65536}"#,
            br#"{"x":"\uD800"}"#,
            br#"{"x":01}"#,
            br#"{"x":1.}"#,
            br#"{} trailing"#,
            b"true",
        ] {
            assert!(parse_config(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn finite_decimal_parser_covers_cli_and_json_number_forms() {
        for (raw, expected) in [
            ("11", 11.0),
            ("11.25", 11.25),
            ("1.125e1", 11.25),
            ("-12.5E-2", -0.125),
            ("0.0000000000000000000012", 1.2e-21),
        ] {
            let actual = parse_finite_decimal(raw).expect("finite decimal");
            assert!((actual - expected).abs() <= expected.abs().max(1.0) * 1e-14);
        }
        for raw in ["", "-", "+1", "1.", "1e", "NaN", "inf", "1e999"] {
            assert_eq!(parse_finite_decimal(raw), None, "accepted {raw}");
        }
    }

    #[test]
    fn depth_budget_stops_adversarial_nesting() {
        let input = format!(
            "{}0{}",
            "[".repeat(MAX_DEPTH + 2),
            "]".repeat(MAX_DEPTH + 2)
        );
        assert!(parse_config(input.as_bytes()).is_err());
    }
}

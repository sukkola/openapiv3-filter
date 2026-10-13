//! YAML output for a `serde_json::Value`.
//!
//! The styling rules (when to quote, literal and folded block scalars, compact lists, number
//! spelling) are adapted from `serde-saphyr` 1.3.0 (`src/ser/quoting.rs`, `src/ser/wrapping.rs`,
//! `src/ser/zmij_format.rs`, and `serialize_str` and the layout rules in `src/ser/serializer.rs`)
//! by the serde-saphyr authors (<https://github.com/bourumir-wyngs/serde-saphyr>), MIT OR
//! Apache-2.0; see `NOTICE.md`. They run with serde-saphyr's default `SerializerOptions`: YAML
//! 1.1-safe quoting, 2 space indent, 80 column folding.

use std::fmt::Write;

use serde_json::Value;

use super::YamlError;

const INDENT: usize = 2;
const FOLD_COLS: usize = 80;

/// Serializes `value` as a YAML document.
///
/// `value` need not have come from [`super::parse`] — it can be built programmatically — so this
/// enforces the same nesting cap ([`super::MAX_DEPTH`]) the parser does, rather than recursing
/// into a stack overflow on a pathologically deep tree the parser would never have produced.
pub fn to_string(value: &Value) -> Result<String, YamlError> {
    let mut out = String::new();
    match value {
        Value::Object(map) if !map.is_empty() => write_map(&mut out, map, 0, false, 0)?,
        Value::Array(items) if !items.is_empty() => write_seq(&mut out, items, 0, false, 0)?,
        other => {
            write_inline(&mut out, other, 0);
            out.push('\n');
        }
    }
    Ok(out)
}

fn too_deep(depth: usize) -> Result<(), YamlError> {
    if depth >= super::MAX_DEPTH {
        return Err(YamlError::new("YAML nesting is too deep"));
    }
    Ok(())
}

fn spaces(out: &mut String, n: usize) {
    for _ in 0..n {
        out.push(' ');
    }
}

type Map = serde_json::Map<String, Value>;

/// Writes a non-empty mapping whose keys sit at column `indent`; when `inline_first` the first key
/// continues the current line (after a `- `). `depth` is this map's nesting depth (0 at the root).
fn write_map(
    out: &mut String,
    map: &Map,
    indent: usize,
    inline_first: bool,
    depth: usize,
) -> Result<(), YamlError> {
    too_deep(depth)?;
    for (i, (key, value)) in map.iter().enumerate() {
        if !(i == 0 && inline_first) {
            spaces(out, indent);
        }
        write_key(out, key);
        out.push(':');
        write_value(out, value, indent, true, depth)?;
    }
    Ok(())
}

/// Writes a non-empty sequence whose dashes sit at column `indent`. `depth` is this sequence's
/// nesting depth (0 at the root).
fn write_seq(
    out: &mut String,
    items: &[Value],
    indent: usize,
    inline_first: bool,
    depth: usize,
) -> Result<(), YamlError> {
    too_deep(depth)?;
    for (i, item) in items.iter().enumerate() {
        if !(i == 0 && inline_first) {
            spaces(out, indent);
        }
        out.push('-');
        write_value(out, item, indent, false, depth)?;
    }
    Ok(())
}

/// Writes what follows `key:` or `-`. `owner` is the column of that key or dash; `depth` is the
/// nesting depth of the map/sequence this value is a member of.
fn write_value(
    out: &mut String,
    value: &Value,
    owner: usize,
    in_map: bool,
    depth: usize,
) -> Result<(), YamlError> {
    match value {
        Value::Object(map) if !map.is_empty() => {
            if in_map {
                out.push('\n');
                write_map(out, map, owner + INDENT, false, depth + 1)?;
            } else {
                out.push(' ');
                write_map(out, map, owner + INDENT, true, depth + 1)?;
            }
        }
        Value::Array(items) if !items.is_empty() => {
            if in_map {
                // Compact lists: the dashes line up with the key.
                out.push('\n');
                write_seq(out, items, owner, false, depth + 1)?;
            } else {
                out.push(' ');
                write_seq(out, items, owner + INDENT, true, depth + 1)?;
            }
        }
        _ => {
            out.push(' ');
            write_inline(out, value, owner);
            out.push('\n');
        }
    }
    Ok(())
}

/// Writes a scalar or an empty collection, followed by nothing; block scalars end with their
/// own final newline handling so the caller adds one only for single-line forms.
fn write_inline(out: &mut String, value: &Value, owner: usize) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::Object(_) => out.push_str("{}"),
        Value::Array(_) => out.push_str("[]"),
        Value::String(s) => write_string(out, s, owner),
    }
}

fn write_number(out: &mut String, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        let _ = write!(out, "{i}");
    } else if let Some(u) = n.as_u64() {
        let _ = write!(out, "{u}");
    } else if let Some(f) = n.as_f64() {
        push_float(out, f);
    }
}

/// Floats always carry a decimal point and a signed exponent so they stay floats in YAML.
fn push_float(target: &mut String, f: f64) {
    let mut buf = zmij::Buffer::new();
    let s = buf.format_finite(f);
    if let Some(exp_pos) = s.find(['e', 'E']) {
        target.push_str(&s[..exp_pos]);
        if !s[..exp_pos].contains('.') {
            target.push_str(".0");
        }
        target.push_str(&s[exp_pos..=exp_pos]);
        if !matches!(s.as_bytes().get(exp_pos + 1), Some(b'+' | b'-')) {
            target.push('+');
        }
        target.push_str(&s[exp_pos + 1..]);
    } else if !s.contains('.') {
        target.push_str(s);
        target.push_str(".0");
    } else {
        target.push_str(s);
    }
}

fn write_key(out: &mut String, key: &str) {
    if is_plain_safe(key) && is_plain_value_safe(key, true) {
        out.push_str(key);
    } else {
        write_quoted(out, key);
    }
}

fn write_quoted(out: &mut String, s: &str) {
    out.push('"');
    escape_double_quoted(s, out);
    out.push('"');
}

/// Writes a string value. Multi-line text becomes a literal block, long single-line text a folded
/// block, everything else plain or double-quoted. Block bodies sit two columns right of `owner`.
fn write_string(out: &mut String, s: &str, owner: usize) {
    enum Style {
        Literal,
        Folded,
    }
    let mut style = None;
    if s.contains('\n') {
        if is_block_scalar_content_safe(s) && !s.is_empty() {
            style = Some(Style::Literal);
        }
    } else {
        let probe = s.trim_end_matches(' ');
        if !probe.is_empty() && is_plain_value_safe(probe, false) && s.chars().count() > FOLD_COLS {
            style = Some(Style::Folded);
        }
    }

    let Some(style) = style else {
        if s.len() == 1 && matches!(s, "." | "#" | "-") {
            out.push('\'');
            out.push_str(s);
            out.push('\'');
        } else if is_plain_value_safe(s, false) {
            out.push_str(s);
        } else {
            write_quoted(out, s);
        }
        return;
    };
    if !is_block_scalar_content_safe(s) {
        write_quoted(out, s);
        return;
    }

    let body = owner + INDENT;
    // The first non-empty line with leading spaces needs an explicit indentation indicator.
    let needs_indicator = first_line_leading_spaces(s.trim_end_matches('\n')) > 0;
    let content = s.trim_end_matches('\n');
    let trailing_nl = s.len() - content.len();
    match style {
        Style::Literal => {
            out.push('|');
            if needs_indicator {
                out.push(char::from_digit(INDENT as u32, 10).expect("single digit"));
            }
            match trailing_nl {
                0 => out.push('-'),
                1 if !content.is_empty() => {}
                _ => out.push('+'),
            }
            out.push('\n');
            if content.is_empty() {
                for _ in 0..trailing_nl {
                    spaces(out, body);
                    out.push('\n');
                }
            } else {
                for line in content.split('\n') {
                    spaces(out, body);
                    out.push_str(line);
                    out.push('\n');
                }
                for _ in 1..trailing_nl {
                    spaces(out, body);
                    out.push('\n');
                }
            }
        }
        Style::Folded => {
            out.push('>');
            if needs_indicator {
                out.push(char::from_digit(INDENT as u32, 10).expect("single digit"));
            }
            match trailing_nl {
                0 => out.push('-'),
                1 => {}
                _ => out.push('+'),
            }
            out.push('\n');
            write_folded_block(out, s, body);
        }
    }
    // Block scalars end with a newline already; the caller adds one after every inline value.
    out.pop();
}

fn first_line_leading_spaces(s: &str) -> usize {
    for line in s.split('\n') {
        if !line.is_empty() {
            return line.len() - line.trim_start_matches(' ').len();
        }
    }
    0
}

/// Writes a folded block body, wrapping at ASCII-space runs once a line passes `FOLD_COLS`.
fn write_folded_block(out: &mut String, s: &str, indent: usize) {
    for line in s.split('\n') {
        if line.is_empty() {
            spaces(out, indent);
            out.push('\n');
            continue;
        }
        // Leading spaces would interact with YAML's "more-indented" rule: never wrap those.
        if line.starts_with(' ') {
            spaces(out, indent);
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let mut start = 0usize;
        let mut col = 0usize;
        // (run_start_byte, run_end_byte, run_len_in_chars) of the last usable space run.
        let mut last_space_run: Option<(usize, usize, usize)> = None;
        let mut in_space_run = false;
        let mut run_start = 0usize;
        let mut run_len = 0usize;
        let mut prev_i = 0usize;
        let mut prev_ch_len = 0usize;
        for (i, ch) in line.char_indices() {
            if in_space_run && ch != ' ' {
                let run_end = prev_i + prev_ch_len;
                // A continuation line starting with a tab could read as more-indented.
                if ch != '\t' {
                    last_space_run = Some((run_start, run_end, run_len));
                }
                in_space_run = false;
                run_len = 0;
            }
            if ch == ' ' {
                if in_space_run {
                    run_len += 1;
                } else {
                    in_space_run = true;
                    run_start = i;
                    run_len = 1;
                }
            }
            col += 1;
            if col > FOLD_COLS {
                let Some((ws_start, ws_end, ws_len)) = last_space_run else {
                    break;
                };
                // Breaking inside a run of N spaces: N-1 stay on the line, one is the fold.
                spaces(out, indent);
                out.push_str(&line[start..ws_start]);
                spaces(out, ws_len - 1);
                out.push('\n');
                start = ws_end;
                col = 0;
                last_space_run = None;
            }
            prev_i = i;
            prev_ch_len = ch.len_utf8();
        }
        spaces(out, indent);
        out.push_str(&line[start..]);
        out.push('\n');
    }
}

// --- quoting rules ---------------------------------------------------------------------------

/// YAML 1.1 boolean spellings (`y`, `yes`, `on`, ...), which older readers still resolve.
fn is_yaml11_bool(s: &str) -> bool {
    let t = s.trim();
    ["true", "yes", "y", "on", "false", "no", "n", "off"]
        .iter()
        .any(|w| t.eq_ignore_ascii_case(w))
}

/// Numeric syntax (YAML 1.1 flavour), checked without constructing a value.
fn is_numeric(s: &str) -> bool {
    if !matches!(
        s.as_bytes().first(),
        Some(b'0'..=b'9' | b'+' | b'-' | b'.' | b'_')
    ) {
        return false;
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if unsigned.eq_ignore_ascii_case(".inf") || unsigned.eq_ignore_ascii_case(".nan") {
        return true;
    }
    if is_yaml11_sexagesimal(s) {
        return true;
    }
    if let Some((radix, digits)) = radix_digits(unsigned)
        && is_digit_run(digits, radix, false)
    {
        return true;
    }
    // Underscores are removed before numeric resolution by some readers.
    let normalized;
    let s = if s.contains('_') {
        normalized = s.replace('_', "");
        normalized.as_str()
    } else {
        s
    };
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    if let Some((radix, digits)) = radix_digits(unsigned) {
        // Some readers accept a sign after an unsigned lowercase 0b or 0o prefix.
        let digits = if s.starts_with("0b") || s.starts_with("0o") {
            digits.strip_prefix(['+', '-']).unwrap_or(digits)
        } else {
            digits
        };
        return is_digit_run(digits, radix, true);
    }
    let mantissa = match unsigned.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => {
            let exponent = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
            if !is_digit_run(exponent, 10, true) {
                return false;
            }
            mantissa
        }
        None => unsigned,
    };
    match mantissa.split_once('.') {
        Some((whole, fraction)) => {
            if !whole.is_empty() && !is_digit_run(whole, 10, true) {
                return false;
            }
            fraction.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        }
        None => is_digit_run(mantissa, 10, true),
    }
}

fn radix_digits(s: &str) -> Option<(u32, &str)> {
    match s.as_bytes() {
        [b'0', b'b' | b'B', ..] => Some((2, &s[2..])),
        [b'0', b'o' | b'O', ..] => Some((8, &s[2..])),
        [b'0', b'x' | b'X', ..] => Some((16, &s[2..])),
        _ => None,
    }
}

fn is_digit_run(s: &str, radix: u32, strict_underscores: bool) -> bool {
    let is_digit = |b: u8| b.is_ascii() && char::from(b).is_digit(radix);
    let bytes = s.as_bytes();
    !bytes.is_empty()
        && bytes.iter().enumerate().all(|(i, &b)| {
            is_digit(b)
                || (b == b'_'
                    && (!strict_underscores
                        || (i > 0
                            && is_digit(bytes[i - 1])
                            && bytes.get(i + 1).is_some_and(|&next| is_digit(next)))))
        })
}

/// YAML 1.1 base-60 integers and floats.
fn is_yaml11_sexagesimal(s: &str) -> bool {
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (integer, fraction) = match unsigned.split_once('.') {
        Some((integer, fraction)) => (integer, Some(fraction)),
        None => (unsigned, None),
    };
    let Some((first, rest)) = integer.split_once(':') else {
        return false;
    };
    let Some(first_digit) = first.as_bytes().first() else {
        return false;
    };
    if !first_digit.is_ascii_digit()
        || (fraction.is_none() && *first_digit == b'0')
        || !first.bytes().all(|b| b.is_ascii_digit() || b == b'_')
    {
        return false;
    }
    if !rest.split(':').all(|part| match part.as_bytes() {
        [digit] => digit.is_ascii_digit(),
        [tens, units] => matches!(tens, b'0'..=b'5') && units.is_ascii_digit(),
        _ => false,
    }) {
        return false;
    }
    fraction.is_none_or(|part| part.bytes().all(|b| b.is_ascii_digit() || b == b'_'))
}

/// YAML 1.1 timestamp syntax, without calendar validation.
fn is_yaml11_timestamp(mut s: &str) -> bool {
    fn digits(s: &mut &str, min: usize, max: usize) -> bool {
        let len = s.bytes().take_while(u8::is_ascii_digit).count();
        if !(min..=max).contains(&len) {
            return false;
        }
        *s = &s[len..];
        true
    }
    fn prefix(s: &mut &str, ch: char) -> bool {
        if let Some(rest) = s.strip_prefix(ch) {
            *s = rest;
            true
        } else {
            false
        }
    }
    if !digits(&mut s, 4, 4)
        || !prefix(&mut s, '-')
        || !digits(&mut s, 1, 2)
        || !prefix(&mut s, '-')
        || !digits(&mut s, 1, 2)
    {
        return false;
    }
    if s.is_empty() {
        return true;
    }
    if let Some(rest) = s.strip_prefix(['T', 't']) {
        s = rest;
    } else {
        let rest = s.trim_start_matches([' ', '\t']);
        if rest.len() == s.len() {
            return false;
        }
        s = rest;
    }
    if !digits(&mut s, 1, 2)
        || !prefix(&mut s, ':')
        || !digits(&mut s, 1, 2)
        || !prefix(&mut s, ':')
        || !digits(&mut s, 1, 2)
    {
        return false;
    }
    if prefix(&mut s, '.') || prefix(&mut s, ',') {
        digits(&mut s, 0, usize::MAX);
    }
    s = s.trim_start_matches([' ', '\t']);
    if s.is_empty() || s == "Z" {
        return true;
    }
    if !(prefix(&mut s, '+') || prefix(&mut s, '-')) || !digits(&mut s, 1, 2) {
        return false;
    }
    if prefix(&mut s, ':') && !digits(&mut s, 2, 2) {
        return false;
    }
    s.is_empty()
}

/// Whether implicit scalar resolution (by any common reader) could change a string's type.
fn is_ambiguous(s: &str) -> bool {
    s.is_empty()
        || s == "~"
        || s.eq_ignore_ascii_case("null")
        || s.eq_ignore_ascii_case("true")
        || s.eq_ignore_ascii_case("false")
        || is_numeric(s)
        || s.eq_ignore_ascii_case("nan")
        || s.eq_ignore_ascii_case("inf")
        || s.eq_ignore_ascii_case("+inf")
        || s.eq_ignore_ascii_case("-inf")
        || is_yaml11_bool(s)
        || is_yaml11_timestamp(s)
        || matches!(s, "<<" | "=")
}

fn starts_with_document_marker(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("---").or_else(|| s.strip_prefix("...")) else {
        return false;
    };
    rest.is_empty() || rest.as_bytes().first().is_some_and(u8::is_ascii_whitespace)
}

/// Whether `s` can be a plain mapping key.
fn is_plain_safe(s: &str) -> bool {
    if is_ambiguous(s) || s == "<<" || starts_with_document_marker(s) {
        return false;
    }
    let bytes = s.as_bytes();
    // Surrounding whitespace is not preserved across a round trip for plain keys.
    if bytes.first().is_some_and(u8::is_ascii_whitespace)
        || bytes.last().is_some_and(u8::is_ascii_whitespace)
    {
        return false;
    }
    match bytes[0] {
        b'-' | b'?' => {
            if bytes.len() == 1 || bytes[1].is_ascii_whitespace() {
                return false;
            }
        }
        b',' | b':' | b'[' | b']' | b'{' | b'}' | b'#' | b'&' | b'*' | b'!' | b'|' | b'>'
        | b'\'' | b'"' | b'%' | b'@' | b'`' => return false,
        _ => {}
    }
    !contains_any_or_is_control(s, &[':']) && !has_comment_start(s)
}

/// Whether `s` can be a plain value (in a flow collection when `in_flow`).
fn is_plain_value_safe(s: &str, in_flow: bool) -> bool {
    if is_ambiguous(s) || starts_with_document_marker(s) {
        return false;
    }
    let bytes = s.as_bytes();
    if bytes.first().is_some_and(u8::is_ascii_whitespace)
        || bytes.last().is_some_and(u8::is_ascii_whitespace)
    {
        return false;
    }
    match bytes {
        [b'-' | b'?', b1, ..] if b1.is_ascii_whitespace() => return false,
        [b'-' | b'?']
        | [
            b',' | b':' | b'[' | b']' | b'{' | b'}' | b'#' | b'&' | b'*' | b'!' | b'|' | b'>'
            | b'\'' | b'"' | b'%' | b'@' | b'`',
            ..,
        ] => return false,
        _ => {}
    }
    if s.contains(": ") || s.trim().ends_with(':') {
        return false;
    }
    if in_flow {
        !contains_any_or_is_control(s, &[',', '[', ']', '{', '}']) && !has_comment_start(s)
    } else {
        !contains_any_or_is_control(s, &[]) && !has_comment_start(s)
    }
}

/// Whether `s` can be written literally in a `|` or `>` block.
fn is_block_scalar_content_safe(s: &str) -> bool {
    s.chars().all(|ch| match ch {
        '\n' | '\t' => true,
        '\r' | '\u{0085}' | '\u{2028}' | '\u{2029}' | '\u{FEFF}' => false,
        c => matches!(
            c as u32,
            0x20..=0x7E | 0xA0..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x0010_FFFF
        ),
    })
}

fn is_controll_which_needs_escaping(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{FEFF}' | '\u{2028}' | '\u{2029}' | '\u{FFFE}' | '\u{FFFF}'
        )
}

fn escape_double_quoted(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\0' => out.push_str("\\0"),
            '\u{7}' => out.push_str("\\a"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{b}' => out.push_str("\\v"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            '\u{1b}' => out.push_str("\\e"),
            '\u{FEFF}' => out.push_str("\\uFEFF"),
            '\u{0085}' => out.push_str("\\N"),
            '\u{2028}' => out.push_str("\\L"),
            '\u{2029}' => out.push_str("\\P"),
            c if (c as u32) <= 0xFF && (c.is_control() || (0x7F..=0x9F).contains(&(c as u32))) => {
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            c if (c as u32) <= 0xFFFF && is_controll_which_needs_escaping(c) => {
                let _ = write!(out, "\\u{:04X}", c as u32);
            }
            c => out.push(c),
        }
    }
}

fn contains_any_or_is_control(string: &str, values: &[char]) -> bool {
    string
        .chars()
        .any(|x| is_controll_which_needs_escaping(x) || values.contains(&x))
}

fn has_comment_start(string: &str) -> bool {
    string
        .chars()
        .zip(string.chars().skip(1))
        .any(|(prev, curr)| prev.is_whitespace() && curr == '#')
}

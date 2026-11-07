//! Lua 5.4 source compatibility for configuration files executed by Luau.
//!
//! The scanner only tokenizes; it never evaluates code. `<const>` attributes are
//! blanked in place so line and column numbers in later diagnostics still match the
//! user's file. Other 5.4-only syntax is reported as a migration hint instead of
//! being rewritten, because an automatic rewrite would change semantics.

use std::borrow::Cow;

const MAX_HINTS: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Symbol,
    Number { hex_float: bool },
    String,
}

#[derive(Clone, Copy, Debug)]
struct Token<'a> {
    kind: Kind,
    text: &'a str,
    start: usize,
    end: usize,
    line: usize,
}

/// Returns the source Luau should compile.
pub(super) fn prepare(source: &str) -> Cow<'_, str> {
    let tokens = tokenize(source);
    let mut blank = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if !is_name(token, "local") || tokens.get(index + 1).is_some_and(|t| is_name(t, "function"))
        {
            continue;
        }
        let mut cursor = index + 1;
        while tokens.get(cursor).is_some_and(|t| t.kind == Kind::Name) {
            cursor += 1;
            if let Some((open, close)) = attribute(&tokens, cursor) {
                if is_name(&tokens[cursor + 1], "const") {
                    blank.push((tokens[open].start, tokens[close].end));
                }
                cursor = close + 1;
            }
            if !tokens.get(cursor).is_some_and(|t| is_symbol(t, ",")) {
                break;
            }
            cursor += 1;
        }
    }
    if blank.is_empty() {
        return Cow::Borrowed(source);
    }
    let mut prepared = source.to_owned();
    for (start, end) in blank.into_iter().rev() {
        prepared.replace_range(start..end, &" ".repeat(end - start));
    }
    Cow::Owned(prepared)
}

/// Lists Lua 5.4 constructs that Luau rejects, each with its replacement.
pub(super) fn migration_hints(source: &str) -> Vec<String> {
    let tokens = tokenize(source);
    let mut hints = Vec::new();
    let mut push = |line: usize, message: String| {
        if hints.len() < MAX_HINTS && !hints.iter().any(|hint: &String| hint.ends_with(&message)) {
            hints.push(format!("line {line}: {message}"));
        }
    };
    for (index, token) in tokens.iter().enumerate() {
        let next = tokens.get(index + 1);
        match (token.kind, token.text) {
            (Kind::Name, "goto") if next.is_some_and(|t| t.kind == Kind::Name) => {
                push(token.line, "goto is not available; use loops with `break`/`continue`".into())
            },
            (Kind::Name, "_ENV") => push(
                token.line,
                "_ENV is not available; pass an explicit table or use setfenv".into(),
            ),
            (Kind::Symbol, "::") if next.is_some_and(|t| t.kind == Kind::Name) => {
                push(token.line, "goto labels (`::name::`) are not available".into())
            },
            (Kind::Symbol, "<") if is_close_attribute(&tokens, index) => push(
                token.line,
                "`<close>` variables are not available; close the resource explicitly".into(),
            ),
            (Kind::Symbol, operator @ ("&" | "|" | "<<" | ">>")) => {
                let function = match operator {
                    "&" => "bit32.band(a, b)",
                    "|" => "bit32.bor(a, b)",
                    "<<" => "bit32.lshift(a, n)",
                    _ => "bit32.rshift(a, n)",
                };
                push(token.line, format!("bitwise operator `{operator}`: use {function}"))
            },
            (Kind::Symbol, "~") => push(
                token.line,
                "bitwise operator `~`: use bit32.bxor(a, b) or bit32.bnot(a)".into(),
            ),
            (Kind::Number { hex_float: true }, text) => push(
                token.line,
                format!("hexadecimal float `{text}` is not available; write it in decimal"),
            ),
            _ => {},
        }
    }
    hints
}

/// Formats hints as a block appended to a Luau error message.
pub(super) fn hint_block(hints: &[String]) -> Option<String> {
    if hints.is_empty() {
        return None;
    }
    let mut block =
        String::from("Pebrel configuration runs on Luau; this file uses Lua 5.4-only syntax:");
    for hint in hints {
        block.push_str("\n  ");
        block.push_str(hint);
    }
    Some(block)
}

fn attribute(tokens: &[Token<'_>], open: usize) -> Option<(usize, usize)> {
    let opens = tokens.get(open).is_some_and(|t| is_symbol(t, "<"));
    let named = tokens.get(open + 1).is_some_and(|t| t.kind == Kind::Name);
    let closes = tokens.get(open + 2).is_some_and(|t| is_symbol(t, ">"));
    (opens && named && closes).then_some((open, open + 2))
}

fn is_close_attribute(tokens: &[Token<'_>], open: usize) -> bool {
    open > 0
        && tokens[open - 1].kind == Kind::Name
        && attribute(tokens, open).is_some()
        && is_name(&tokens[open + 1], "close")
}

fn is_name(token: &Token<'_>, text: &str) -> bool {
    token.kind == Kind::Name && token.text == text
}

fn is_symbol(token: &Token<'_>, text: &str) -> bool {
    token.kind == Kind::Symbol && token.text == text
}

fn tokenize(source: &str) -> Vec<Token<'_>> {
    let bytes = source.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line = 1;
    while index < bytes.len() {
        let start = index;
        let start_line = line;
        let byte = bytes[index];
        let kind = match byte {
            b'\n' => {
                line += 1;
                index += 1;
                continue;
            },
            b' ' | b'\t' | b'\r' | 0x0b | 0x0c => {
                index += 1;
                continue;
            },
            b'-' if bytes.get(index + 1) == Some(&b'-') => {
                index += 2;
                match long_bracket(bytes, index) {
                    Some(level) => index = skip_long(bytes, index, level, &mut line),
                    None => {
                        while index < bytes.len() && bytes[index] != b'\n' {
                            index += 1;
                        }
                    },
                }
                continue;
            },
            b'[' if let Some(level) = long_bracket(bytes, index) => {
                index = skip_long(bytes, index, level, &mut line);
                Kind::String
            },
            b'"' | b'\'' => {
                index = skip_quoted(bytes, index, &mut line);
                Kind::String
            },
            b'0'..=b'9' => {
                let (end, hex_float) = scan_number(bytes, index);
                index = end;
                Kind::Number { hex_float }
            },
            b'.' if bytes.get(index + 1).is_some_and(u8::is_ascii_digit) => {
                index = scan_number(bytes, index).0;
                Kind::Number { hex_float: false }
            },
            byte if byte == b'_' || byte.is_ascii_alphabetic() || byte >= 0x80 => {
                while index < bytes.len()
                    && (bytes[index] == b'_'
                        || bytes[index].is_ascii_alphanumeric()
                        || bytes[index] >= 0x80)
                {
                    index += 1;
                }
                Kind::Name
            },
            _ => {
                let rest = &bytes[index..];
                let width =
                    [&b"..."[..], b"..", b"::", b"<<", b">>", b"//", b"==", b"~=", b"<=", b">="]
                        .iter()
                        .find(|symbol| rest.starts_with(symbol))
                        .map_or(1, |symbol| symbol.len());
                index += width;
                Kind::Symbol
            },
        };
        let end = index.min(bytes.len());
        // Multi-byte names are scanned bytewise; every boundary above is ASCII.
        if let Some(text) = source.get(start..end) {
            tokens.push(Token { kind, text, start, end, line: start_line });
        }
    }
    tokens
}

fn long_bracket(bytes: &[u8], index: usize) -> Option<usize> {
    if bytes.get(index) != Some(&b'[') {
        return None;
    }
    let mut cursor = index + 1;
    while bytes.get(cursor) == Some(&b'=') {
        cursor += 1;
    }
    (bytes.get(cursor) == Some(&b'[')).then_some(cursor - index - 1)
}

fn skip_long(bytes: &[u8], index: usize, level: usize, line: &mut usize) -> usize {
    let mut cursor = index + level + 2;
    while cursor < bytes.len() {
        if bytes[cursor] == b'\n' {
            *line += 1;
        } else if bytes[cursor] == b']'
            && bytes.get(cursor + 1 + level) == Some(&b']')
            && bytes[cursor + 1..cursor + 1 + level].iter().all(|byte| *byte == b'=')
        {
            return cursor + level + 2;
        }
        cursor += 1;
    }
    bytes.len()
}

fn skip_quoted(bytes: &[u8], index: usize, line: &mut usize) -> usize {
    let quote = bytes[index];
    let mut cursor = index + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' => {
                // `\z` and escaped newlines may span lines; count them for hints.
                if bytes.get(cursor + 1) == Some(&b'\n') {
                    *line += 1;
                }
                cursor += 2;
                continue;
            },
            b'\n' => return cursor,
            byte if byte == quote => return cursor + 1,
            _ => {},
        }
        cursor += 1;
    }
    bytes.len()
}

fn scan_number(bytes: &[u8], index: usize) -> (usize, bool) {
    let hex = bytes[index] == b'0' && matches!(bytes.get(index + 1), Some(b'x' | b'X'));
    let mut cursor = if hex { index + 2 } else { index };
    let mut fraction = false;
    let mut exponent = false;
    while let Some(&byte) = bytes.get(cursor) {
        let exponent_mark =
            if hex { matches!(byte, b'p' | b'P') } else { matches!(byte, b'e' | b'E') };
        if exponent_mark {
            exponent = true;
            cursor += 1;
            if matches!(bytes.get(cursor), Some(b'+' | b'-')) {
                cursor += 1;
            }
        } else if byte == b'.' && bytes.get(cursor + 1) != Some(&b'.') {
            fraction = true;
            cursor += 1;
        } else if byte.is_ascii_alphanumeric() || byte == b'_' {
            cursor += 1;
        } else {
            break;
        }
    }
    (cursor, hex && (fraction || exponent))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn const_attributes_are_blanked_without_moving_positions() {
        let source = "local a <const>, b <const> = 1, 2\nlocal c<const> = '<const>'\n";
        let prepared = prepare(source);
        assert_eq!(prepared.len(), source.len());
        assert_eq!(prepared.lines().count(), source.lines().count());
        assert!(!prepared.lines().next().unwrap().contains("<const>"));
        assert!(prepared.contains("'<const>'"), "string contents stay untouched");
    }

    #[test]
    fn comparisons_and_unrelated_code_are_left_alone() {
        for source in [
            "return a <const> b",
            "if x < const and y > 1 then end",
            "-- local a <const> = 1",
            "local s = [[local a <const> = 1]]",
            "local function f() end",
        ] {
            assert!(matches!(prepare(source), Cow::Borrowed(_)), "{source}");
        }
    }

    #[test]
    fn hints_name_each_unsupported_construct_with_its_line() {
        let control =
            "local x <close> = f()\ngoto done\n::done::\nlocal h = 0x1p4\nlocal e = _ENV\n";
        let bitwise = "local y = a & b | c ~ d\nlocal z = 1 << 2 >> 1\n";
        let hints = [migration_hints(control), migration_hints(bitwise)].concat().join("\n");
        for expected in [
            "line 1: `<close>`",
            "line 2: goto",
            "line 3: goto labels",
            "line 4: hexadecimal float `0x1p4`",
            "line 5: _ENV",
            "line 1: bitwise operator `&`",
            "line 1: bitwise operator `|`",
            "line 1: bitwise operator `~`",
            "line 2: bitwise operator `<<`",
            "line 2: bitwise operator `>>`",
        ] {
            assert!(hints.contains(expected), "missing {expected:?} in\n{hints}");
        }
        let repeated = "x = a & b\n".repeat(3) + &"y = a | b\nz = a << b\n".repeat(2);
        assert_eq!(migration_hints(&repeated).len(), 3, "one hint per construct");
        let many = format!("{control}{bitwise}");
        assert_eq!(migration_hints(&many).len(), MAX_HINTS);
    }

    #[test]
    fn ordinary_configuration_produces_no_hints() {
        let source = "local pebrel = require 'pebrel'\nlocal c = pebrel.config_builder()\n\
                      c.window = { opacity = 0.9, padding = { x = 0x10 } } -- a & b\n\
                      if a ~= b and 7 // 2 == 3 then c.env = { A = 'x | y' } end\nreturn c\n";
        assert!(migration_hints(source).is_empty());
    }
}

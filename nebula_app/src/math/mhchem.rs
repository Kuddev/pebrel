//! mhchem chemical formula parser: translates `\ce{...}` content to standard TeX.
//!
//! Translation instead of direct IR construction keeps the downstream latex
//! parser and all existing IR/layout/validate pipelines unchanged.
//! Input that cannot be translated returns `None`, letting the caller fall
//! back to source text.

use std::fmt::Write;

/// Translate the content of `\ce{...}` into standard TeX.
///
/// Returns `None` when the input cannot be translated (caller falls back to source).
///
/// Supported syntax:
///
/// | Pattern | TeX output | Example |
/// |---------|-----------|---------|
/// | Element symbol (upper + optional lower) | `\mathrm{...}` | `H` -> `\mathrm{H}` |
/// | Number after element | subscript `_{N}` | `H2` -> `\mathrm{H}_{2}` |
/// | Coefficient (number before element) | upright number | `2H2` -> `\mathrm{2}\mathrm{H}_{2}` |
/// | `+` | `+` | |
/// | `->` | `\to` | |
/// | `<=>` | `\rightleftharpoons` | |
/// | `<->` | `\leftrightarrow` | |
/// | `^` followed by charge (digits then + or -) | superscript `^{...}` | |
/// | `(s)`, `(l)`, `(g)`, `(aq)` | `\text{(...)}` | |
/// | `.` (hydrate dot) | `\cdot` | |
/// | `v` (precipitation) | `\downarrow` | |
/// | `^` (not followed by charge) | `\uparrow` | |
/// | Parenthesized group | as-is | |
///
/// NOT supported (falls back to source):
/// - Arrow-over-text `->[T]`
/// - Stereochemistry markers
/// - SMILES notation
/// - Any unrecognized character or token sequence
pub(crate) fn translate_ce(source: &str) -> Option<String> {
    let s = source.trim();
    if s.is_empty() {
        return None;
    }

    let mut out = String::with_capacity(s.len() * 2);
    let bytes = s.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    let mut elem = String::with_capacity(2);
    let mut sub = String::with_capacity(4);
    let mut coef_ok = true;

    let mut in_chg = false;
    let mut chg = String::with_capacity(4);

    let mut in_state = false;
    let mut state = String::with_capacity(8);

    let mut depth: i32 = 0;

    while i < n {
        let b = bytes[i];

        // --- state label mode ---
        if in_state {
            if b == b')' {
                let label = std::mem::take(&mut state);
                if matches!(label.as_str(), "s" | "l" | "g" | "aq") {
                    let _ = write!(out, "\\text{{({})}}", label);
                } else {
                    out.push('(');
                    out.push_str(&label);
                    out.push(')');
                }
                in_state = false;
                coef_ok = true;
                i += 1;
                continue;
            }
            if b.is_ascii_alphabetic() {
                state.push(b as char);
            } else {
                return None;
            }
            i += 1;
            continue;
        }

        // --- parenthesized group ---
        if depth > 0 {
            if b == b')' {
                depth -= 1;
                if depth == 0 {
                    coef_ok = true;
                    out.push(')');
                    i += 1;
                    continue;
                }
            } else if b == b'(' {
                depth += 1;
            }
            if !b.is_ascii_alphanumeric() && b != b'+' && b != b'-' {
                return None;
            }
            out.push(b as char);
            i += 1;
            continue;
        }

        // --- charge mode ---
        if in_chg {
            match b {
                b'+' | b'-' => {
                    chg.push(b as char);
                    let _ = write!(out, "^{{{}}}", chg);
                    chg.clear();
                    in_chg = false;
                    coef_ok = true;
                    i += 1;
                    continue;
                },
                b'0'..=b'9' => {
                    chg.push(b as char);
                    i += 1;
                    continue;
                },
                _ => return None,
            }
        }

        // --- main state machine ---
        // Helper: flush element + subscript
        // We use a simple function call pattern instead of a closure
        // to avoid lifetime issues.

        match b {
            b' ' | b'\t' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                coef_ok = true;
                i += 1;
            },

            b'0'..=b'9' => {
                if !elem.is_empty() {
                    // number after element -> subscript
                    sub.push(b as char);
                    i += 1;
                    continue;
                }
                flush_elem(&mut out, &mut elem, &mut sub);
                if coef_ok {
                    // coefficient
                    let _ = write!(out, "\\text{{");
                    while i < n && bytes[i].is_ascii_digit() {
                        out.push(bytes[i] as char);
                        i += 1;
                    }
                    out.push('}');
                    coef_ok = false;
                } else {
                    out.push(b as char);
                    i += 1;
                }
                continue;
            },

            // uppercase -> element
            b'A'..=b'Z' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                elem.push(b as char);
                i += 1;
                while i < n && bytes[i].is_ascii_lowercase() {
                    elem.push(bytes[i] as char);
                    i += 1;
                }
                coef_ok = false;
                continue;
            },

            // lowercase (non-element suffix)
            b'a'..=b'z' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                if b == b'v' {
                    out.push_str("\\downarrow ");
                    coef_ok = true;
                    i += 1;
                    continue;
                }
                return None;
            },

            // ->
            b'-' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                if i + 1 < n && bytes[i + 1] == b'>' {
                    out.push_str("\\to ");
                    i += 2;
                    coef_ok = true;
                } else {
                    out.push('-');
                    i += 1;
                    coef_ok = false;
                }
                continue;
            },

            // <=>, <->, <-
            b'<' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                let h3 = i + 2 < n;
                if h3 && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
                    out.push_str("\\leftrightarrow ");
                    i += 3;
                } else if h3 && bytes[i + 1] == b'=' && bytes[i + 2] == b'>' {
                    out.push_str("\\rightleftharpoons ");
                    i += 3;
                } else if i + 1 < n && bytes[i + 1] == b'-' {
                    out.push_str("\\leftarrow ");
                    i += 2;
                } else {
                    out.push('<');
                    i += 1;
                }
                coef_ok = true;
                continue;
            },

            // => or =
            b'=' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                if i + 1 < n && bytes[i + 1] == b'>' {
                    out.push_str("\\to ");
                    i += 2;
                } else {
                    out.push('=');
                    i += 1;
                }
                coef_ok = true;
                continue;
            },

            // +
            b'+' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                out.push('+');
                i += 1;
                coef_ok = true;
                continue;
            },

            // ^
            b'^' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                if i + 1 < n
                    && (bytes[i + 1].is_ascii_digit()
                        || bytes[i + 1] == b'+'
                        || bytes[i + 1] == b'-')
                {
                    in_chg = true;
                    chg.clear();
                    i += 1;
                } else {
                    out.push_str("\\uparrow ");
                    i += 1;
                    coef_ok = true;
                }
                continue;
            },

            // .
            b'.' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                out.push_str("\\cdot ");
                i += 1;
                coef_ok = true;
                continue;
            },

            // (
            b'(' => {
                flush_elem(&mut out, &mut elem, &mut sub);
                let mut close = i + 1;
                while close < n {
                    if bytes[close] == b')' {
                        break;
                    }
                    if !bytes[close].is_ascii_alphabetic() {
                        close = i;
                        break;
                    }
                    close += 1;
                }
                if close > i && close < n && bytes[close] == b')' {
                    let inner = &s[i + 1..close];
                    if is_state_label(inner) {
                        let _ = write!(out, "\\text{{({})}}", inner);
                        i = close + 1;
                        coef_ok = true;
                        continue;
                    }
                }
                depth = 1;
                out.push('(');
                i += 1;
                continue;
            },

            // lone )
            b')' => return None,

            // unrecognized -> fallback
            _ => return None,
        }
    }

    // tail flush
    flush_elem(&mut out, &mut elem, &mut sub);
    if in_chg || depth > 0 || in_state {
        return None;
    }

    Some(out)
}

/// Flush element symbol and subscript into the output buffer.
fn flush_elem(out: &mut String, elem: &mut String, sub: &mut String) {
    if !elem.is_empty() {
        let _ = write!(out, "\\mathrm{{{}}}", elem);
        if !sub.is_empty() {
            let _ = write!(out, "_{{{}}}", sub);
            sub.clear();
        }
        elem.clear();
    } else if !sub.is_empty() {
        let _ = write!(out, "_{{{}}}", sub);
        sub.clear();
    }
}

fn is_state_label(s: &str) -> bool {
    matches!(s, "s" | "l" | "g" | "aq")
}

/// 在 TeX 源码中查找 mhchem 和 siunitx 命令并翻译为标准 TeX。
///
/// 扫描以下命令：
/// - `\ce{...}` → 调用 `translate_ce`
/// - `\unit{...}`, `\si{...}` → 调用 `siunitx::translate_unit`
/// - `\SI{<数>}{<单位>}`, `\qty{<数>}{<单位>}` → 调用 `siunitx::translate_qty`
///
/// 无法翻译的命令保持不变，后续 pulldown-latex 会报错，由调用方的回退逻辑处理。
pub(crate) fn substitute_chemistry_commands(source: &str) -> String {
    let mut out = String::with_capacity(source.len() + 256);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    while i < n {
        if bytes[i] == b'\\' {
            let cmd_start = i + 1;
            let mut cmd_end = cmd_start;
            while cmd_end < n && bytes[cmd_end].is_ascii_alphabetic() {
                cmd_end += 1;
            }
            let cmd = &source[cmd_start..cmd_end];

            let replacement: Option<String> = match cmd {
                "ce" => {
                    let content = braced_content(source, cmd_end);
                    content.and_then(|r| translate_ce(&source[r]))
                },
                "si" | "unit" => {
                    let content = braced_content(source, cmd_end);
                    content.and_then(|r| super::siunitx::translate_unit(&source[r]))
                },
                "SI" | "qty" => {
                    let first = braced_content(source, cmd_end);
                    let second = first.as_ref().and_then(|r| braced_content(source, r.end + 1));
                    match (first, second) {
                        (Some(f), Some(s)) => super::siunitx::translate_qty(&source[f], &source[s]),
                        _ => None,
                    }
                },
                _ => None,
            };

            if let Some(tex) = replacement {
                out.push_str(&tex);
                // Skip past the entire original command and its arguments
                let after = match cmd {
                    "ce" | "si" | "unit" => {
                        braced_content(source, cmd_end).map(|r| r.end + 1).unwrap_or(i + 1)
                    },
                    "SI" | "qty" => {
                        let e1 =
                            braced_content(source, cmd_end).map(|r| r.end + 1).unwrap_or(i + 1);
                        braced_content(source, e1).map(|r| r.end + 1).unwrap_or(i + 1)
                    },
                    _ => i + 1,
                };
                i = after;
                continue;
            }
        }
        // 非反斜杠字符，支持多字节 UTF-8
        let ch = source[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }

    out
}

/// 找到 `{...}` 分组，返回内容（不含花括号）的字节范围。
fn braced_content(source: &str, at: usize) -> Option<std::ops::Range<usize>> {
    let bytes = source.as_bytes();
    let mut p = at;
    while p < bytes.len() && bytes[p].is_ascii_whitespace() {
        p += 1;
    }
    if p >= bytes.len() || bytes[p] != b'{' {
        return None;
    }
    p += 1;
    let start = p;
    let mut depth = 1u32;
    while p < bytes.len() && depth > 0 {
        if bytes[p] == b'{' {
            depth += 1;
        } else if bytes[p] == b'}' {
            depth -= 1;
        }
        p += 1;
    }
    if depth != 0 {
        return None;
    }
    Some(start..p - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_reaction() {
        let t = translate_ce("2H2 + O2 -> 2H2O").expect("basic reaction");
        assert!(t.contains(r"\mathrm{H}_{2}"));
        assert!(t.contains(r"\mathrm{O}_{2}"));
        assert!(t.contains(r"\to "));
        assert!(t.contains(r"\text{2}"));
    }

    #[test]
    fn test_charge_ions() {
        let t = translate_ce("SO4^2-").expect("sulfate");
        assert!(t.contains(r"\mathrm{S}"));
        assert!(t.contains(r"\mathrm{O}_{4}"));
        assert!(t.contains("^{2-}"));

        let t = translate_ce("H^+").expect("proton");
        assert!(t.contains("^{+}"));
    }

    #[test]
    fn test_equilibrium() {
        let t = translate_ce("A <=> B").expect("equilibrium");
        assert!(t.contains(r"\rightleftharpoons "));
    }

    #[test]
    fn test_resonance() {
        let t = translate_ce("A <-> B").expect("resonance");
        assert!(t.contains(r"\leftrightarrow "));
    }

    #[test]
    fn test_hydrate() {
        let t = translate_ce("CuSO4.5H2O").expect("hydrate");
        assert!(t.contains(r"\mathrm{Cu}"));
        assert!(t.contains(r"\mathrm{S}"));
        assert!(t.contains(r"\mathrm{O}_{4}"));
        assert!(t.contains(r"\cdot "));
    }

    #[test]
    fn test_precipitation() {
        let t = translate_ce("CaCO3 v").expect("precipitation");
        assert!(t.contains(r"\downarrow "));
    }

    #[test]
    fn test_state_labels() {
        let t = translate_ce("CO2(aq)").expect("aqueous");
        assert!(t.contains(r"\text{(aq)}"));

        let t = translate_ce("Fe(s)").expect("solid");
        assert!(t.contains(r"\text{(s)}"));
    }

    #[test]
    fn test_gas_arrow() {
        let t = translate_ce("NH4^").expect("gas arrow");
        assert!(t.contains(r"\uparrow "));
    }

    #[test]
    fn test_parenthesized_group() {
        let t = translate_ce("Fe(OH)2").expect("parenthesized");
        assert!(t.contains(r"\mathrm{Fe}"));
        assert!(t.contains("("));
        assert!(t.contains(")"));
    }

    #[test]
    fn test_empty_fallback() {
        assert!(translate_ce("").is_none());
        assert!(translate_ce("z").is_none());
    }

    #[test]
    fn test_unicode_fallback() {
        assert!(translate_ce("\u{03b1}").is_none());
    }
}

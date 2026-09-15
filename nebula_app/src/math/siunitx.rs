//! siunitx unit macro translator: converts `\unit{...}`, `\si{...}`, `\qty{...}{...}`,
//! and `\SI{...}{...}` content into standard TeX that pulldown-latex can parse.
//!
//! Non-translatable input returns `None`, letting the caller fall back to source text.

use std::collections::HashMap;

/// Build a lookup table of siunitx unit macros to their TeX representation.
fn unit_map() -> HashMap<&'static str, &'static str> {
    let mut m = HashMap::with_capacity(48);

    // SI prefixes
    m.insert("yocto", "y");
    m.insert("zepto", "z");
    m.insert("atto", "a");
    m.insert("femto", "f");
    m.insert("pico", "p");
    m.insert("nano", "n");
    m.insert("micro", "\u{00b5}");
    m.insert("milli", "m");
    m.insert("centi", "c");
    m.insert("deci", "d");
    m.insert("deka", "da");
    m.insert("hecto", "h");
    m.insert("kilo", "k");
    m.insert("mega", "M");
    m.insert("giga", "G");
    m.insert("tera", "T");
    m.insert("peta", "P");
    m.insert("exa", "E");
    m.insert("zetta", "Z");
    m.insert("yotta", "Y");

    // Base units
    m.insert("metre", "m");
    m.insert("meter", "m");
    m.insert("gram", "g");
    m.insert("second", "s");
    m.insert("ampere", "A");
    m.insert("kelvin", "K");
    m.insert("mole", "mol");
    m.insert("candela", "cd");

    // Derived
    m.insert("degreeCelsius", "\u{00b0}C");
    m.insert("percent", "%");
    m.insert("liter", "L");
    m.insert("litre", "L");
    m.insert("molar", "M");
    m.insert("newton", "N");
    m.insert("pascal", "Pa");
    m.insert("joule", "J");
    m.insert("watt", "W");
    m.insert("volt", "V");
    m.insert("hertz", "Hz");

    // Modifiers
    m.insert("per", "/");
    m.insert("squared", "^{2}");
    m.insert("cubic", "^{3}");

    m
}

/// Translate the content of `\unit{...}` or `\si{...}` into TeX `\mathrm{...}` form.
///
/// Input is a sequence of siunitx unit macros separated by `\per`, `\squared`,
/// `\cubic`, `.`, or `~`.
pub(crate) fn translate_unit(source: &str) -> Option<String> {
    if source.trim().is_empty() {
        return None;
    }

    let map = unit_map();
    let mut out = String::with_capacity(source.len() + 16);
    let bytes = source.as_bytes();
    let n = bytes.len();
    let mut i = 0usize;

    out.push_str("\\mathrm{");

    while i < n {
        let b = bytes[i];

        if b == b'\\' {
            i += 1;
            if i >= n {
                return None;
            }
            let mut name = String::with_capacity(16);
            while i < n && bytes[i].is_ascii_alphabetic() {
                name.push(bytes[i] as char);
                i += 1;
            }
            if name.is_empty() {
                return None;
            }

            if let Some(tex) = map.get(name.as_str()) {
                out.push_str(tex);
            } else {
                // Unknown macro -> fallback
                return None;
            }
        } else if b == b' ' || b == b'\t' {
            i += 1;
        } else if b == b'~' || b == b'.' {
            out.push_str("\\,");
            i += 1;
        } else if b == b'{' || b == b'}' {
            return None;
        } else {
            return None;
        }
    }

    out.push('}');
    Some(out)
}

/// Translate `\qty{<number>}{<unit>}` or `\SI{<number>}{<unit>}` into TeX.
pub(crate) fn translate_qty(number: &str, unit: &str) -> Option<String> {
    let unit_tex = translate_unit(unit)?;
    if number.is_empty() {
        return None;
    }

    let valid_number = number
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-');
    if !valid_number {
        return None;
    }

    let mut out = String::with_capacity(number.len() + unit_tex.len() + 4);
    out.push_str(number);
    out.push_str("\\,");
    out.push_str(&unit_tex);
    Some(out)
}

/// Find a `{...}` group starting at or after `at`. Returns the byte range of
/// the content (not including braces).
pub(crate) fn braced_content(source: &str, at: usize) -> Option<std::ops::Range<usize>> {
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
    fn test_braced_content() {
        let range = braced_content(r"\ce{2H2}", 3).expect("should find brace");
        assert_eq!(&r"\ce{2H2}"[range], "2H2");
    }

    #[test]
    fn test_translate_unit() {
        let t = translate_unit(r"\kilo\gram").expect("kg");
        assert!(t.contains(r"\mathrm{kg}"));
    }

    #[test]
    fn test_translate_unit_per() {
        let t = translate_unit(r"\metre\per\second").expect("m/s");
        assert!(t.contains(r"\mathrm{m/s}"));
    }

    #[test]
    fn test_translate_qty() {
        let t = translate_qty("1.2", r"\kilo\gram").expect("1.2 kg");
        assert!(t.contains("1.2"));
        assert!(t.contains(r"\mathrm{kg}"));
    }

    #[test]
    fn test_empty_fallback() {
        assert!(translate_unit("").is_none());
        assert!(translate_unit(r"\unknown").is_none());
    }
}

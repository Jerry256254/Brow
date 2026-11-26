//! Own CSS parser: stylesheets, rules, selectors and color values.

use crate::html::Document;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color { r, g, b, a: 255 }
    }
}

pub fn parse_color(input: &str) -> Option<Color> {
    let s = input.trim().to_lowercase();
    if let Some(hex) = s.strip_prefix('#') {
        let n = hex.len();
        let val = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
        return match n {
            3 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                Some(Color::rgb(r, g, b))
            }
            4 => {
                let r = u8::from_str_radix(&hex[0..1].repeat(2), 16).ok()?;
                let g = u8::from_str_radix(&hex[1..2].repeat(2), 16).ok()?;
                let b = u8::from_str_radix(&hex[2..3].repeat(2), 16).ok()?;
                let a = u8::from_str_radix(&hex[3..4].repeat(2), 16).ok()?;
                Some(Color { r, g, b, a })
            }
            6 => Some(Color { r: val(0)?, g: val(2)?, b: val(4)?, a: 255 }),
            8 => Some(Color { r: val(0)?, g: val(2)?, b: val(4)?, a: val(6)? }),
            _ => None,
        };
    }
    if s.starts_with("rgb(") || s.starts_with("rgba(") {
        let inner = s
            .trim_start_matches("rgba(")
            .trim_start_matches("rgb(")
            .trim_end_matches(')');
        let parts: Vec<&str> = inner.split(',').map(|p| p.trim()).collect();
        if parts.len() < 3 {
            return None;
        }
        let ch = |p: &str| -> Option<u8> {
            if let Some(pct) = p.strip_suffix('%') {
                pct.parse::<f32>().ok().map(|v| (v * 2.55).round().clamp(0.0, 255.0) as u8)
            } else {
                p.parse::<u8>().ok()
            }
        };
        let a = if parts.len() > 3 {
            parts[3]
                .parse::<f32>()
                .ok()
                .map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8)
                .unwrap_or(255)
        } else {
            255
        };
        return Some(Color { r: ch(parts[0])?, g: ch(parts[1])?, b: ch(parts[2])?, a });
    }
    named_color(&s)
}

pub fn named_color(s: &str) -> Option<Color> {
    let c = match s {
        "black" => Color::rgb(0, 0, 0),
        "white" => Color::rgb(255, 255, 255),
        "red" => Color::rgb(255, 0, 0),
        "green" => Color::rgb(0, 128, 0),
        "lime" => Color::rgb(0, 255, 0),
        "blue" => Color::rgb(0, 0, 255),
        "yellow" => Color::rgb(255, 255, 0),
        "cyan" | "aqua" => Color::rgb(0, 255, 255),
        "magenta" | "fuchsia" => Color::rgb(255, 0, 255),
        "gray" | "grey" => Color::rgb(128, 128, 128),
        "silver" => Color::rgb(192, 192, 192),
        "maroon" => Color::rgb(128, 0, 0),
        "olive" => Color::rgb(128, 128, 0),
        "purple" => Color::rgb(128, 0, 128),
        "teal" => Color::rgb(0, 128, 128),
        "navy" => Color::rgb(0, 0, 128),
        "orange" => Color::rgb(255, 165, 0),
        "brown" => Color::rgb(165, 42, 42),
        "pink" => Color::rgb(255, 192, 203),
        "transparent" => Color { r: 0, g: 0, b: 0, a: 0 },
        _ => return None,
    };
    Some(c)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combinator {
    Descendant,
    Child,
}

#[derive(Debug, Clone, Default)]
pub struct Compound {
    pub tag: Option<String>,
    pub id: Option<String>,
    pub classes: Vec<String>,
    pub universal: bool,
}

impl Compound {
    pub fn specificity(&self) -> (u32, u32, u32) {
        (
            self.id.is_some() as u32,
            self.classes.len() as u32,
            self.tag.is_some() as u32,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Selector {
    /// (combinator linking this compound to the previous one, compound).
    /// The first entry's combinator is unused.
    pub parts: Vec<(Combinator, Compound)>,
}

impl Selector {
    pub fn specificity(&self) -> (u32, u32, u32) {
        let mut s = (0, 0, 0);
        for (_, c) in &self.parts {
            let c = c.specificity();
            s.0 += c.0;
            s.1 += c.1;
            s.2 += c.2;
        }
        s
    }
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub selectors: Vec<Selector>,
    pub declarations: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default)]
pub struct Stylesheet {
    pub rules: Vec<Rule>,
}

pub fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find("/*") {
        out.push_str(&rest[..pos]);
        match rest[pos..].find("*/") {
            Some(e) => rest = &rest[pos + e + 2..],
            None => {
                rest = "";
                break;
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn parse_declarations(input: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for part in input.split(';') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some(colon) = part.find(':') {
            let prop = part[..colon].trim().to_lowercase();
            let val = part[colon + 1..].trim().to_string();
            if !prop.is_empty() {
                out.push((prop, val));
            }
        }
    }
    out
}

fn parse_compound(s: &str) -> Compound {
    let mut c = Compound::default();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    if chars.get(0) == Some(&'*') {
        c.universal = true;
        i = 1;
    } else if chars.get(0).map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
        let start = i;
        while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
            i += 1;
        }
        c.tag = Some(chars[start..i].iter().collect::<String>().to_lowercase());
    }
    while i < chars.len() {
        match chars[i] {
            '#' => {
                i += 1;
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    i += 1;
                }
                c.id = Some(chars[start..i].iter().collect());
            }
            '.' => {
                i += 1;
                let start = i;
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '-' || chars[i] == '_') {
                    i += 1;
                }
                c.classes.push(chars[start..i].iter().collect());
            }
            ':' => {
                // Pseudo-classes/elements are parsed and ignored (treated as matching).
                i += 1;
                if i < chars.len() && chars[i] == ':' {
                    i += 1;
                }
                while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '-') {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '(' {
                    let mut depth = 1;
                    i += 1;
                    while i < chars.len() && depth > 0 {
                        if chars[i] == '(' {
                            depth += 1;
                        } else if chars[i] == ')' {
                            depth -= 1;
                        }
                        i += 1;
                    }
                }
            }
            '[' => {
                // Attribute selectors: parse and ignore.
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '[' {
                        depth += 1;
                    } else if chars[i] == ']' {
                        depth -= 1;
                    }
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }
    c
}

pub fn parse_selector(input: &str) -> Option<Selector> {
    let mut parts: Vec<(Combinator, Compound)> = Vec::new();
    let mut cur = String::new();
    let mut pending_comb = Combinator::Descendant;
    let mut saw_space = false;
    let flush = |cur: &mut String, parts: &mut Vec<(Combinator, Compound)>, comb: Combinator| {
        let t = cur.trim();
        if !t.is_empty() {
            parts.push((comb, parse_compound(t)));
            cur.clear();
        }
    };
    for c in input.chars() {
        match c {
            '>' => {
                flush(&mut cur, &mut parts, pending_comb);
                pending_comb = Combinator::Child;
                saw_space = false;
            }
            c if c.is_whitespace() => {
                if !cur.trim().is_empty() {
                    saw_space = true;
                }
            }
            _ => {
                if saw_space && !cur.trim().is_empty() {
                    flush(&mut cur, &mut parts, pending_comb);
                    pending_comb = Combinator::Descendant;
                    saw_space = false;
                }
                cur.push(c);
            }
        }
    }
    flush(&mut cur, &mut parts, pending_comb);
    if parts.is_empty() {
        None
    } else {
        Some(Selector { parts })
    }
}

pub fn parse_stylesheet(input: &str) -> Stylesheet {
    let s = strip_comments(input);
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    let mut rules = Vec::new();
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        if chars[i] == '@' {
            // Skip at-rules (@media, @import, ...): skip to matching block or semicolon.
            while i < chars.len() && chars[i] != '{' && chars[i] != ';' {
                i += 1;
            }
            if i < chars.len() && chars[i] == '{' {
                let mut depth = 1;
                i += 1;
                while i < chars.len() && depth > 0 {
                    if chars[i] == '{' {
                        depth += 1;
                    } else if chars[i] == '}' {
                        depth -= 1;
                    }
                    i += 1;
                }
            } else if i < chars.len() {
                i += 1;
            }
            continue;
        }
        let start = i;
        while i < chars.len() && chars[i] != '{' {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        let sel_text: String = chars[start..i].iter().collect();
        i += 1;
        let dstart = i;
        let mut depth = 1;
        while i < chars.len() && depth > 0 {
            if chars[i] == '{' {
                depth += 1;
            } else if chars[i] == '}' {
                depth -= 1;
            }
            i += 1;
        }
        let dend = if depth == 0 { i - 1 } else { i };
        let decl_text: String = chars[dstart..dend].iter().collect();
        let mut selectors = Vec::new();
        for part in sel_text.split(',') {
            if let Some(sel) = parse_selector(part) {
                selectors.push(sel);
            }
        }
        if !selectors.is_empty() {
            rules.push(Rule {
                selectors,
                declarations: parse_declarations(&decl_text),
            });
        }
    }
    Stylesheet { rules }
}

pub fn matches_compound(doc: &Document, el: usize, c: &Compound) -> bool {
    let e = match doc.element(el) {
        Some(e) if !e.tag.starts_with('#') => e,
        _ => return false,
    };
    if let Some(tag) = &c.tag {
        if !e.tag.eq_ignore_ascii_case(tag) {
            return false;
        }
    }
    if let Some(id) = &c.id {
        if e.id() != Some(id.as_str()) {
            return false;
        }
    }
    let classes = e.classes();
    for cl in &c.classes {
        if !classes.contains(&cl.as_str()) {
            return false;
        }
    }
    true
}

pub fn matches_selector(doc: &Document, sel: &Selector, el: usize) -> bool {
    let parts = &sel.parts;
    let last = &parts[parts.len() - 1].1;
    if !matches_compound(doc, el, last) {
        return false;
    }
    let mut cur = el;
    for j in (1..parts.len()).rev() {
        let (comb, target) = (&parts[j].0, &parts[j - 1].1);
        match comb {
            Combinator::Child => {
                let p = match doc.parent(cur) {
                    Some(p) if doc.is_element(p) => p,
                    _ => return false,
                };
                if !matches_compound(doc, p, target) {
                    return false;
                }
                cur = p;
            }
            Combinator::Descendant => {
                let mut p = doc.parent(cur);
                let mut found = None;
                while let Some(a) = p {
                    if doc.is_element(a) {
                        if matches_compound(doc, a, target) {
                            found = Some(a);
                            break;
                        }
                    }
                    p = doc.parent(a);
                }
                match found {
                    Some(a) => cur = a,
                    None => return false,
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html;

    #[test]
    fn colors() {
        assert_eq!(parse_color("#fff"), Some(Color::rgb(255, 255, 255)));
        assert_eq!(parse_color("#ff0000"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(parse_color("rgb(1, 2, 3)"), Some(Color::rgb(1, 2, 3)));
        assert_eq!(parse_color("rgba(0,0,0,0.5)"), Some(Color { r: 0, g: 0, b: 0, a: 128 }));
        assert_eq!(parse_color("red"), Some(Color::rgb(255, 0, 0)));
        assert_eq!(parse_color("bogus"), None);
    }

    #[test]
    fn parses_rules() {
        let sheet = parse_stylesheet("h1, .big { color: #ff0000; margin: 0 auto; } /* c */ p { font-size: 14px }");
        assert_eq!(sheet.rules.len(), 2);
        assert_eq!(sheet.rules[0].selectors.len(), 2);
        assert_eq!(sheet.rules[0].declarations[0], ("color".to_string(), "#ff0000".to_string()));
        assert_eq!(sheet.rules[1].declarations[0].0, "font-size");
    }

    #[test]
    fn selector_matching() {
        let doc = html::parse("<html><body><div class=\"a\"><p id=\"x\">hi</p></div></body></html>");
        let p = doc.find_first("p").unwrap();
        let div = doc.find_first("div").unwrap();
        assert!(matches_selector(&doc, &parse_selector("p#x").unwrap(), p));
        assert!(matches_selector(&doc, &parse_selector("div.a p").unwrap(), p));
        assert!(matches_selector(&doc, &parse_selector("div > p").unwrap(), p));
        assert!(!matches_selector(&doc, &parse_selector("span p").unwrap(), p));
        assert!(matches_selector(&doc, &parse_selector("div.a > p#x").unwrap(), p));
        assert!(!matches_selector(&doc, &parse_selector("body > p").unwrap(), p));
        assert!(doc.is_element(div));
    }

    #[test]
    fn specificity_order() {
        let a = parse_selector("#id").unwrap().specificity();
        let b = parse_selector(".c").unwrap().specificity();
        let c = parse_selector("p").unwrap().specificity();
        assert!(a > b && b > c);
    }
}

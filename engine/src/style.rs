//! Cascade: UA stylesheet + author stylesheets + inline styles -> Computed style.

use crate::css::{self, Color, Selector, Stylesheet};
use crate::html::Document;

pub const UA_CSS: &str = r#"
html, body { display: block; margin: 0; padding: 0; }
body { display: block; margin: 8px; color: #111111; background-color: #ffffff; font-size: 16px; }
p { display: block; margin: 16px 0; }
h1 { display: block; font-size: 32px; font-weight: bold; margin: 21px 0; }
h2 { display: block; font-size: 24px; font-weight: bold; margin: 19px 0; }
h3 { display: block; font-size: 19px; font-weight: bold; margin: 18px 0; }
h4 { display: block; font-size: 16px; font-weight: bold; margin: 16px 0; }
h5 { display: block; font-size: 13px; font-weight: bold; margin: 15px 0; }
h6 { display: block; font-size: 11px; font-weight: bold; margin: 14px 0; }
div, section, article, header, footer, main, nav, aside, form, fieldset,
blockquote, pre, figure, figcaption, address, details { display: block; }
ul, ol { display: block; margin: 16px 0; padding: 0 0 0 40px; }
li { display: block; }
a { display: inline; color: #0000ee; }
b, strong { display: inline; font-weight: bold; }
i, em, span, code, small, label, u, s, sub, sup { display: inline; }
img { display: inline; }
br { display: inline; }
hr { display: block; height: 2px; background-color: #cccccc; margin: 8px 0; border-width: 0; }
table { display: block; }
tr, td, th { display: block; }
input, button, textarea, select { display: inline; }
script, style, head, meta, title, link, base { display: none; }
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Display {
    Block,
    Inline,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy)]
pub struct Computed {
    pub display: Display,
    pub color: Color,
    pub background: Option<Color>,
    pub width: Option<Length>,
    pub height: Option<Length>,
    /// top, right, bottom, left
    pub margin: [f32; 4],
    pub padding: [f32; 4],
    pub border_width: [f32; 4],
    pub border_color: Color,
    pub font_size: f32,
    pub bold: bool,
    pub align: Align,
}

impl Default for Computed {
    fn default() -> Self {
        Computed {
            display: Display::Inline,
            color: Color::rgb(0, 0, 0),
            background: None,
            width: None,
            height: None,
            margin: [0.0; 4],
            padding: [0.0; 4],
            border_width: [0.0; 4],
            border_color: Color::rgb(0, 0, 0),
            font_size: 16.0,
            bold: false,
            align: Align::Left,
        }
    }
}

pub fn parse_length(s: &str) -> Option<Length> {
    let t = s.trim().to_lowercase();
    if t == "auto" || t.is_empty() {
        return None;
    }
    if let Some(p) = t.strip_suffix('%') {
        return p.parse::<f32>().ok().map(Length::Percent);
    }
    if let Some(px) = t.strip_suffix("px") {
        return px.parse::<f32>().ok().map(Length::Px);
    }
    if let Some(em) = t.strip_suffix("em") {
        return em.parse::<f32>().ok().map(|v| Length::Px(v * 16.0));
    }
    t.parse::<f32>().ok().map(Length::Px)
}

fn parse_box_values(s: &str) -> Option<[f32; 4]> {
    let parts: Vec<f32> = s
        .split_whitespace()
        .filter_map(|p| parse_length(p).map(|l| match l {
            Length::Px(v) => v,
            Length::Percent(_) => 0.0,
        }))
        .collect();
    match parts.len() {
        1 => Some([parts[0]; 4]),
        2 => Some([parts[0], parts[1], parts[0], parts[1]]),
        3 => Some([parts[0], parts[1], parts[2], parts[1]]),
        4 => Some([parts[0], parts[1], parts[2], parts[3]]),
        _ => None,
    }
}

/// Expand shorthand properties into longhands.
pub fn expand_declaration(prop: &str, value: &str) -> Vec<(String, String)> {
    let v = value.trim();
    match prop {
        "margin" | "padding" => match parse_box_values(v) {
            Some(b) => vec![
                (format!("{}-top", prop), b[0].to_string()),
                (format!("{}-right", prop), b[1].to_string()),
                (format!("{}-bottom", prop), b[2].to_string()),
                (format!("{}-left", prop), b[3].to_string()),
            ],
            None => vec![(prop.to_string(), v.to_string())],
        },
        "border-width" => match parse_box_values(v) {
            Some(b) => vec![
                ("border-top-width".to_string(), b[0].to_string()),
                ("border-right-width".to_string(), b[1].to_string()),
                ("border-bottom-width".to_string(), b[2].to_string()),
                ("border-left-width".to_string(), b[3].to_string()),
            ],
            None => vec![(prop.to_string(), v.to_string())],
        },
        "border" => {
            let mut out = Vec::new();
            for tok in v.split_whitespace() {
                if let Some(l) = parse_length(tok) {
                    if let Length::Px(px) = l {
                        out.push(("border-width".to_string(), px.to_string()));
                    }
                } else if css::parse_color(tok).is_some() {
                    out.push(("border-color".to_string(), tok.to_string()));
                }
            }
            if out.is_empty() {
                out.push((prop.to_string(), v.to_string()));
            }
            out
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let side = prop.strip_prefix("border-").unwrap();
            let mut out = Vec::new();
            for tok in v.split_whitespace() {
                if let Some(Length::Px(px)) = parse_length(tok) {
                    out.push((format!("border-{}-width", side), px.to_string()));
                } else if css::parse_color(tok).is_some() {
                    out.push(("border-color".to_string(), tok.to_string()));
                }
            }
            if out.is_empty() {
                out.push((prop.to_string(), v.to_string()));
            }
            out
        }
        "background" | "background-color" => {
            let first = v.split_whitespace().next().unwrap_or("");
            if prop == "background" && css::parse_color(first).is_some() {
                vec![("background-color".to_string(), first.to_string())]
            } else {
                vec![(prop.to_string(), v.to_string())]
            }
        }
        _ => vec![(prop.to_string(), v.to_string())],
    }
}

pub fn apply_declaration(c: &mut Computed, prop: &str, value: &str) {
    for (p, v) in expand_declaration(prop, value) {
        apply_longhand(c, &p, &v);
    }
}

fn apply_longhand(c: &mut Computed, prop: &str, value: &str) {
    let v = value.trim();
    match prop {
        "display" => {
            c.display = match v.to_lowercase().as_str() {
                "none" => Display::None,
                "block" => Display::Block,
                "inline" => Display::Inline,
                _ => c.display,
            }
        }
        "color" => {
            if let Some(col) = css::parse_color(v) {
                c.color = col;
            }
        }
        "background-color" => {
            c.background = css::parse_color(v);
        }
        "width" => c.width = parse_length(v),
        "height" => c.height = parse_length(v),
        "margin-top" | "margin-right" | "margin-bottom" | "margin-left" => {
            if let Some(Length::Px(px)) = parse_length(v) {
                let i = match prop {
                    "margin-top" => 0,
                    "margin-right" => 1,
                    "margin-bottom" => 2,
                    _ => 3,
                };
                c.margin[i] = px;
            }
        }
        "padding-top" | "padding-right" | "padding-bottom" | "padding-left" => {
            if let Some(Length::Px(px)) = parse_length(v) {
                let i = match prop {
                    "padding-top" => 0,
                    "padding-right" => 1,
                    "padding-bottom" => 2,
                    _ => 3,
                };
                c.padding[i] = px;
            }
        }
        "border-width" => {
            if let Some(b) = parse_box_values(v) {
                c.border_width = b;
            }
        }
        "border-top-width" | "border-right-width" | "border-bottom-width" | "border-left-width" => {
            if let Some(Length::Px(px)) = parse_length(v) {
                let i = match prop {
                    "border-top-width" => 0,
                    "border-right-width" => 1,
                    "border-bottom-width" => 2,
                    _ => 3,
                };
                c.border_width[i] = px;
            }
        }
        "border-color" => {
            if let Some(col) = css::parse_color(v) {
                c.border_color = col;
            }
        }
        "font-size" => {
            if let Some(Length::Px(px)) = parse_length(v) {
                c.font_size = px.max(6.0);
            }
        }
        "font-weight" => {
            let w = v.to_lowercase();
            c.bold = w == "bold" || w == "bolder" || w.parse::<u32>().map(|n| n >= 600).unwrap_or(false);
        }
        "text-align" => {
            c.align = match v.to_lowercase().as_str() {
                "center" => Align::Center,
                "right" => Align::Right,
                _ => Align::Left,
            }
        }
        _ => {}
    }
}

/// Parsed UA stylesheet, cached for the process lifetime (perf: it used to be
/// re-parsed for every single element).
pub fn ua_stylesheet() -> &'static Stylesheet {
    static UA: std::sync::OnceLock<Stylesheet> = std::sync::OnceLock::new();
    UA.get_or_init(|| css::parse_stylesheet(UA_CSS))
}

/// Compute the style of an element: UA sheet < author sheets < inline style,
/// ordered by specificity then source order.
pub fn compute_style(doc: &Document, el: usize, author_sheets: &[Stylesheet]) -> Computed {
    let ua = ua_stylesheet();
    let mut sheets: Vec<&Stylesheet> = Vec::with_capacity(author_sheets.len() + 1);
    sheets.push(ua);
    sheets.extend(author_sheets.iter());

    let mut matched: Vec<((u32, u32, u32), usize, &Vec<(String, String)>)> = Vec::new();
    for (order, sheet) in sheets.iter().enumerate() {
        for rule in &sheet.rules {
            let mut best: Option<(u32, u32, u32)> = None;
            for sel in &rule.selectors {
                if css::matches_selector(doc, sel, el) {
                    let s = sel.specificity();
                    if best.map(|b| s > b).unwrap_or(true) {
                        best = Some(s);
                    }
                }
            }
            if let Some(s) = best {
                matched.push((s, order, &rule.declarations));
            }
        }
    }
    matched.sort_by_key(|m| (m.0, m.1));

    let mut c = Computed::default();
    for (_, _, decls) in &matched {
        for (p, v) in decls.iter() {
            apply_declaration(&mut c, p, v);
        }
    }
    if let Some(e) = doc.element(el) {
        if let Some(style_attr) = e.attr("style") {
            for (p, v) in css::parse_declarations(style_attr) {
                apply_declaration(&mut c, &p, &v);
            }
        }
    }
    c
}

#[allow(dead_code)]
pub fn selector_specificity(sel: &Selector) -> (u32, u32, u32) {
    sel.specificity()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html;

    #[test]
    fn ua_styles_apply() {
        let doc = html::parse("<html><body><p>hi</p></body></html>");
        let p = doc.find_first("p").unwrap();
        let c = compute_style(&doc, p, &[]);
        assert_eq!(c.display, Display::Block);
        assert_eq!(c.margin[0], 16.0);
        let body = doc.find_first("body").unwrap();
        let cb = compute_style(&doc, body, &[]);
        assert_eq!(cb.margin[0], 8.0);
        assert_eq!(cb.background, Some(Color::rgb(255, 255, 255)));
    }

    #[test]
    fn author_and_inline_override() {
        let doc = html::parse("<html><body><p style=\"color: #00ff00\">hi</p></body></html>");
        let sheet = css::parse_stylesheet("p { color: #ff0000; font-size: 20px; }");
        let p = doc.find_first("p").unwrap();
        let c = compute_style(&doc, p, &[sheet]);
        assert_eq!(c.color, Color::rgb(0, 255, 0));
        assert_eq!(c.font_size, 20.0);
    }

    #[test]
    fn specificity_beats_order() {
        let doc = html::parse("<html><body><p class=\"x\">hi</p></body></html>");
        let s1 = css::parse_stylesheet("p { color: #ff0000; }");
        let s2 = css::parse_stylesheet(".x { color: #0000ff; }");
        let p = doc.find_first("p").unwrap();
        let c = compute_style(&doc, p, &[s1, s2]);
        assert_eq!(c.color, Color::rgb(0, 0, 255));
    }

    #[test]
    fn shorthand_expansion() {
        let mut c = Computed::default();
        apply_declaration(&mut c, "margin", "1px 2px 3px 4px");
        assert_eq!(c.margin, [1.0, 2.0, 3.0, 4.0]);
        apply_declaration(&mut c, "padding", "8px");
        assert_eq!(c.padding, [8.0; 4]);
        apply_declaration(&mut c, "border", "2px solid red");
        assert_eq!(c.border_width, [2.0; 4]);
        assert_eq!(c.border_color, Color::rgb(255, 0, 0));
        apply_declaration(&mut c, "width", "50%");
        assert_eq!(c.width, Some(Length::Percent(50.0)));
    }
}

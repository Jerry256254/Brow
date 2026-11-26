//! Own HTML parser: tokenizer + tree builder producing an arena-based DOM.
//! Supports the practical HTML subset: elements, attributes, comments,
//! doctype, void elements, raw-text elements (script/style/title/textarea),
//! implied end tags and entity decoding.

#[derive(Debug, Clone)]
pub enum Node {
    Element(Element),
    Text(String),
}

#[derive(Debug, Clone)]
pub struct Element {
    pub tag: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<usize>,
}

impl Element {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
    pub fn id(&self) -> Option<&str> {
        self.attr("id")
    }
    pub fn classes(&self) -> Vec<&str> {
        self.attr("class")
            .map(|c| c.split_whitespace().collect())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub parents: Vec<Option<usize>>,
    pub root: usize,
}

impl Document {
    pub fn element(&self, idx: usize) -> Option<&Element> {
        match self.nodes.get(idx) {
            Some(Node::Element(e)) => Some(e),
            _ => None,
        }
    }
    pub fn text(&self, idx: usize) -> Option<&str> {
        match self.nodes.get(idx) {
            Some(Node::Text(t)) => Some(t),
            _ => None,
        }
    }
    pub fn tag(&self, idx: usize) -> Option<&str> {
        self.element(idx).map(|e| e.tag.as_str())
    }
    pub fn parent(&self, idx: usize) -> Option<usize> {
        self.parents.get(idx).copied().flatten()
    }
    pub fn children(&self, idx: usize) -> &[usize] {
        self.element(idx).map(|e| e.children.as_slice()).unwrap_or(&[])
    }
    pub fn is_element(&self, idx: usize) -> bool {
        matches!(self.nodes.get(idx), Some(Node::Element(e)) if !e.tag.starts_with('#'))
    }
    pub fn find_first(&self, tag: &str) -> Option<usize> {
        self.find_all(tag).into_iter().next()
    }
    pub fn find_all(&self, tag: &str) -> Vec<usize> {
        let mut out = Vec::new();
        let mut queue = vec![self.root];
        let mut i = 0;
        while i < queue.len() {
            let n = queue[i];
            i += 1;
            if let Some(t) = self.tag(n) {
                if t == tag {
                    out.push(n);
                }
            }
            queue.extend_from_slice(self.children(n));
        }
        out
    }
    pub fn text_content(&self, idx: usize) -> String {
        let mut s = String::new();
        let mut queue = vec![idx];
        let mut i = 0;
        while i < queue.len() {
            let n = queue[i];
            i += 1;
            match self.nodes.get(n) {
                Some(Node::Text(t)) => s.push_str(t),
                Some(Node::Element(_)) => {
                    queue.extend_from_slice(self.children(n));
                }
                None => {}
            }
        }
        s
    }
}

const VOID: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
    "source", "track", "wbr",
];

const RAWTEXT: &[&str] = &["script", "style", "textarea", "title"];

fn is_void(tag: &str) -> bool {
    VOID.contains(&tag)
}

fn is_rawtext(tag: &str) -> bool {
    RAWTEXT.contains(&tag)
}

fn implied_close(new: &str, top: &str) -> bool {
    match new {
        "li" => top == "li",
        "dt" | "dd" => top == "dt" || top == "dd",
        "td" | "th" => top == "td" || top == "th",
        "tr" => matches!(top, "tr" | "td" | "th"),
        "option" => top == "option",
        "address" | "article" | "aside" | "blockquote" | "details" | "div" | "dl"
        | "fieldset" | "figcaption" | "figure" | "footer" | "form" | "h1" | "h2" | "h3"
        | "h4" | "h5" | "h6" | "header" | "hgroup" | "hr" | "main" | "menu" | "nav"
        | "ol" | "p" | "pre" | "section" | "table" | "ul" => top == "p",
        _ => false,
    }
}

/// Maximum DOM nesting depth; deeper elements are flattened (anti stack-overflow).
pub const MAX_DEPTH: usize = 512;
/// Longest entity reference we try to decode ("&" + 32 chars + ";").
const MAX_ENTITY_LEN: usize = 34;

fn decode_one(ent: &str) -> Option<char> {
    match ent {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        "nbsp" => Some('\u{00a0}'),
        _ => {
            if let Some(num) = ent.strip_prefix("#x").or_else(|| ent.strip_prefix("#X")) {
                u32::from_str_radix(num, 16).ok().and_then(char::from_u32)
            } else if let Some(num) = ent.strip_prefix('#') {
                num.parse::<u32>().ok().and_then(char::from_u32)
            } else {
                None
            }
        }
    }
}

pub fn decode_entities(input: &str) -> String {
    if !input.contains('&') {
        return input.to_string();
    }
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        let tail = &rest[pos..];
        // Bound the ';' search so hostile input ("&&&&...") stays O(n).
        let window = &tail[..tail.len().min(MAX_ENTITY_LEN)];
        match window.find(';') {
            Some(rel) if (1..=32).contains(&rel) => {
                let ent = &window[1..rel];
                match decode_one(ent) {
                    Some(c) => {
                        out.push(c);
                        rest = &tail[rel + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &tail[1..];
                    }
                }
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn len(&self) -> usize {
        self.chars.len()
    }
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }
    fn peek_at(&self, n: usize) -> Option<char> {
        self.chars.get(self.pos + n).copied()
    }
    fn starts_with(&self, s: &str) -> bool {
        let mut i = self.pos;
        for c in s.chars() {
            if self.chars.get(i) != Some(&c) {
                return false;
            }
            i += 1;
        }
        true
    }
    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(c) if c.is_whitespace()) {
            self.pos += 1;
        }
    }
    fn skip_until(&mut self, s: &str) {
        let pat: Vec<char> = s.chars().collect();
        while self.pos < self.len() {
            let mut i = self.pos;
            let mut ok = true;
            for c in &pat {
                if self.chars.get(i) != Some(c) {
                    ok = false;
                    break;
                }
                i += 1;
            }
            if ok {
                self.pos = i;
                return;
            }
            self.pos += 1;
        }
        self.pos = self.len();
    }
    fn skip_until_char(&mut self, c: char) {
        while let Some(ch) = self.peek() {
            self.pos += 1;
            if ch == c {
                return;
            }
        }
    }
    fn read_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':' {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.chars[start..self.pos].iter().collect()
    }
    fn read_attr_name(&mut self) -> String {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || c == '=' || c == '>' || c == '/' {
                break;
            }
            self.pos += 1;
        }
        self.chars[start..self.pos].iter().collect()
    }
    fn read_attr_value(&mut self) -> String {
        match self.peek() {
            Some('"') => {
                self.pos += 1;
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == '"' {
                        break;
                    }
                    self.pos += 1;
                }
                let v: String = self.chars[start..self.pos].iter().collect();
                if self.peek() == Some('"') {
                    self.pos += 1;
                }
                v
            }
            Some('\'') => {
                self.pos += 1;
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c == '\'' {
                        break;
                    }
                    self.pos += 1;
                }
                let v: String = self.chars[start..self.pos].iter().collect();
                if self.peek() == Some('\'') {
                    self.pos += 1;
                }
                v
            }
            _ => {
                let start = self.pos;
                while let Some(c) = self.peek() {
                    if c.is_whitespace() || c == '>' {
                        break;
                    }
                    self.pos += 1;
                }
                self.chars[start..self.pos].iter().collect()
            }
        }
    }
    fn read_raw_until_close(&mut self, tag: &str) -> String {
        let close: Vec<char> = format!("</{}", tag).chars().collect();
        let mut i = self.pos;
        while i < self.len() {
            let mut j = i;
            let mut ok = true;
            for c in &close {
                let c = c.to_ascii_lowercase();
                match self.chars.get(j) {
                    Some(&ch) if ch.to_ascii_lowercase() == c => j += 1,
                    _ => {
                        ok = false;
                        break;
                    }
                }
            }
            if ok {
                match self.chars.get(j) {
                    Some(c) if c.is_whitespace() || *c == '>' || *c == '/' => {
                        let text: String = self.chars[self.pos..i].iter().collect();
                        self.pos = j;
                        self.skip_until_char('>');
                        return text;
                    }
                    _ => {}
                }
            }
            i += 1;
        }
        let text: String = self.chars[self.pos..].iter().collect();
        self.pos = self.len();
        text
    }
}

pub fn parse(input: &str) -> Document {
    let mut p = Parser {
        chars: input.chars().collect(),
        pos: 0,
    };
    let mut doc = Document {
        nodes: vec![Node::Element(Element {
            tag: "#document".to_string(),
            attrs: Vec::new(),
            children: Vec::new(),
        })],
        parents: vec![None],
        root: 0,
    };
    let mut stack: Vec<usize> = vec![0];

    fn push_text(doc: &mut Document, stack: &[usize], text: String) {
        if text.is_empty() {
            return;
        }
        let idx = doc.nodes.len();
        doc.nodes.push(Node::Text(text));
        doc.parents.push(Some(*stack.last().unwrap()));
        if let Some(Node::Element(e)) = doc.nodes.get_mut(*stack.last().unwrap()) {
            e.children.push(idx);
        }
    }

    while p.pos < p.len() {
        if p.peek() == Some('<') {
            if p.starts_with("<!--") {
                p.pos += 4;
                p.skip_until("-->");
            } else if p.starts_with("<!") || p.starts_with("<?") {
                p.skip_until_char('>');
            } else if p.starts_with("</") {
                p.pos += 2;
                let name = p.read_name().to_lowercase();
                p.skip_until_char('>');
                if let Some(pos) = stack
                    .iter()
                    .rposition(|&i| doc.tag(i).map(|t| t == name).unwrap_or(false))
                {
                    stack.truncate(pos);
                }
            } else if p.peek_at(1).map(|c| c.is_ascii_alphabetic()).unwrap_or(false) {
                p.pos += 1;
                let name = p.read_name().to_lowercase();
                let mut attrs: Vec<(String, String)> = Vec::new();
                let mut self_closing = false;
                loop {
                    p.skip_ws();
                    match p.peek() {
                        None => break,
                        Some('>') => {
                            p.pos += 1;
                            break;
                        }
                        Some('/') if p.peek_at(1) == Some('>') => {
                            p.pos += 2;
                            self_closing = true;
                            break;
                        }
                        Some(_) => {
                            let an = p.read_attr_name().to_lowercase();
                            p.skip_ws();
                            if p.peek() == Some('=') {
                                p.pos += 1;
                                p.skip_ws();
                                let v = p.read_attr_value();
                                attrs.push((an, decode_entities(&v)));
                            } else if !an.is_empty() {
                                attrs.push((an, String::new()));
                            }
                        }
                    }
                }
                loop {
                    let top = doc.tag(*stack.last().unwrap()).map(|s| s.to_string());
                    match top {
                        Some(t) if implied_close(&name, &t) => {
                            stack.pop();
                        }
                        _ => break,
                    }
                }
                let idx = doc.nodes.len();
                doc.nodes.push(Node::Element(Element {
                    tag: name.clone(),
                    attrs,
                    children: Vec::new(),
                }));
                doc.parents.push(Some(*stack.last().unwrap()));
                let parent = *stack.last().unwrap();
                if let Some(Node::Element(e)) = doc.nodes.get_mut(parent) {
                    e.children.push(idx);
                }
                if is_rawtext(&name) && !self_closing && !is_void(&name) {
                    let raw = p.read_raw_until_close(&name);
                    if !raw.is_empty() {
                        let tidx = doc.nodes.len();
                        doc.nodes.push(Node::Text(raw));
                        doc.parents.push(Some(idx));
                        if let Some(Node::Element(e)) = doc.nodes.get_mut(idx) {
                            e.children.push(tidx);
                        }
                    }
                } else if !is_void(&name) && !self_closing {
                    // Cap nesting depth: deeper tags become empty leaves, so
                    // later recursive passes (layout/paint) cannot overflow the stack.
                    if stack.len() < MAX_DEPTH {
                        stack.push(idx);
                    }
                }
            } else {
                push_text(&mut doc, &stack, "<".to_string());
                p.pos += 1;
            }
        } else {
            let start = p.pos;
            while p.pos < p.len() && p.peek() != Some('<') {
                p.pos += 1;
            }
            let raw: String = p.chars[start..p.pos].iter().collect();
            push_text(&mut doc, &stack, decode_entities(&raw));
        }
    }
    doc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_tree() {
        let doc = parse("<!DOCTYPE html><html><body><h1 id=\"t\">Hello</h1><p>A &amp; B</p></body></html>");
        let h1 = doc.find_first("h1").expect("h1");
        assert_eq!(doc.element(h1).unwrap().id(), Some("t"));
        assert_eq!(doc.text_content(h1), "Hello");
        let p = doc.find_first("p").expect("p");
        assert_eq!(doc.text_content(p), "A & B");
    }

    #[test]
    fn decodes_entities() {
        assert_eq!(decode_entities("&lt;a&gt;&quot;&#65;&#x42;&nbsp;"), "<a>\"AB\u{00a0}");
        assert_eq!(decode_entities("plain &unknown; &"), "plain &unknown; &");
    }

    #[test]
    fn entity_bomb_is_linear() {
        let bomb = format!("&{}x;", "&".repeat(200_000));
        let out = decode_entities(&bomb);
        assert!(out.starts_with('&'));
        assert!(out.len() < bomb.len() + 10);
        // no semicolon at all: stays literal, linear
        let out2 = decode_entities(&"&".repeat(200_000));
        assert_eq!(out2.len(), 200_000);
    }

    #[test]
    fn depth_is_capped() {
        let doc = parse(&format!("{}<p>deep</p>{}", "<div>".repeat(100_000), "</div>".repeat(100_000)));
        let mut max_depth = 0;
        let mut stack = vec![(doc.root, 0usize)];
        while let Some((i, d)) = stack.pop() {
            max_depth = max_depth.max(d);
            for &c in doc.children(i) {
                stack.push((c, d + 1));
            }
        }
        assert!(max_depth <= MAX_DEPTH + 2, "depth {}", max_depth);
    }

    #[test]
    fn implied_close_of_p() {
        let doc = parse("<p>one<p>two");
        let ps = doc.find_all("p");
        assert_eq!(ps.len(), 2);
        assert_eq!(doc.text_content(ps[0]), "one");
    }

    #[test]
    fn void_and_self_closing() {
        let doc = parse("<div><br><img src=\"x.png\"/><span>hi</span></div>");
        assert!(doc.find_first("br").is_some());
        assert!(doc.find_first("img").is_some());
        assert_eq!(doc.find_first("span").map(|i| doc.text_content(i)), Some("hi".to_string()));
    }

    #[test]
    fn rawtext_script() {
        let doc = parse("<html><head><script>if (a < b) { x(); }</script></head><body>ok</body></html>");
        let s = doc.find_first("script").expect("script");
        assert_eq!(doc.text_content(s), "if (a < b) { x(); }");
        assert!(doc.find_first("body").is_some());
    }

    #[test]
    fn attributes_unquoted_and_single() {
        let doc = parse("<a href=/x class='c1 c2' target=_blank>x</a>");
        let a = doc.find_first("a").unwrap();
        let e = doc.element(a).unwrap();
        assert_eq!(e.attr("href"), Some("/x"));
        assert_eq!(e.classes(), vec!["c1", "c2"]);
        assert_eq!(e.attr("target"), Some("_blank"));
    }
}

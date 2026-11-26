//! Own layout engine: simplified CSS block/inline box model with word wrapping.

use crate::css::Stylesheet;
use crate::font;
use crate::html::{Document, Node};
use crate::style::{Align, Computed, Display, Length};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoxKind {
    Block,
    Anon,
    Inline,
    Text,
    Br,
    Rect,
}

#[derive(Debug, Clone)]
pub struct LayoutBox {
    pub kind: BoxKind,
    /// border-box origin and size, in CSS px
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub children: Vec<LayoutBox>,
    pub text: String,
    pub style: Computed,
}

impl LayoutBox {
    fn new(kind: BoxKind, style: Computed) -> LayoutBox {
        LayoutBox {
            kind,
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            children: Vec::new(),
            text: String::new(),
            style,
        }
    }
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn is_inline_node(doc: &Document, idx: usize, sheets: &[Stylesheet]) -> bool {
    match doc.nodes.get(idx) {
        Some(Node::Text(_)) => true,
        Some(Node::Element(_)) => {
            let s = crate::style::compute_style(doc, idx, sheets);
            s.display == Display::Inline
        }
        None => false,
    }
}

fn build_inline(doc: &Document, idx: usize, sheets: &[Stylesheet], inherited: &Computed) -> Option<LayoutBox> {
    match doc.nodes.get(idx)? {
        Node::Text(t) => {
            let text = collapse_ws(t);
            if text.is_empty() {
                return None;
            }
            let mut b = LayoutBox::new(BoxKind::Text, *inherited);
            b.text = text;
            Some(b)
        }
        Node::Element(e) => {
            if e.tag == "br" {
                return Some(LayoutBox::new(BoxKind::Br, *inherited));
            }
            let style = crate::style::compute_style(doc, idx, sheets);
            if style.display == Display::None {
                return None;
            }
            let mut b = LayoutBox::new(BoxKind::Inline, style);
            for &c in doc.children(idx) {
                if let Some(child) = build_inline(doc, c, sheets, &style) {
                    b.children.push(child);
                }
            }
            Some(b)
        }
    }
}

fn build_element(doc: &Document, idx: usize, sheets: &[Stylesheet]) -> Option<LayoutBox> {
    let style = crate::style::compute_style(doc, idx, sheets);
    if style.display == Display::None {
        return None;
    }
    let kind = if style.display == Display::Block {
        BoxKind::Block
    } else {
        BoxKind::Inline
    };
    let mut b = LayoutBox::new(kind, style);
    let mut anon: Option<LayoutBox> = None;
    for &c in doc.children(idx) {
        if is_inline_node(doc, c, sheets) {
            if anon.is_none() {
                anon = Some(LayoutBox::new(BoxKind::Anon, b.style));
            }
            if let Some(child) = build_inline(doc, c, sheets, &b.style) {
                anon.as_mut().unwrap().children.push(child);
            }
        } else {
            if let Some(a) = anon.take() {
                if !a.children.is_empty() {
                    b.children.push(a);
                }
            }
            if let Some(child) = build_element(doc, c, sheets) {
                b.children.push(child);
            }
        }
    }
    if let Some(a) = anon.take() {
        if !a.children.is_empty() {
            b.children.push(a);
        }
    }
    Some(b)
}

/// Build the layout tree for a document at a given viewport width and lay it out.
pub fn build(doc: &Document, sheets: &[Stylesheet], viewport_width: f32) -> LayoutBox {
    let mut root_style = Computed::default();
    root_style.display = Display::Block;
    root_style.width = Some(Length::Px(viewport_width));
    root_style.background = Some(crate::css::Color::rgb(255, 255, 255));
    let mut root = LayoutBox::new(BoxKind::Block, root_style);
    for &c in doc.children(doc.root) {
        match doc.nodes.get(c) {
            Some(Node::Text(t)) => {
                let text = collapse_ws(t);
                if !text.is_empty() {
                    let mut anon = LayoutBox::new(BoxKind::Anon, root_style);
                    let mut tb = LayoutBox::new(BoxKind::Text, root_style);
                    tb.text = text;
                    anon.children.push(tb);
                    root.children.push(anon);
                }
            }
            _ => {
                if let Some(b) = build_element(doc, c, sheets) {
                    root.children.push(b);
                }
            }
        }
    }
    layout_block(&mut root, 0.0, 0.0, viewport_width);
    root
}

fn layout_block(b: &mut LayoutBox, x: f32, y: f32, containing: f32) {
    let st = b.style;
    let (mt, mr, _mb, ml) = (st.margin[0], st.margin[1], st.margin[2], st.margin[3]);
    let (bt, br, bb, bl) = (
        st.border_width[0],
        st.border_width[1],
        st.border_width[2],
        st.border_width[3],
    );
    let (pt, pr, pb, pl) = (st.padding[0], st.padding[1], st.padding[2], st.padding[3]);

    let avail = (containing - ml - mr - bl - br).max(0.0);
    let w = match st.width {
        Some(Length::Px(px)) => px.min(avail + bl + br + ml + mr),
        Some(Length::Percent(p)) => (containing * p / 100.0 - ml - mr - bl - br).max(0.0),
        None => avail,
    };
    b.x = x + ml;
    b.y = y + mt;
    b.w = w;

    let content_x = b.x + bl + pl;
    let content_w = (w - bl - br - pl - pr).max(0.0);
    let mut cy = b.y + bt + pt;
    let n = b.children.len();
    for i in 0..n {
        let kind = b.children[i].kind;
        match kind {
            BoxKind::Block => {
                layout_block(&mut b.children[i], content_x, cy, content_w);
                let child = &b.children[i];
                cy = child.y + child.h + child.style.margin[2];
            }
            BoxKind::Anon => {
                layout_anon(&mut b.children[i], content_x, cy, content_w);
                cy += b.children[i].h;
            }
            _ => {}
        }
    }
    let content_h = (cy - (b.y + bt + pt)).max(0.0);
    b.h = match st.height {
        Some(Length::Px(px)) => px,
        _ => content_h + pt + pb + bt + bb,
    };
}

struct Word {
    text: String,
    w: f32,
    style: Computed,
}

fn collect_words(b: &LayoutBox, words: &mut Vec<Word>) {
    for c in &b.children {
        match c.kind {
            BoxKind::Text => {
                for piece in c.text.split(' ') {
                    if piece.is_empty() {
                        continue;
                    }
                    let w = font::text_width(piece, c.style.font_size);
                    words.push(Word {
                        text: piece.to_string(),
                        w,
                        style: c.style,
                    });
                }
            }
            BoxKind::Br => {
                words.push(Word {
                    text: String::new(),
                    w: -1.0,
                    style: c.style,
                });
            }
            BoxKind::Inline | BoxKind::Anon => collect_words(c, words),
            _ => {}
        }
    }
}

fn layout_anon(b: &mut LayoutBox, x: f32, y: f32, width: f32) {
    b.x = x;
    b.y = y;
    b.w = width;

    let mut words: Vec<Word> = Vec::new();
    collect_words(b, &mut words);
    b.children.clear();

    let base_lh = font::line_height(b.style.font_size).max(1.0);
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut cur: Vec<usize> = Vec::new();
    let mut cur_w = 0.0f32;
    for (wi, word) in words.iter().enumerate() {
        if word.w < 0.0 {
            // <br>: force line break
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
            continue;
        }
        let space_w = if cur.is_empty() { 0.0 } else { font::char_advance(' ', font::scale_for(word.style.font_size)) };
        let lh = font::line_height(word.style.font_size).max(1.0);
        if !cur.is_empty() && cur_w + space_w + word.w > width + 0.01 {
            lines.push(std::mem::take(&mut cur));
            cur_w = 0.0;
        }
        let space_w = if cur.is_empty() { 0.0 } else { font::char_advance(' ', font::scale_for(word.style.font_size)) };
        cur_w += space_w + word.w;
        cur.push(wi);
        let _ = lh;
    }
    if !cur.is_empty() {
        lines.push(cur);
    }

    let mut li = 0usize;
    let mut cy = 0.0f32;
    for line in &lines {
        let mut line_w = 0.0f32;
        let mut lh = base_lh;
        for &wi in line {
            line_w += words[wi].w + if line_w > 0.0 { font::char_advance(' ', font::scale_for(words[wi].style.font_size)) } else { 0.0 };
            lh = lh.max(font::line_height(words[wi].style.font_size));
        }
        let offset = match b.style.align {
            Align::Left => 0.0,
            Align::Center => ((width - line_w) / 2.0).max(0.0),
            Align::Right => (width - line_w).max(0.0),
        };
        let mut cx = offset;
        for &wi in line {
            let word = &words[wi];
            let space_w = if cx > offset { font::char_advance(' ', font::scale_for(word.style.font_size)) } else { 0.0 };
            cx += space_w;
            let mut tb = LayoutBox::new(BoxKind::Text, word.style);
            tb.text = word.text.clone();
            tb.x = x + cx;
            tb.y = y + cy;
            tb.w = word.w;
            tb.h = lh;
            b.children.push(tb);
            cx += word.w;
        }
        cy += lh;
        li += 1;
    }
    let _ = li;

    b.h = cy;
}

/// Walk the layout tree (debug / dump purposes).
pub fn dump(b: &LayoutBox, depth: usize, out: &mut String) {
    let kind = match b.kind {
        BoxKind::Block => "BLOCK",
        BoxKind::Anon => "ANON",
        BoxKind::Inline => "INLINE",
        BoxKind::Text => "TEXT",
        BoxKind::Br => "BR",
        BoxKind::Rect => "RECT",
    };
    out.push_str(&format!(
        "{:indent$}{} x={:.0} y={:.0} w={:.0} h={:.0} {}\n",
        "",
        kind,
        b.x,
        b.y,
        b.w,
        b.h,
        b.text,
        indent = depth * 2
    ));
    for c in &b.children {
        dump(c, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html;

    fn layout_html(html_text: &str, width: f32) -> LayoutBox {
        let doc = html::parse(html_text);
        let sheets: Vec<Stylesheet> = Vec::new();
        build(&doc, &sheets, width)
    }

    #[test]
    fn body_fills_viewport_minus_margin() {
        let root = layout_html("<html><body><p>hello</p></body></html>", 200.0);
        assert_eq!(root.w, 200.0);
        let html_box = &root.children[0];
        let body = &html_box.children[0];
        assert_eq!(body.kind, BoxKind::Block);
        // body margin 8 on each side
        assert!((body.w - 184.0).abs() < 0.01, "body w = {}", body.w);
    }

    #[test]
    fn text_wraps_into_lines() {
        let root = layout_html(
            "<html><body><p>one two three four five six seven eight nine ten</p></body></html>",
            60.0,
        );
        let body = &root.children[0].children[0];
        let p = &body.children[0];
        let anon = &p.children[0];
        assert!(anon.children.len() > 1, "expected wrapping, got {} text boxes", anon.children.len());
        assert!(anon.h > font::line_height(16.0));
    }

    #[test]
    fn fixed_width_respected() {
        let root = layout_html(
            "<html><body><div style=\"width: 100px\">x</div></body></html>",
            300.0,
        );
        let body = &root.children[0].children[0];
        let div = &body.children[0];
        assert!((div.w - 100.0).abs() < 0.01);
    }

    #[test]
    fn display_none_removed() {
        let root = layout_html(
            "<html><body><div style=\"display: none\">hidden</div><p>shown</p></body></html>",
            200.0,
        );
        let body = &root.children[0].children[0];
        assert_eq!(body.children.len(), 1);
        assert_eq!(body.children[0].kind, BoxKind::Block);
    }
}

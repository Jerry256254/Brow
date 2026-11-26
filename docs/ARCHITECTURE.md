# KucLab Brow — engine architecture

The engine (`engine/`, crate `brow-engine`) is a from-scratch, zero-dependency
rendering pipeline:

```
URL ──▶ net.rs (HTTP/1.1 | file://) ──▶ adblock.rs (filter check)
     ──▶ html.rs (tokenizer + tree builder) ──▶ DOM (arena)
     ──▶ css.rs / style.rs (parse + cascade) ──▶ Computed styles
     ──▶ layout.rs (box tree, block/inline layout, word wrap) ──▶ LayoutBox tree
     ──▶ paint.rs (software rasterizer, built-in font) ──▶ RGBA framebuffer
     ──▶ png.rs (PNG encoder) or terminal blit (shell)
```

## Pipeline stages

1. **Fetch** — `net::fetch` speaks HTTP/1.1 over raw `TcpStream` (redirects,
   chunked transfer decoding, `Connection: close`). `file://` reads local files.
   Before any connection, `adblock::AdBlock::is_blocked` is consulted; blocked
   requests are never sent and are counted (`Browser::blocked_count`).
2. **Parse** — `html::parse` builds an arena DOM (`Vec<Node>` + parent links),
   handling comments, doctype, attributes, entities, void elements, raw-text
   elements (`script`/`style`/...) and implied end tags (`<p>`, `<li>`, ...).
3. **Style** — `style::compute_style` cascades the built-in UA sheet, all author
   sheets (`<style>` blocks + `<link rel=stylesheet>`, fetched through the
   ad blocker) and inline `style` attributes, ordered by specificity.
   Supported properties: `display`, `color`, `background-color`, `width`,
   `height`, `margin`, `padding`, `border`, `border-color`, `border-width`,
   `font-size`, `font-weight`, `text-align`.
4. **Layout** — `layout::build` creates a `LayoutBox` tree: block boxes stack
   vertically; runs of inline content become anonymous blocks; text is wrapped
   greedily using the built-in font metrics (`font::text_width`).
5. **Paint** — `paint::render` rasterizes backgrounds, borders and glyphs into an
   RGBA `Framebuffer` at an integer device scale.
6. **Output** — `png::write_png` encodes the framebuffer as PNG (own zlib stored
   blocks + CRC32/Adler32); the shell blits it to the terminal with truecolor
   half-block characters.

## Ad blocker filter syntax

- `0.0.0.0 ads.example.com` / `127.0.0.1 tracker.io` — hosts-style (host + subdomains)
- `||ads.example.com^` — domain-anchored (`^` = separator or end); optional path
  pattern after the domain, e.g. `||127.0.0.1/ads.js`
- `*/ads/*`, `*/banner.js` — wildcard patterns matched against the whole URL
- `@@||example.com/allowed.js` — exception (wins over block rules)
- `!` / `[Adblock Plus 2.0]` — comments/headers, ignored

## Deliberate v0.1 scope

- `http://` and `file://` only; `https://` returns a clear error until the TLS
  transport lands (roadmap item, right after packaging).
- No JavaScript, no images yet; `<script>`/`<img>` are fetched only to be
  blocked/counted.
- Layout is a simplified box model (no floats, no flex/grid, no margin collapsing).

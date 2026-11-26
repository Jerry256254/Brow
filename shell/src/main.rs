//! KucLab Brow shell: terminal UI + CLI modes on top of the Brownie engine.

use brownie::adblock::AdBlock;
use brownie::layout::{self, BoxKind};
use brownie::theme;
use brownie::{Browser, Page};
use std::io::{IsTerminal, Read, Write};
use std::process::{Command, Stdio};

const DEFAULT_FILTERS: &str = "filters/blocklist.txt";
const START_PAGE: &str = include_str!("../assets/start.html");

struct Tab {
    url: String,
    page: Option<Page>,
    scroll: f32,
    status: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Err(e) = run(&args) {
        eprintln!("brow: {}", e);
        std::process::exit(1);
    }
}

fn usage() -> String {
    "KucLab Brow 0.1 - own engine (Brownie), no bloat, ad-blocked\n\
     \n\
     USAGE:\n\
     \x20 brow [OPTIONS] [URL]        interactive terminal browser\n\
     \x20 brow --fetch URL            print raw response body\n\
     \x20 brow --dump-dom URL         print parsed DOM tree\n\
     \x20 brow --dump-layout URL      print layout box tree (--width N, default 80)\n\
     \x20 brow --screenshot URL OUT.png [--width N --height N --scale N]\n\
     \x20 brow --bench URL [--iterations N --width N --height N]   pipeline benchmark\n\
     \x20 brow --frame-shot OUT.png [--width N --height N --scale N]   render one UI frame to PNG (design sample)\n\
     \n\
     OPTIONS:\n\
     \x20 --filters PATH              ad-block filter list (default: ./filters/blocklist.txt if present)\n\
     \x20 --no-adblock                disable the ad blocker\n\
     \x20 --width N --height N        viewport for --dump-layout/--screenshot (defaults: 80 / 400 / scale 2)\n\
     \x20 --scale N                   device scale for --screenshot (default 2)\n\
     \x20 -h, --help                  this help\n\
     \n\
     KEYS (interactive): / edit URL, Enter load, j/k or arrows scroll,\n\
     \x20 PgUp/PgDn page, r reload, t new tab, Tab switch tab, a toggle adblock,\n\
     \x20 s screenshot, q quit"
        .to_string()
}

fn make_browser(args: &[String]) -> Result<Browser, String> {
    let mut b = Browser::new();
    if let Some(i) = args.iter().position(|a| a == "--no-adblock") {
        let _ = i;
        return Ok(b);
    }
    if let Some(i) = args.iter().position(|a| a == "--filters") {
        let path = args.get(i + 1).ok_or("--filters needs a path")?;
        b.adblock = AdBlock::load_from_file(path).map_err(|e| format!("cannot load {}: {}", path, e))?;
    } else if std::path::Path::new(DEFAULT_FILTERS).exists() {
        b.adblock = AdBlock::load_from_file(DEFAULT_FILTERS)
            .map_err(|e| format!("cannot load {}: {}", DEFAULT_FILTERS, e))?;
    }
    Ok(b)
}

fn arg_value(args: &[String], flag: &str, default: &str) -> String {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1).cloned())
        .unwrap_or_else(|| default.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{}", usage());
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--fetch") {
        let url = brownie::normalize_url(args.get(i + 1).ok_or("--fetch needs a URL")?);
        let mut b = make_browser(args)?;
        if b.adblock.is_blocked(&url) {
            b.blocked_count += 1;
            return Err(format!("blocked by ad blocker: {}", url));
        }
        let resp = brownie::net::fetch(&url, b.max_redirects).map_err(|e| e.to_string())?;
        print!("{}", resp.text());
        eprintln!("-- HTTP {} from {} (blocked so far: {})", resp.status, resp.final_url, b.blocked_count);
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--dump-dom") {
        let url = args.get(i + 1).ok_or("--dump-dom needs a URL")?;
        let mut b = make_browser(args)?;
        let page = b.open(url).map_err(|e| e.to_string())?;
        let mut out = String::new();
        dump_dom(&page.doc, page.doc.root, 0, &mut out);
        print!("{}", out);
        eprintln!("blocked: {}", b.blocked_count);
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--dump-layout") {
        let url = args.get(i + 1).ok_or("--dump-layout needs a URL")?;
        let width: f32 = arg_value(args, "--width", "80").parse().map_err(|_| "bad --width")?;
        let mut b = make_browser(args)?;
        let page = b.open(url).map_err(|e| e.to_string())?;
        let root = b.layout_page(&page, width);
        let mut out = String::new();
        layout::dump(&root, 0, &mut out);
        print!("{}", out);
        eprintln!("blocked: {}", b.blocked_count);
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--bench") {
        let url = args.get(i + 1).ok_or("--bench needs a URL")?;
        let iterations: usize = arg_value(args, "--iterations", "20").parse().map_err(|_| "bad --iterations")?;
        let width: usize = arg_value(args, "--width", "320").parse().map_err(|_| "bad --width")?;
        let height: usize = arg_value(args, "--height", "400").parse().map_err(|_| "bad --height")?;
        let mut b = make_browser(args)?;
        let r = b.bench(url, width, height, iterations).map_err(|e| e.to_string())?;
        println!("{}", r);
        eprintln!("blocked: {}", b.blocked_count);
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--frame-shot") {
        let out = args.get(i + 1).ok_or("--frame-shot needs an output .png path")?;
        let width: usize = arg_value(args, "--width", "480").parse().map_err(|_| "bad --width")?;
        let height: usize = arg_value(args, "--height", "340").parse().map_err(|_| "bad --height")?;
        let scale: u32 = arg_value(args, "--scale", "2").parse().map_err(|_| "bad --scale")?;
        frame_shot(out, width, height, scale)?;
        return Ok(());
    }

    if let Some(i) = args.iter().position(|a| a == "--screenshot") {
        let url = args.get(i + 1).ok_or("--screenshot needs a URL")?;
        let out = args.get(i + 2).ok_or("--screenshot needs an output .png path")?;
        let width: usize = arg_value(args, "--width", "320").parse().map_err(|_| "bad --width")?;
        let height: usize = arg_value(args, "--height", "400").parse().map_err(|_| "bad --height")?;
        let scale: u32 = arg_value(args, "--scale", "2").parse().map_err(|_| "bad --scale")?;
        let mut b = make_browser(args)?;
        let page = b.screenshot(url, out, width, height, scale).map_err(|e| e.to_string())?;
        println!("saved {} ({}x{} @{}x) - title: {:?} - blocked: {}", out, width, height, scale, page.title, b.blocked_count);
        return Ok(());
    }

    // interactive mode
    let url = args.iter().find(|a| !a.starts_with('-')).cloned();
    if !std::io::stdin().is_terminal() {
        eprintln!("stdin is not a terminal; use --screenshot/--dump-dom/--dump-layout/--fetch (see --help)");
        return Ok(());
    }
    interactive(url)
}

fn dump_dom(doc: &brownie::Document, idx: usize, depth: usize, out: &mut String) {
    match doc.nodes.get(idx) {
        Some(brownie::html::Node::Element(e)) => {
            let attrs = e
                .attrs
                .iter()
                .map(|(k, v)| format!(" {}=\"{}\"", k, v))
                .collect::<String>();
            out.push_str(&format!("{:indent$}<{}{}>\n", "", e.tag, attrs, indent = depth * 2));
            for &c in doc.children(idx) {
                dump_dom(doc, c, depth + 1, out);
            }
        }
        Some(brownie::html::Node::Text(t)) => {
            let t = t.trim();
            if !t.is_empty() {
                out.push_str(&format!("{:indent$}\"{}\"\n", "", t, indent = depth * 2));
            }
        }
        None => {}
    }
}

// ---------------------------------------------------------------- interactive

struct TermGuard {
    saved: String,
}

impl TermGuard {
    fn enter() -> Option<TermGuard> {
        let out = Command::new("stty").arg("-g").stdin(Stdio::inherit()).output().ok()?;
        let saved = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let _ = Command::new("stty").args(["raw", "-echo"]).stdin(Stdio::inherit()).status();
        Some(TermGuard { saved })
    }
}

impl Drop for TermGuard {
    fn drop(&mut self) {
        let _ = Command::new("stty").arg(&self.saved).stdin(Stdio::inherit()).status();
        print!("\x1b[?25h\x1b[?1049l");
        let _ = std::io::stdout().flush();
    }
}

fn terminal_size() -> (usize, usize) {
    if let Ok(out) = Command::new("stty").arg("size").stdin(Stdio::inherit()).output() {
        let s = String::from_utf8_lossy(&out.stdout);
        let mut it = s.split_whitespace();
        if let (Some(r), Some(c)) = (it.next(), it.next()) {
            if let (Ok(r), Ok(c)) = (r.parse::<usize>(), c.parse::<usize>()) {
                return (r.max(10), c.max(20));
            }
        }
    }
    (24, 80)
}

enum Key {
    Char(char),
    Enter,
    Esc,
    Backspace,
    Up,
    Down,
    PgUp,
    PgDn,
    Tab,
    ShiftTab,
    Home,
    End,
    CtrlC,
    Other,
}

fn read_key(stdin: &mut std::io::StdinLock) -> Key {
    let mut buf = [0u8; 8];
    let n = stdin.read(&mut buf).unwrap_or(0);
    if n == 0 {
        return Key::Other;
    }
    match buf[0] {
        3 => Key::CtrlC,
        b'\r' | b'\n' => Key::Enter,
        0x1b => {
            if n >= 3 && buf[1] == b'[' {
                match buf[2] {
                    b'A' => return Key::Up,
                    b'B' => return Key::Down,
                    b'C' => return Key::Other,
                    b'D' => return Key::Other,
                    b'Z' => return Key::ShiftTab,
                    b'5' if n >= 4 && buf[3] == b'~' => return Key::PgUp,
                    b'6' if n >= 4 && buf[3] == b'~' => return Key::PgDn,
                    b'H' => return Key::Home,
                    b'F' => return Key::End,
                    _ => {}
                }
                Key::Esc
            } else if n == 1 {
                Key::Esc
            } else {
                Key::Esc
            }
        }
        0x7f | 0x08 => Key::Backspace,
        b'\t' => Key::Tab,
        c if c >= 0x20 => {
            // decode UTF-8 char
            let s = &buf[..n];
            match std::str::from_utf8(s) {
                Ok(st) => st.chars().next().map(Key::Char).unwrap_or(Key::Other),
                Err(_) => Key::Other,
            }
        }
        _ => Key::Other,
    }
}

fn interactive(start_url: Option<String>) -> Result<(), String> {
    let mut browser = make_browser(&std::env::args().skip(1).collect::<Vec<_>>())?;
    let _guard = TermGuard::enter().ok_or("cannot set terminal raw mode")?;
    print!("\x1b[?1049h\x1b[?25l\x1b[2J");
    let _ = std::io::stdout().flush();

    let mut tabs: Vec<Tab> = Vec::new();
    let mut active = 0usize;
    let mut bar_input = String::new();
    let mut bar_mode = false;

    if let Some(u) = start_url {
        bar_input = u;
        bar_mode = true;
    }

    let mut shot_counter = 0usize;

    loop {
        let (rows, cols) = terminal_size();
        if tabs.is_empty() {
            tabs.push(Tab {
                url: String::new(),
                page: None,
                scroll: 0.0,
                status: "KucLab Brow ready - press / to enter a URL".to_string(),
            });
        }
        // auto-load the paper start page on empty tabs
        if tabs[active].page.is_none() && tabs[active].url.is_empty() {
            match browser.open_source("brow://start", START_PAGE) {
                Ok(p) => {
                    tabs[active].url = "brow://start".to_string();
                    tabs[active].page = Some(p);
                    tabs[active].status = "start page".to_string();
                }
                Err(e) => tabs[active].status = format!("start page error: {}", e),
            }
        }

        // render - paper chrome: header / hairline-or-input / content / hairline / footer
        let content_rows = rows.saturating_sub(4).max(1);
        let fb_w = cols;
        let fb_h = content_rows * 2;
        let mut screen = String::new();
        screen.push_str("\x1b[H");

        // header row on paper
        let mut segs: Vec<(String, brownie::css::Color)> =
            vec![("KucLab Brow".to_string(), theme::INK_STRONG)];
        for (i, t) in tabs.iter().enumerate() {
            let c = if i == active { theme::ACCENT } else { theme::MUTED };
            segs.push((format!("  {} {}", i + 1, short(&t.url, 18)), c));
        }
        let right = format!("ads blocked: {}", browser.blocked_count);
        let used: usize =
            segs.iter().map(|(s, _)| s.chars().count()).sum::<usize>() + right.chars().count();
        let pad = cols.saturating_sub(used);
        screen.push_str(&esc_bg(theme::PAPER));
        for (s, c) in &segs {
            screen.push_str(&esc_fg(*c));
            screen.push_str(s);
        }
        screen.push_str(&esc_fg(theme::MUTED));
        for _ in 0..pad {
            screen.push(' ');
        }
        screen.push_str(&esc_fg(theme::MUTED_2));
        screen.push_str(&right);
        screen.push_str("\x1b[0m\n");

        // hairline row, or the URL input on a surface when editing
        if bar_mode {
            let prompt = " open: ";
            screen.push_str(&esc_bg(theme::SURFACE));
            screen.push_str(&esc_fg(theme::LINE));
            screen.push('│');
            screen.push_str(&esc_fg(theme::INK));
            screen.push_str(prompt);
            screen.push_str(&bar_input);
            screen.push_str(&esc_fg(theme::ACCENT));
            screen.push('█');
            let used = 1 + prompt.chars().count() + bar_input.chars().count() + 1;
            for _ in 0..cols.saturating_sub(used + 1) {
                screen.push(' ');
            }
            screen.push_str(&esc_fg(theme::LINE));
            screen.push('│');
            screen.push_str("\x1b[0m\n");
        } else {
            screen.push_str(&esc_bg(theme::PAPER));
            screen.push_str(&esc_fg(theme::LINE));
            for _ in 0..cols {
                screen.push('─');
            }
            screen.push_str("\x1b[0m\n");
        }

        // content
        let rendered = match tabs[active]
            .page
            .as_ref()
            .map(|page| browser.render_page(page, fb_w, fb_h, tabs[active].scroll, 1))
        {
            Some(Ok(v)) => Some(v),
            Some(Err(e)) => {
                tabs[active].status = format!("render error: {}", e);
                None
            }
            None => None,
        };
        if let Some((fb, root)) = rendered {
            let max_scroll = (root.h - fb_h as f32).max(0.0);
            tabs[active].scroll = tabs[active].scroll.clamp(0.0, max_scroll);
            blit(&fb, &mut screen);
            let pct = if max_scroll > 0.0 { (tabs[active].scroll / max_scroll * 100.0) as u32 } else { 100 };
            let page = tabs[active].page.as_ref().unwrap();
            tabs[active].status = format!(
                "{} | HTTP {} | \"{}\" | blocked: {} | {}%",
                short(&page.final_url, 40),
                page.status,
                page.title,
                browser.blocked_count,
                pct
            );
        } else {
            screen.push_str(&esc_bg(theme::PAPER));
            screen.push_str(&esc_fg(theme::MUTED));
            let msg = "KucLab Brow - engine: Brownie. Press / and type a URL, Enter to load.";
            for r in 0..content_rows {
                if r == 2 {
                    screen.push_str(msg);
                } else {
                    for _ in 0..cols {
                        screen.push(' ');
                    }
                }
                screen.push_str("\x1b[0m\n");
            }
        }

        // footer hairline + footer row
        screen.push_str(&esc_bg(theme::PAPER));
        screen.push_str(&esc_fg(theme::LINE));
        for _ in 0..cols {
            screen.push('─');
        }
        screen.push_str("\x1b[0m\n");
        screen.push_str(&esc_bg(theme::PAPER));
        screen.push_str(&esc_fg(theme::MUTED_2));
        let mut st = format!(
            " {}  [/] open  [j/k] scroll  [r] reload  [t] tab  [Tab] switch  [a] adblock {}  [s] shot  [q] quit",
            tabs[active].status,
            if browser.adblock.is_empty() { "off" } else { "on" }
        );
        while st.chars().count() < cols {
            st.push(' ');
        }
        screen.push_str(&st);
        screen.push_str("\x1b[0m");

        let mut out = std::io::stdout().lock();
        out.write_all(screen.as_bytes()).map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
        drop(out);

        // input
        let mut stdin = std::io::stdin().lock();
        let key = read_key(&mut stdin);
        drop(stdin);

        if bar_mode {
            match key {
                Key::Esc => bar_mode = false,
                Key::Enter => {
                    let url = brownie::normalize_url(&bar_input);
                    bar_mode = false;
                    let tab = &mut tabs[active];
                    tab.status = format!("loading {} ...", url);
                    match browser.open(&url) {
                        Ok(page) => {
                            tab.url = page.final_url.clone();
                            tab.page = Some(page);
                            tab.scroll = 0.0;
                        }
                        Err(e) => {
                            tab.status = format!("error: {}", e);
                        }
                    }
                }
                Key::Backspace => {
                    bar_input.pop();
                }
                Key::Char(c) => bar_input.push(c),
                Key::CtrlC => return Ok(()),
                _ => {}
            }
        } else {
            match key {
                Key::Char('q') | Key::CtrlC => return Ok(()),
                Key::Char('/') | Key::Char(':') => {
                    bar_mode = true;
                    bar_input = tabs[active].url.clone();
                }
                Key::Char('j') | Key::Down => tabs[active].scroll += 10.0,
                Key::Char('k') | Key::Up => tabs[active].scroll -= 10.0,
                Key::PgDn => tabs[active].scroll += fb_h as f32,
                Key::PgUp => tabs[active].scroll -= fb_h as f32,
                Key::Home => tabs[active].scroll = 0.0,
                Key::End => tabs[active].scroll = f32::MAX,
                Key::Char('r') => {
                    let url = tabs[active].url.clone();
                    if !url.is_empty() {
                        if let Ok(page) = browser.open(&url) {
                            tabs[active].page = Some(page);
                            tabs[active].scroll = 0.0;
                        }
                    }
                }
                Key::Char('t') => {
                    tabs.push(Tab {
                        url: String::new(),
                        page: None,
                        scroll: 0.0,
                        status: String::new(),
                    });
                    active = tabs.len() - 1;
                    bar_input.clear();
                    bar_mode = true;
                }
                Key::Tab => {
                    active = (active + 1) % tabs.len();
                }
                Key::ShiftTab => {
                    active = if active == 0 { tabs.len() - 1 } else { active - 1 };
                }
                Key::Char('a') => {
                    if browser.adblock.is_empty() {
                        if let Ok(ab) = AdBlock::load_from_file(DEFAULT_FILTERS) {
                            browser.adblock = ab;
                            tabs[active].status = "adblock ON".to_string();
                        }
                    } else {
                        browser.adblock = AdBlock::load_from_str("");
                        tabs[active].status = "adblock OFF".to_string();
                    }
                }
                Key::Char('s') => {
                    if let Some(page) = &tabs[active].page {
                        shot_counter += 1;
                        let path = format!("screenshot-{}.png", shot_counter);
                        match browser.render_page(page, 320, 400, tabs[active].scroll, 2) {
                            Ok((fb, _)) => match brownie::png::write_png(&path, &fb) {
                                Ok(()) => tabs[active].status = format!("saved {}", path),
                                Err(e) => tabs[active].status = format!("screenshot failed: {}", e),
                            },
                            Err(e) => tabs[active].status = format!("render failed: {}", e),
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

fn esc_fg(c: brownie::css::Color) -> String {
    format!("\x1b[38;2;{};{};{}m", c.r, c.g, c.b)
}

fn esc_bg(c: brownie::css::Color) -> String {
    format!("\x1b[48;2;{};{};{}m", c.r, c.g, c.b)
}

/// Render one frame of the interactive UI (paper chrome + start page) to a PNG.
/// Used to produce design samples without a terminal.
fn frame_shot(out_path: &str, width: usize, height: usize, scale: u32) -> Result<(), String> {
    use brownie::theme::*;
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut browser = make_browser(&args)?;
    let page = browser.open_source("brow://start", START_PAGE).map_err(|e| e.to_string())?;
    let ds = scale.max(1);
    let dsf = ds as f32;
    let mut fb = brownie::Framebuffer::new(width * ds as usize, height * ds as usize);
    fb.clear(PAPER);
    // header
    fb.fill_rect(0.0, 0.0, width as f32 * dsf, 40.0 * dsf, PAPER);
    fb.draw_text(18.0 * dsf, 10.0 * dsf, "KucLab Brow", 30.0 * dsf, INK_STRONG);
    fb.draw_text(190.0 * dsf, 16.0 * dsf, "1  brow://start", 13.0 * dsf, ACCENT);
    let right = format!("ads blocked: {}", browser.blocked_count);
    fb.draw_text((width as f32 - 120.0) * dsf, 17.0 * dsf, &right, 11.0 * dsf, MUTED_2);
    // hairline under header
    fb.fill_rect(0.0, 40.0 * dsf, width as f32 * dsf, dsf, LINE);
    // content: the start page rendered by Brownie
    let content_h = height.saturating_sub(64);
    let (page_fb, _root) = browser
        .render_page(&page, width, content_h, 0.0, 1)
        .map_err(|e| e.to_string())?;
    for y in 0..page_fb.height {
        for x in 0..page_fb.width {
            let i = (y * page_fb.width + x) * 4;
            let c = brownie::css::Color { r: page_fb.data[i], g: page_fb.data[i + 1], b: page_fb.data[i + 2], a: 255 };
            for dy in 0..ds as i32 {
                for dx in 0..ds as i32 {
                    fb.set_pixel(
                        (x as u32 * ds + dx as u32) as i32,
                        ((41 + y) as u32 * ds + dy as u32) as i32,
                        c,
                    );
                }
            }
        }
    }
    // footer hairline + footer
    let fy = (height - 24) as f32 * dsf;
    fb.fill_rect(0.0, fy, width as f32 * dsf, dsf, LINE);
    fb.fill_rect(0.0, fy + dsf, width as f32 * dsf, 23.0 * dsf, PAPER);
    fb.draw_text(
        18.0 * dsf,
        (height as f32 - 17.0) * dsf,
        "brow://start - KucLab Brow - start - ads blocked: 0 - [/] open  [j/k] scroll  [q] quit",
        11.0 * dsf,
        MUTED_2,
    );
    brownie::png::write_png(out_path, &fb).map_err(|e| e.to_string())?;
    println!("saved {} ({}x{} @{}x)", out_path, width * ds as usize, height * ds as usize, ds);
    Ok(())
}

fn blit(fb: &brownie::Framebuffer, out: &mut String) {
    use std::fmt::Write as _;
    for r in 0..fb.height / 2 {
        for c in 0..fb.width {
            let ti = ((2 * r) * fb.width + c) * 4;
            let bi = ((2 * r + 1) * fb.width + c) * 4;
            if ti + 3 >= fb.data.len() || bi + 3 >= fb.data.len() {
                out.push(' ');
                continue;
            }
            let (tr, tg, tb) = (fb.data[ti], fb.data[ti + 1], fb.data[ti + 2]);
            let (br, bg, bb) = (fb.data[bi], fb.data[bi + 1], fb.data[bi + 2]);
            let _ = write!(out, "\x1b[38;2;{};{};{}m\x1b[48;2;{};{};{}m\u{2580}", tr, tg, tb, br, bg, bb);
        }
        out.push_str("\x1b[0m\n");
    }
}

fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max - 3).collect::<String>())
    }
}

#[allow(dead_code)]
fn kind_name(k: BoxKind) -> &'static str {
    match k {
        BoxKind::Block => "BLOCK",
        BoxKind::Anon => "ANON",
        BoxKind::Inline => "INLINE",
        BoxKind::Text => "TEXT",
        BoxKind::Br => "BR",
        BoxKind::Rect => "RECT",
    }
}

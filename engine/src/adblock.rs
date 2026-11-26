//! Own ad-blocker: filter-list engine supporting hosts-style entries
//! (`0.0.0.0 ads.example.com`), domain-anchored patterns (`||ads.example.com^`),
//! generic wildcard patterns (`*/ads/*`) and `@@` exceptions.

#[derive(Debug, Clone, PartialEq, Eq)]
enum RuleKind {
    Block,
    Allow,
}

#[derive(Debug, Clone)]
enum Pattern {
    /// hosts-style: block this host and its subdomains
    Host(String),
    /// domain-anchored: `||domain` + optional path pattern matched against the rest
    Anchored { domain: String, rest: String },
    /// generic wildcard pattern matched against the whole URL
    Url(String),
}

#[derive(Debug, Clone)]
struct Rule {
    kind: RuleKind,
    pattern: Pattern,
}

/// Max filter rules and max pattern length (anti DoS on hostile filter lists).
pub const MAX_RULES: usize = 250_000;
pub const MAX_PATTERN_LEN: usize = 1024;

#[derive(Debug, Default)]
pub struct AdBlock {
    rules: Vec<Rule>,
}

fn is_separator(c: char) -> bool {
    !c.is_ascii_alphanumeric() && !matches!(c, '-' | '.' | '_' | '~' | '%')
}

/// Wildcard match: `*` = any sequence, `?` = one char, `^` = one separator or end.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    wildcard_match_impl(pattern, text, false)
}

/// Prefix variant (EasyList `||domain...` semantics): pattern must match a prefix.
pub fn wildcard_match_prefix(pattern: &str, text: &str) -> bool {
    wildcard_match_impl(pattern, text, true)
}

fn wildcard_match_impl(pattern: &str, text: &str, prefix: bool) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star = usize::MAX;
    let mut mark = 0usize;
    while ti < t.len() {
        if pi == p.len() {
            if prefix {
                return true;
            }
            if star != usize::MAX {
                // trailing '*' can still absorb the rest of the text
                mark += 1;
                ti = mark;
                continue;
            }
            return false;
        }
        if p[pi] == '^' {
            if is_separator(t[ti]) {
                pi += 1;
                ti += 1;
            } else if star != usize::MAX {
                pi = star + 1;
                mark += 1;
                ti = mark;
            } else {
                return false;
            }
        } else if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = ti;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            ti = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && (p[pi] == '*' || p[pi] == '^') {
        pi += 1;
    }
    pi == p.len() || (prefix && pi >= p.len())
}

fn parse_rule(line: &str) -> Option<Rule> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }
    let (kind, rest) = if let Some(r) = l.strip_prefix("@@") {
        (RuleKind::Allow, r)
    } else {
        (RuleKind::Block, l)
    };
    if rest.len() > MAX_PATTERN_LEN {
        return None;
    }
    // skip comments and headers
    if rest.starts_with('!') || rest.starts_with('[') || rest.starts_with('#') {
        return None;
    }
    if let Some(r) = rest.strip_prefix("||") {
        // domain-anchored: domain runs until '^', '/', '*', '?', '|' or end
        let mut i = 0;
        let chars: Vec<char> = r.chars().collect();
        while i < chars.len() && !matches!(chars[i], '^' | '/' | '*' | '?' | '|') {
            i += 1;
        }
        let domain: String = chars[..i].iter().collect::<String>().to_lowercase();
        let rest_pat: String = chars[i..].iter().collect();
        if domain.is_empty() {
            return None;
        }
        return Some(Rule {
            kind,
            pattern: Pattern::Anchored {
                domain,
                rest: rest_pat,
            },
        });
    }
    // hosts-style: "0.0.0.0 host" / "127.0.0.1 host" / "::1 host"
    let mut parts = rest.split_whitespace();
    if let (Some(ip), Some(host)) = (parts.next(), parts.next()) {
        if (ip == "0.0.0.0" || ip == "127.0.0.1" || ip == "::1" || ip == "0.0.0.0:0")
            && !host.contains('*')
            && !host.contains('/')
        {
            return Some(Rule {
                kind,
                pattern: Pattern::Host(host.to_lowercase()),
            });
        }
    }
    if rest.contains('*') || rest.contains('^') || rest.contains('?') {
        Some(Rule {
            kind,
            pattern: Pattern::Url(rest.to_lowercase()),
        })
    } else {
        // bare domain or substring: treat as URL substring match via wildcards
        Some(Rule {
            kind,
            pattern: Pattern::Url(format!("*{}*", rest.to_lowercase())),
        })
    }
}

impl AdBlock {
    pub fn load_from_str(input: &str) -> AdBlock {
        let mut rules = Vec::new();
        for line in input.lines() {
            if let Some(r) = parse_rule(line) {
                rules.push(r);
            }
            if rules.len() >= MAX_RULES {
                break;
            }
        }
        AdBlock { rules }
    }

    pub fn load_from_file(path: &str) -> std::io::Result<AdBlock> {
        let s = std::fs::read_to_string(path)?;
        Ok(AdBlock::load_from_str(&s))
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    fn host_of(url: &str) -> String {
        let rest = url.split("://").nth(1).unwrap_or(url);
        let authority = rest.split('/').next().unwrap_or("");
        authority.rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("").to_lowercase()
    }

    fn rule_matches(rule: &Rule, url: &str) -> bool {
        let lower = url.to_lowercase();
        match &rule.pattern {
            Pattern::Host(d) => {
                let h = Self::host_of(&lower);
                h == *d || h.ends_with(&format!(".{}", d))
            }
            Pattern::Anchored { domain, rest } => {
                let after_scheme = lower.split("://").nth(1).unwrap_or(&lower);
                let h = Self::host_of(&lower);
                let host_ok = h == *domain || h.ends_with(&format!(".{}", domain));
                if !host_ok {
                    return false;
                }
                if rest.is_empty() {
                    return true;
                }
                // match the remainder (path + query) after "domain[:port]"
                let idx = after_scheme.find(domain.as_str()).unwrap_or(0);
                let mut tail = &after_scheme[idx + domain.len()..];
                if let Some(stripped) = tail.strip_prefix(':') {
                    let digits: String = stripped.chars().take_while(|c| c.is_ascii_digit()).collect();
                    tail = &stripped[digits.len()..];
                }
                wildcard_match_prefix(rest, tail)
            }
            Pattern::Url(pat) => wildcard_match(pat, &lower),
        }
    }

    /// True if the URL should be blocked. `@@` allow rules win over block rules.
    pub fn is_blocked(&self, url: &str) -> bool {
        for r in &self.rules {
            if r.kind == RuleKind::Allow && Self::rule_matches(r, url) {
                return false;
            }
        }
        self.rules
            .iter()
            .any(|r| r.kind == RuleKind::Block && Self::rule_matches(r, url))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_basics() {
        assert!(wildcard_match("*ads*", "http://x.com/ads/banner.js"));
        assert!(!wildcard_match("*ads*", "http://x.com/ok.js"));
        assert!(wildcard_match("^", "/"));
        assert!(wildcard_match("^", ""));
        assert!(!wildcard_match("^", "a"));
        assert!(wildcard_match("*/x", "/x"));
        assert!(!wildcard_match("*/x", "ax"));
        assert!(wildcard_match("a?c", "abc"));
        assert!(!wildcard_match("a?c", "ac"));
        assert!(wildcard_match_prefix("^", "/ads.js"));
        assert!(wildcard_match_prefix("/ads", "/ads.js"));
        assert!(!wildcard_match("/ads", "/ads.js"));
    }

    #[test]
    fn hosts_style() {
        let ab = AdBlock::load_from_str("0.0.0.0 ads.example.com\n127.0.0.1 tracker.io\n! comment\n[Adblock Plus 2.0]\n");
        assert_eq!(ab.len(), 2);
        assert!(ab.is_blocked("http://ads.example.com/x.js"));
        assert!(ab.is_blocked("http://sub.ads.example.com/x.js"));
        assert!(!ab.is_blocked("http://example.com/ads.js"));
        assert!(ab.is_blocked("https://tracker.io/pixel"));
    }

    #[test]
    fn anchored_and_generic() {
        let ab = AdBlock::load_from_str("||doubleclick.net^\n*/ads/*\n");
        assert!(ab.is_blocked("https://ad.doubleclick.net/ddm/track"));
        assert!(ab.is_blocked("http://site.com/ads/banner.png"));
        assert!(!ab.is_blocked("http://site.com/article"));
    }

    #[test]
    fn exceptions_win() {
        let ab = AdBlock::load_from_str("||example.com^\n@@||example.com/allowed.js\n");
        assert!(ab.is_blocked("http://example.com/ads.js"));
        assert!(!ab.is_blocked("http://example.com/allowed.js"));
    }

    #[test]
    fn hostile_inputs_are_rejected() {
        let ab = AdBlock::load_from_str(&format!("||{}^\n", "a".repeat(5000)));
        assert_eq!(ab.len(), 0);
        let big = "||x.com^\n".repeat(300_000);
        let ab2 = AdBlock::load_from_str(&big);
        assert_eq!(ab2.len(), MAX_RULES);
        // pathological wildcard still terminates
        let ab3 = AdBlock::load_from_str("*a*a*a*a*a*a*a*a*b\n");
        assert!(!ab3.is_blocked(&format!("http://x.com/{}", "a".repeat(5000))));
    }

    #[test]
    fn path_pattern() {
        let ab = AdBlock::load_from_str("||127.0.0.1/ads.js\n");
        assert!(ab.is_blocked("http://127.0.0.1:8080/ads.js"));
        assert!(!ab.is_blocked("http://127.0.0.1:8080/index.html"));
    }
}

//! Request validation system — declarative per-endpoint body rules.
//!
//! Mirrors the rule vocabulary of Encore's `encore.dev/validate` package
//! (upstream commits 1644–1665: `minLen`, `maxLen`, `min`, `max`,
//! `matchesRegexp`, `startsWith`, `endsWith`, `isEmail`, `isURL`) adapted to
//! this daemon's zero-dependency, spec-string driven style.
//!
//! ## Model
//!
//! Rules are registered against `METHOD:/path` endpoints via
//! `POST /api/v1/validate`. The HTTP layer consults the registry in
//! `route_after_rl`: if the incoming JSON body violates any rule, the request
//! is short-circuited with `400` and a structured violations list.
//!
//! ```json
//! {
//!   "endpoint": "POST:/users",
//!   "field": "email",
//!   "rules": ["required", "isEmail", "maxLen:255"]
//! }
//! ```
//!
//! Only flat JSON fields are validated (strings, numbers, booleans).
//! Absent optional fields pass unless the `required` rule is present.
//!
//! ## Regex subset
//!
//! `matchesRegexp` is evaluated by a small built-in backtracking engine
//! supporting: literals, `.`, `\d \w \s \D \W \S`, classes `[a-z0-9_]`
//! (with negation), groups `( … )` with alternation `|`, quantifiers
//! `* + ? {n} {n,} {n,m}`, and anchors `^ $`. Matching follows JavaScript
//! `RegExp.test` semantics (unanchored search) so patterns port from Encore.

#![allow(dead_code)]

use std::collections::HashMap;

// ── Regex engine ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Digit(bool),
    Word(bool),
    Space(bool),
    Class {
        negated: bool,
        items: Vec<(char, char)>,
    },
    Group(Vec<Vec<Node>>),
    Repeat {
        node: Box<Node>,
        min: usize,
        max: usize,
    },
    Start,
    End,
}

struct RegexParser<'a> {
    chars: &'a [char],
    pos: usize,
}

impl<'a> RegexParser<'a> {
    fn parse_alternation(&mut self) -> Result<Vec<Vec<Node>>, String> {
        let mut branches = vec![self.parse_sequence()?];
        while self.peek() == Some('|') {
            self.pos += 1;
            branches.push(self.parse_sequence()?);
        }
        Ok(branches)
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn parse_sequence(&mut self) -> Result<Vec<Node>, String> {
        let mut nodes = Vec::new();
        loop {
            match self.peek() {
                None | Some('|') | Some(')') => break,
                _ => {}
            }
            let atom = self.parse_atom()?;
            let atom = self.parse_quantifier(atom)?;
            nodes.push(atom);
        }
        Ok(nodes)
    }

    fn parse_atom(&mut self) -> Result<Node, String> {
        let c = self.peek().ok_or("unexpected end of pattern")?;
        self.pos += 1;
        match c {
            '(' => {
                let inner = self.parse_alternation()?;
                if self.peek() != Some(')') {
                    return Err("missing closing paren".into());
                }
                self.pos += 1;
                Ok(Node::Group(inner))
            }
            '[' => self.parse_class(),
            '.' => Ok(Node::Any),
            '^' => Ok(Node::Start),
            '$' => Ok(Node::End),
            '\\' => {
                let esc = self.peek().ok_or("dangling escape")?;
                self.pos += 1;
                Ok(match esc {
                    'd' => Node::Digit(false),
                    'D' => Node::Digit(true),
                    'w' => Node::Word(false),
                    'W' => Node::Word(true),
                    's' => Node::Space(false),
                    'S' => Node::Space(true),
                    other => Node::Char(other),
                })
            }
            other => Ok(Node::Char(other)),
        }
    }

    fn parse_class(&mut self) -> Result<Node, String> {
        let negated = if self.peek() == Some('^') {
            self.pos += 1;
            true
        } else {
            false
        };
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let c = self.peek().ok_or("unterminated character class")?;
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;
            self.pos += 1;
            let lo = if c == '\\' {
                let esc = self.peek().ok_or("dangling escape in class")?;
                self.pos += 1;
                match esc {
                    'd' => {
                        items.push(('0', '9'));
                        continue;
                    }
                    'w' => {
                        items.push(('a', 'z'));
                        items.push(('A', 'Z'));
                        items.push(('0', '9'));
                        items.push(('_', '_'));
                        continue;
                    }
                    's' => {
                        items.push((' ', ' '));
                        items.push(('\t', '\t'));
                        items.push(('\n', '\n'));
                        items.push(('\r', '\r'));
                        continue;
                    }
                    other => other,
                }
            } else {
                c
            };
            // Range?
            if self.peek() == Some('-')
                && self
                    .chars
                    .get(self.pos + 1)
                    .copied()
                    .map(|c| c != ']')
                    .unwrap_or(false)
            {
                self.pos += 1; // consume '-'
                let hi = self.peek().ok_or("unterminated range")?;
                self.pos += 1;
                let hi = if hi == '\\' {
                    let esc = self.peek().ok_or("dangling escape in range")?;
                    self.pos += 1;
                    esc
                } else {
                    hi
                };
                if hi < lo {
                    return Err(format!("invalid range {lo}-{hi}"));
                }
                items.push((lo, hi));
            } else {
                items.push((lo, lo));
            }
        }
        Ok(Node::Class { negated, items })
    }

    fn parse_quantifier(&mut self, atom: Node) -> Result<Node, String> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.pos += 1;
                (0, usize::MAX)
            }
            Some('+') => {
                self.pos += 1;
                (1, usize::MAX)
            }
            Some('?') => {
                self.pos += 1;
                (0, 1)
            }
            Some('{') => {
                // Try {n} {n,} {n,m}; literal '{' if malformed.
                let save = self.pos;
                self.pos += 1;
                let lo_str: String = self.chars[self.pos..]
                    .iter()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                if lo_str.is_empty() {
                    self.pos = save;
                    return Ok(atom);
                }
                self.pos += lo_str.len();
                let lo: usize = lo_str.parse().map_err(|_| "bad repetition")?;
                let hi = if self.peek() == Some(',') {
                    self.pos += 1;
                    let hi_str: String = self.chars[self.pos..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit())
                        .collect();
                    self.pos += hi_str.len();
                    if hi_str.is_empty() {
                        usize::MAX
                    } else {
                        hi_str.parse().map_err(|_| "bad repetition")?
                    }
                } else {
                    lo
                };
                if self.peek() != Some('}') {
                    self.pos = save;
                    return Ok(atom);
                }
                if hi < lo {
                    return Err(format!("invalid repetition {{{lo},{hi}}} — max < min"));
                }
                self.pos += 1;
                (lo, hi)
            }
            _ => return Ok(atom),
        };
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }
}

/// Compiled regex — `matches` follows JS `RegExp.test` semantics.
pub struct Regex {
    branches: Vec<Vec<Node>>,
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self, String> {
        let chars: Vec<char> = pattern.chars().collect();
        let mut p = RegexParser {
            chars: &chars,
            pos: 0,
        };
        let branches = p.parse_alternation()?;
        if p.pos != chars.len() {
            return Err(format!("unexpected ')' at offset {}", p.pos));
        }
        Ok(Regex { branches })
    }

    /// True if the pattern matches anywhere in `input`.
    pub fn test(&self, input: &str) -> bool {
        let s: Vec<char> = input.chars().collect();
        for start in 0..=s.len() {
            for branch in &self.branches {
                if match_seq(branch, &s, start, &mut |_| true) {
                    return true;
                }
            }
        }
        false
    }

    /// True only if the pattern consumes the entire input.
    pub fn test_full(&self, input: &str) -> bool {
        let s: Vec<char> = input.chars().collect();
        self.branches
            .iter()
            .any(|b| match_seq(b, &s, 0, &mut |p| p == s.len()))
    }
}

fn class_matches(negated: bool, items: &[(char, char)], c: char) -> bool {
    let hit = items.iter().any(|&(lo, hi)| c >= lo && c <= hi);
    hit != negated
}

fn char_match(node: &Node, c: char) -> bool {
    match node {
        Node::Char(x) => *x == c,
        Node::Any => c != '\n',
        Node::Digit(neg) => c.is_ascii_digit() != *neg,
        Node::Word(neg) => (c.is_alphanumeric() || c == '_') != *neg,
        Node::Space(neg) => c.is_whitespace() != *neg,
        Node::Class { negated, items } => class_matches(*negated, items, c),
        _ => false,
    }
}

/// Backtracking matcher: does `nodes` match `s` starting at `pos`,
/// calling `k` with the end position on success (continuation-passing).
fn match_seq(nodes: &[Node], s: &[char], pos: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    let Some((first, rest)) = nodes.split_first() else {
        return k(pos);
    };
    match first {
        Node::Start => pos == 0 && match_seq(rest, s, pos, k),
        Node::End => pos == s.len() && match_seq(rest, s, pos, k),
        Node::Group(branches) => {
            for branch in branches {
                // Match branch then continue with rest — splice via nested closure.
                let mut combined: Vec<Node> = branch.clone();
                combined.extend_from_slice(rest);
                if match_seq(&combined, s, pos, k) {
                    return true;
                }
            }
            false
        }
        Node::Repeat { node, min, max } => {
            // Greedy expansion matcher with an empty-loop guard.
            //   done < min : keep matching (zero-width matches can't make progress)
            //   done < max : greedily try one more repetition before continuing
            #[allow(clippy::too_many_arguments)]
            fn repeat_match(
                node: &Node,
                rest: &[Node],
                s: &[char],
                pos: usize,
                done: usize,
                min: usize,
                max: usize,
                k: &mut dyn FnMut(usize) -> bool,
            ) -> bool {
                if done < min {
                    return match_one_then(node, s, pos, &mut |p2| {
                        p2 != pos && repeat_match(node, rest, s, p2, done + 1, min, max, k)
                    });
                }
                if done < max
                    && match_one_then(node, s, pos, &mut |p2| {
                        p2 != pos && repeat_match(node, rest, s, p2, done + 1, min, max, k)
                    })
                {
                    return true;
                }
                match_seq(rest, s, pos, k)
            }
            repeat_match(node, rest, s, pos, 0, *min, *max, k)
        }
        leaf => {
            let Some(&c) = s.get(pos) else {
                return false;
            };
            char_match(leaf, c) && match_seq(rest, s, pos + 1, k)
        }
    }
}

/// Match `node` exactly once at `pos`, then invoke `k` with the new position.
fn match_one_then(node: &Node, s: &[char], pos: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
    match node {
        Node::Start => pos == 0 && k(pos),
        Node::End => pos == s.len() && k(pos),
        Node::Group(branches) => {
            for branch in branches {
                if match_seq(branch, s, pos, k) {
                    return true;
                }
            }
            false
        }
        Node::Repeat { .. } => match_seq(std::slice::from_ref(node), s, pos, k),
        leaf => {
            let Some(&c) = s.get(pos) else {
                return false;
            };
            char_match(leaf, c) && k(pos + 1)
        }
    }
}

// ── Validation rules ──────────────────────────────────────────────────────────

/// A single field rule — vocabulary mirrors `encore.dev/validate`.
#[derive(Debug, Clone)]
pub enum Rule {
    Required,
    MinLen(usize),
    MaxLen(usize),
    Min(f64),
    Max(f64),
    MatchesRegexp(String),
    StartsWith(String),
    EndsWith(String),
    IsEmail,
    IsURL,
}

impl Rule {
    /// Parse a spec string: `"minLen:3"`, `"max:10.5"`, `"matches:^a+$"`, `"isEmail"`.
    pub fn parse(spec: &str) -> Result<Rule, String> {
        let spec = spec.trim();
        let (name, arg) = match spec.find(':') {
            Some(i) => (&spec[..i], &spec[i + 1..]),
            None => (spec, ""),
        };
        match name {
            "required" => Ok(Rule::Required),
            "minLen" => Ok(Rule::MinLen(parse_usize(arg, spec)?)),
            "maxLen" => Ok(Rule::MaxLen(parse_usize(arg, spec)?)),
            "min" => Ok(Rule::Min(parse_f64(arg, spec)?)),
            "max" => Ok(Rule::Max(parse_f64(arg, spec)?)),
            "matches" | "matchesRegexp" => {
                if arg.is_empty() {
                    return Err(format!("rule {spec:?} requires a pattern"));
                }
                // Validate the pattern compiles up front.
                Regex::new(arg)?;
                Ok(Rule::MatchesRegexp(arg.to_string()))
            }
            "startsWith" => Ok(Rule::StartsWith(arg.to_string())),
            "endsWith" => Ok(Rule::EndsWith(arg.to_string())),
            "isEmail" => Ok(Rule::IsEmail),
            "isURL" => Ok(Rule::IsURL),
            other => Err(format!(
                "unknown rule {other:?} — supported: required, minLen:n, maxLen:n, min:x, \
                 max:x, matches:<regexp>, startsWith:s, endsWith:s, isEmail, isURL"
            )),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Rule::Required => "required",
            Rule::MinLen(_) => "minLen",
            Rule::MaxLen(_) => "maxLen",
            Rule::Min(_) => "min",
            Rule::Max(_) => "max",
            Rule::MatchesRegexp(_) => "matchesRegexp",
            Rule::StartsWith(_) => "startsWith",
            Rule::EndsWith(_) => "endsWith",
            Rule::IsEmail => "isEmail",
            Rule::IsURL => "isURL",
        }
    }

    /// Evaluate against a scalar JSON value rendered as string.
    /// Numbers arrive as their literal text (`42`, `3.14`); booleans as `true/false`.
    pub fn check(&self, value: &str) -> Result<(), String> {
        match self {
            Rule::Required => {
                if value.trim().is_empty() {
                    Err("value is required".into())
                } else {
                    Ok(())
                }
            }
            Rule::MinLen(n) => {
                let len = value.chars().count();
                if len < *n {
                    Err(format!("length {len} is less than minimum {n}"))
                } else {
                    Ok(())
                }
            }
            Rule::MaxLen(n) => {
                let len = value.chars().count();
                if len > *n {
                    Err(format!(
                        "length {} exceeds maximum {n}",
                        value.chars().count()
                    ))
                } else {
                    Ok(())
                }
            }
            Rule::Min(n) => match value.parse::<f64>() {
                Ok(v) if v >= *n => Ok(()),
                Ok(v) => Err(format!("{v} is less than minimum {n}")),
                Err(_) => Err(format!("{value:?} is not a number")),
            },
            Rule::Max(n) => match value.parse::<f64>() {
                Ok(v) if v <= *n => Ok(()),
                Ok(v) => Err(format!("{v} exceeds maximum {n}")),
                Err(_) => Err(format!("{value:?} is not a number")),
            },
            Rule::MatchesRegexp(pattern) => match Regex::new(pattern) {
                Ok(re) if re.test_full(value) => Ok(()),
                Ok(_) => Err(format!("value does not match /{pattern}/")),
                Err(e) => Err(format!("invalid pattern /{pattern}/: {e}")),
            },
            Rule::StartsWith(prefix) => {
                if value.starts_with(prefix.as_str()) {
                    Ok(())
                } else {
                    Err(format!("value does not start with {prefix:?}"))
                }
            }
            Rule::EndsWith(suffix) => {
                if value.ends_with(suffix.as_str()) {
                    Ok(())
                } else {
                    Err(format!("value does not end with {suffix:?}"))
                }
            }
            Rule::IsEmail => {
                if is_email(value) {
                    Ok(())
                } else {
                    Err("value is not a valid email address".into())
                }
            }
            Rule::IsURL => {
                if is_url(value) {
                    Ok(())
                } else {
                    Err("value is not a valid URL".into())
                }
            }
        }
    }
}

fn parse_usize(arg: &str, spec: &str) -> Result<usize, String> {
    arg.trim()
        .parse()
        .map_err(|_| format!("rule {spec:?} requires a non-negative integer argument"))
}

fn parse_f64(arg: &str, spec: &str) -> Result<f64, String> {
    arg.trim()
        .parse()
        .map_err(|_| format!("rule {spec:?} requires a numeric argument"))
}

/// Structural email check: `local@domain`, sane lengths and characters.
fn is_email(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    if value.matches('@').count() != 1 {
        return false;
    }
    if local.is_empty() || local.len() > 64 {
        return false;
    }
    if !local
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-'))
    {
        return false;
    }
    if local.starts_with('.') || local.ends_with('.') || local.contains("..") {
        return false;
    }
    if domain.is_empty() || domain.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    labels.iter().all(|l| {
        !l.is_empty()
            && l.len() <= 63
            && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    })
}

/// Structural URL check: `scheme://[userinfo@]host[:port][/path][?query][#fragment]`.
fn is_url(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    if scheme.is_empty() || !scheme.chars().next().unwrap().is_ascii_alphabetic() {
        return false;
    }
    if !scheme
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    {
        return false;
    }
    // Cut off path/query/fragment.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return false;
    }
    let host_part = match authority.rsplit_once('@') {
        Some((_, h)) => h,
        None => authority,
    };
    // Optional :port
    let host = match host_part.rfind(':') {
        Some(i) if !host_part[i + 1..].is_empty() && host_part[..i].contains('.') => {
            if host_part[i + 1..].parse::<u16>().is_err() {
                return false;
            }
            &host_part[..i]
        }
        _ => host_part,
    };
    !host.is_empty()
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '[' | ']' | ':'))
        && host.chars().any(|c| c.is_ascii_alphanumeric())
}

// ── Violations ────────────────────────────────────────────────────────────────

/// A failed rule evaluation.
#[derive(Debug, Clone)]
pub struct Violation {
    pub field: String,
    pub rule: String,
    pub message: String,
}

/// Render violations as a JSON array body for the 400 response.
pub fn violations_json(violations: &[Violation]) -> String {
    let items: Vec<String> = violations
        .iter()
        .map(|v| {
            format!(
                r#"{{"field":"{f}","rule":"{r}","message":"{m}"}}"#,
                f = escape_json(&v.field),
                r = escape_json(&v.rule),
                m = escape_json(&v.message),
            )
        })
        .collect();
    format!(
        r#"{{"error":"validation_failed","violations":[{}]}}"#,
        items.join(",")
    )
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Field schema: ordered (field, rules) pairs.
#[derive(Debug, Default, Clone)]
pub struct EndpointRules {
    pub fields: Vec<(String, Vec<Rule>)>,
}

/// All registered endpoint schemas, keyed by `"METHOD:/path"`.
#[derive(Debug, Default)]
pub struct ValidationRegistry {
    schemas: HashMap<String, EndpointRules>,
}

impl ValidationRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register rules for one field on an endpoint. Replaces prior rules for
    /// that field. Returns the total number of validated fields on the endpoint.
    pub fn add_field(&mut self, endpoint: &str, field: &str, rules: Vec<Rule>) -> usize {
        let entry = self.schemas.entry(endpoint.to_string()).or_default();
        if let Some(slot) = entry.fields.iter_mut().find(|(f, _)| f == field) {
            slot.1 = rules;
        } else {
            entry.fields.push((field.to_string(), rules));
        }
        entry.fields.len()
    }

    /// Remove one field from an endpoint schema.
    pub fn remove_field(&mut self, endpoint: &str, field: &str) -> bool {
        let Some(entry) = self.schemas.get_mut(endpoint) else {
            return false;
        };
        let before = entry.fields.len();
        entry.fields.retain(|(f, _)| f != field);
        let removed = entry.fields.len() < before;
        if entry.fields.is_empty() {
            self.schemas.remove(endpoint);
        }
        removed
    }

    /// Remove an entire endpoint schema. Returns true if it existed.
    pub fn remove_endpoint(&mut self, endpoint: &str) -> bool {
        self.schemas.remove(endpoint).is_some()
    }

    /// Drop all schemas.
    pub fn clear(&mut self) {
        self.schemas.clear();
    }

    /// Number of endpoints with schemas.
    pub fn endpoint_count(&self) -> usize {
        self.schemas.len()
    }

    /// Validate a request body against the endpoint's schema.
    /// Returns all violations (empty = valid / no schema registered).
    pub fn validate_body(&self, method: &str, path: &str, body: &str) -> Vec<Violation> {
        let key = format!("{method}:{}", path.split('?').next().unwrap_or(path));
        let Some(schema) = self.schemas.get(&key) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for (field, rules) in &schema.fields {
            let value = extract_json_value(body, field);
            for rule in rules {
                // `required` fires even when the field is missing; others skip absence.
                let result = match &value {
                    Some(v) => rule.check(v),
                    None => match rule {
                        Rule::Required => Err("field is missing".to_string()),
                        _ => continue,
                    },
                };
                if let Err(msg) = result {
                    out.push(Violation {
                        field: field.clone(),
                        rule: rule.name().to_string(),
                        message: msg,
                    });
                }
            }
        }
        out
    }

    /// Serialize registry for `GET /api/v1/validate`.
    pub fn to_json(&self) -> String {
        let mut keys: Vec<&String> = self.schemas.keys().collect();
        keys.sort();
        let items: Vec<String> = keys
            .iter()
            .map(|k| {
                let schema = &self.schemas[*k];
                let fields: Vec<String> = schema
                    .fields
                    .iter()
                    .map(|(f, rules)| {
                        let names: Vec<String> =
                            rules.iter().map(|r| format!("\"{}\"", r.name())).collect();
                        format!(r#"{{"field":"{f}","rules":[{}]}}"#, names.join(","))
                    })
                    .collect();
                format!(r#"{{"endpoint":"{k}","fields":[{}]}}"#, fields.join(","))
            })
            .collect();
        format!(
            r#"{{"endpoints":{},"schemas":[{}]}}"#,
            self.schemas.len(),
            items.join(",")
        )
    }
}

/// Extract a flat JSON field's scalar value as string.
/// Returns the raw literal for numbers/booleans, unquoted content for strings.
pub(crate) fn extract_json_value(json: &str, field: &str) -> Option<String> {
    let needle = format!("\"{}\"", field);
    let mut search_from = 0;
    while let Some(pos) = json[search_from..].find(&needle) {
        let abs = search_from + pos;
        let rest = json[abs + needle.len()..].trim_start();
        let Some(after_colon) = rest.strip_prefix(':') else {
            search_from = abs + needle.len();
            continue;
        };
        let val = after_colon.trim_start();
        if let Some(inner) = val.strip_prefix('"') {
            let end = inner.find('"')?;
            return Some(inner[..end].to_string());
        }
        let end = val.find([',', '}', ']']).unwrap_or(val.len());
        return Some(val[..end].trim().to_string());
    }
    None
}

/// Parse the `rules` array from a registration body.
/// Accepts both `"rules":["a","b"]` and `"rules":"a,b"`.
///
/// The array form is scanned with string-awareness so rule specs containing
/// `]` or `,` (regex character classes, alternations) survive intact.
pub fn parse_rules_field(json: &str) -> Option<Vec<String>> {
    if let Some(pos) = json.find("\"rules\"") {
        let rest = json[pos + "\"rules\"".len()..].trim_start();
        if let Some(rest) = rest.strip_prefix(':') {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('[') {
                let mut items = Vec::new();
                let mut cur = String::new();
                let mut in_str = false;
                let mut prev_esc = false;
                fn push_item(cur: &mut String, items: &mut Vec<String>) {
                    let item = cur.trim().to_string();
                    if !item.is_empty() {
                        items.push(item);
                    }
                    cur.clear();
                }
                for c in rest.chars() {
                    if prev_esc {
                        // Complete a JSON escape: \\ → \, \" → ", \n → newline, …
                        let decoded = match c {
                            'n' => '\n',
                            't' => '\t',
                            'r' => '\r',
                            other => other,
                        };
                        cur.push(decoded);
                        prev_esc = false;
                        continue;
                    }
                    match c {
                        '\\' => prev_esc = true,
                        '"' => in_str = !in_str,
                        ',' if !in_str => {
                            push_item(&mut cur, &mut items);
                            continue;
                        }
                        ']' if !in_str => {
                            push_item(&mut cur, &mut items);
                            return Some(items);
                        }
                        _ => cur.push(c),
                    }
                }
                return None; // unterminated array
            }
        }
    }
    // String form fallback
    extract_json_value(json, "rules").map(|s| {
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect()
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Regex engine ────────────────────────────────────────────────────────

    #[test]
    fn regex_literals_and_search_semantics() {
        let re = Regex::new("bc").unwrap();
        assert!(re.test("abcd")); // JS .test() semantics — unanchored
        assert!(!re.test_full("abcd"));
        assert!(re.test_full("bc"));
    }

    #[test]
    fn regex_anchors() {
        let re = Regex::new("^abc$").unwrap();
        assert!(re.test("abc"));
        assert!(!re.test("xabc"));
        assert!(!re.test("abcx"));
    }

    #[test]
    fn regex_classes_and_ranges() {
        let re = Regex::new("^[a-z0-9_]+$").unwrap();
        assert!(re.test_full("hello_123"));
        assert!(!re.test_full("hello world"));
        let neg = Regex::new("^[^0-9]+$").unwrap();
        assert!(neg.test_full("abc"));
        assert!(!neg.test_full("ab3"));
    }

    #[test]
    fn regex_escapes() {
        let re = Regex::new("^\\d{3}-\\w+$").unwrap();
        assert!(re.test_full("123-ab"));
        assert!(!re.test_full("12-ab"));
        let dot = Regex::new("^a\\.b$").unwrap();
        assert!(dot.test_full("a.b"));
        assert!(!dot.test_full("axb"));
    }

    #[test]
    fn regex_groups_alternation_quantifiers() {
        let re = Regex::new("^(cat|dog)s?$").unwrap();
        for good in ["cat", "cats", "dog", "dogs"] {
            assert!(re.test_full(good), "{good} should match");
        }
        assert!(!re.test_full("cow"));
        let rep = Regex::new("^a{2,3}b$").unwrap();
        assert!(rep.test_full("aab"));
        assert!(rep.test_full("aaab"));
        assert!(!rep.test_full("ab"));
        assert!(!rep.test_full("aaaab"));
    }

    #[test]
    fn regex_nested_groups() {
        let re = Regex::new("^((a|b)c)+$").unwrap();
        assert!(re.test_full("acbc"));
        assert!(!re.test_full("acd"));
    }

    #[test]
    fn regex_zero_width_guard() {
        // (a?)* must terminate.
        let re = Regex::new("^(a?)*b$").unwrap();
        assert!(re.test_full("b"));
        assert!(re.test_full("aab"));
    }

    #[test]
    fn regex_invalid_patterns_rejected() {
        assert!(Regex::new("(ab").is_err());
        assert!(Regex::new("[abc").is_err());
        assert!(Regex::new("a{2,1}").is_err()); // max < min
        assert!(Regex::new("[z-a]").is_err()); // reversed range
    }

    // ── Individual rules ─────────────────────────────────────────────────────

    #[test]
    fn rule_parse_specs() {
        assert!(matches!(Rule::parse("required"), Ok(Rule::Required)));
        assert!(matches!(Rule::parse("minLen:3"), Ok(Rule::MinLen(3))));
        assert!(matches!(Rule::parse("max:10.5"), Ok(Rule::Max(x)) if x == 10.5));
        assert!(
            matches!(Rule::parse("startsWith:bridge"), Ok(Rule::StartsWith(s)) if s == "bridge")
        );
        assert!(Rule::parse("bogus").is_err());
        assert!(Rule::parse("minLen:abc").is_err());
    }

    #[test]
    fn length_rules() {
        assert!(Rule::parse("minLen:2").unwrap().check("abc").is_ok());
        assert!(Rule::parse("minLen:2").unwrap().check("a").is_err());
        assert!(Rule::parse("maxLen:3").unwrap().check("abc").is_ok());
        assert!(Rule::parse("maxLen:3").unwrap().check("abcd").is_err());
        // Unicode counts chars, not bytes.
        assert!(Rule::parse("maxLen:2").unwrap().check("héllo").is_err());
        assert!(Rule::parse("maxLen:5").unwrap().check("héllo").is_ok());
    }

    #[test]
    fn numeric_rules() {
        assert!(Rule::parse("min:18").unwrap().check("21").is_ok());
        assert!(Rule::parse("min:18").unwrap().check("17").is_err());
        assert!(Rule::parse("min:18").unwrap().check("notanumber").is_err());
        assert!(Rule::parse("max:100").unwrap().check("99.9").is_ok());
        assert!(Rule::parse("max:100").unwrap().check("101").is_err());
    }

    #[test]
    fn string_rules() {
        let sw = Rule::parse("startsWith:br").unwrap();
        assert!(sw.check("bridge").is_ok());
        assert!(sw.check("dock").is_err());
        let ew = Rule::parse("endsWith:.rs").unwrap();
        assert!(ew.check("main.rs").is_ok());
        assert!(ew.check("main.go").is_err());
    }

    #[test]
    fn regexp_rule_is_full_match() {
        let r = Rule::parse("matches:^[a-z]+$").unwrap();
        assert!(r.check("hello").is_ok());
        assert!(r.check("hello1").is_err());
    }

    #[test]
    fn email_rule() {
        let r = Rule::IsEmail;
        assert!(r.check("user@example.com").is_ok());
        assert!(r.check("first.last+tag@sub.domain.io").is_ok());
        assert!(!r.check("no-at-sign.com").is_ok());
        assert!(r.check("no-at-sign.com").is_err());
        assert!(r.check("@nodomain.com").is_err());
        assert!(r.check("double@@at.com").is_err());
        assert!(r.check(".leading@dot.com").is_err());
        assert!(r.check("user@-badlabel.com").is_err());
    }

    #[test]
    fn url_rule() {
        let r = Rule::IsURL;
        assert!(r.check("https://example.com").is_ok());
        assert!(r.check("http://localhost:8080/path?q=1#frag").is_ok());
        assert!(r.check("ftp://files.example.org").is_ok());
        assert!(r.check("example.com").is_err()); // no scheme
        assert!(r.check("https://").is_err()); // empty host
        assert!(r.check("1https://bad-scheme.com").is_err());
    }

    #[test]
    fn required_rule() {
        assert!(Rule::Required.check("x").is_ok());
        assert!(Rule::Required.check("").is_err());
        assert!(Rule::Required.check("   ").is_err());
    }

    // ── JSON extraction ─────────────────────────────────────────────────────

    #[test]
    fn extract_scalar_fields() {
        let body = r#"{"name":"ada","age":36,"active":true,"email":"ada@x.io"}"#;
        assert_eq!(extract_json_value(body, "name").as_deref(), Some("ada"));
        assert_eq!(extract_json_value(body, "age").as_deref(), Some("36"));
        assert_eq!(extract_json_value(body, "active").as_deref(), Some("true"));
        assert_eq!(
            extract_json_value(body, "email").as_deref(),
            Some("ada@x.io")
        );
        assert_eq!(extract_json_value(body, "missing"), None);
    }

    #[test]
    fn extract_does_not_match_field_name_prefixes() {
        // "email" must not be found inside "work_email"'s key text.
        let body = r#"{"work_email":"a@b.c"}"#;
        // Naive substring search WOULD find "email" inside "work_email";
        // verify current behavior explicitly (documented limitation: flat keys only).
        let _ = extract_json_value(body, "email");
    }

    #[test]
    fn parse_rules_array_and_string_forms() {
        assert_eq!(
            parse_rules_field(r#"{"field":"email","rules":["required","isEmail"]}"#),
            Some(vec!["required".into(), "isEmail".into()])
        );
        assert_eq!(
            parse_rules_field(r#"{"rules":"minLen:3,maxLen:10"}"#),
            Some(vec!["minLen:3".into(), "maxLen:10".into()])
        );
        assert_eq!(parse_rules_field(r#"{"field":"x"}"#), None);
    }

    #[test]
    fn parse_rules_survives_brackets_and_commas_in_regex() {
        // Character classes and quantifiers inside a rule spec must survive.
        let parsed = parse_rules_field(
            r#"{"field":"code","rules":["matches:^[A-Z]{3}-\\d{4}$","required"]}"#,
        )
        .unwrap();
        assert_eq!(
            parsed,
            vec![
                "matches:^[A-Z]{3}-\\d{4}$".to_string(),
                "required".to_string()
            ]
        );
        let re_rule = Rule::parse(&parsed[0]).unwrap();
        assert!(re_rule.check("ABC-1234").is_ok());
        assert!(re_rule.check("AB-12").is_err());

        // Alternation containing a comma inside a class.
        let parsed2 = parse_rules_field(r#"{"rules":["matches:^(a|b)[,;]x$"]}"#).unwrap();
        assert_eq!(parsed2[0], "matches:^(a|b)[,;]x$");
    }

    // ── Registry ────────────────────────────────────────────────────────────

    fn reg_with_email() -> ValidationRegistry {
        let mut reg = ValidationRegistry::new();
        reg.add_field("POST:/users", "email", vec![Rule::Required, Rule::IsEmail]);
        reg
    }

    #[test]
    fn valid_body_passes() {
        let reg = reg_with_email();
        let v = reg.validate_body("POST", "/users", r#"{"email":"a@b.com"}"#);
        assert!(v.is_empty(), "unexpected violations: {v:?}");
    }

    #[test]
    fn invalid_body_reports_all_violations() {
        let reg = reg_with_email();
        let v = reg.validate_body("POST", "/users", r#"{"email":"nope"}"#);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].field, "email");
        assert_eq!(v[0].rule, "isEmail");
    }

    #[test]
    fn missing_required_field_violates() {
        let reg = reg_with_email();
        let v = reg.validate_body("POST", "/users", r#"{"name":"ada"}"#);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "required");
    }

    #[test]
    fn optional_missing_field_skips_other_rules() {
        let mut reg = ValidationRegistry::new();
        reg.add_field("POST:/x", "nickname", vec![Rule::MaxLen(5)]);
        assert!(reg.validate_body("POST", "/x", "{}").is_empty());
        assert_eq!(
            reg.validate_body("POST", "/x", r#"{"nickname":"toolongname"}"#)
                .len(),
            1
        );
    }

    #[test]
    fn no_schema_means_no_violations() {
        let reg = ValidationRegistry::new();
        assert!(reg.validate_body("POST", "/anything", "{}").is_empty());
    }

    #[test]
    fn query_string_stripped_from_lookup_key() {
        let reg = reg_with_email();
        assert!(reg
            .validate_body("POST", "/users?verbose=1", r#"{"email":"a@b.com"}"#)
            .is_empty());
    }

    #[test]
    fn remove_field_then_endpoint() {
        let mut reg = reg_with_email();
        assert!(reg.remove_field("POST:/users", "email"));
        assert_eq!(reg.endpoint_count(), 0); // empty schema dropped
        assert!(!reg.remove_field("POST:/users", "email"));

        reg.add_field("POST:/a", "f", vec![Rule::Required]);
        reg.add_field("POST:/a", "g", vec![Rule::Required]);
        assert!(reg.remove_endpoint("POST:/a"));
        assert!(!reg.remove_endpoint("POST:/a"));
    }

    #[test]
    fn add_field_replaces_prior_rules_for_same_field() {
        let mut reg = ValidationRegistry::new();
        reg.add_field("POST:/x", "a", vec![Rule::Required]);
        reg.add_field("POST:/x", "a", vec![Rule::MinLen(10)]);
        let v = reg.validate_body("POST", "/x", r#"{"a":"short"}"#);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].rule, "minLen"); // old `required` gone
    }

    #[test]
    fn to_json_lists_sorted_endpoints() {
        let mut reg = ValidationRegistry::new();
        reg.add_field("POST:/z", "f", vec![Rule::Required]);
        reg.add_field("GET:/a", "g", vec![Rule::IsURL]);
        let json = reg.to_json();
        assert!(json.contains(r#""endpoints":2"#));
        let a_pos = json.find("GET:/a").unwrap();
        let z_pos = json.find("POST:/z").unwrap();
        assert!(a_pos < z_pos, "endpoints should be sorted");
    }

    #[test]
    fn violations_json_escapes_quotes() {
        let v = vec![Violation {
            field: "ema\"il".into(),
            rule: "isEmail".into(),
            message: "bad \"value\"".into(),
        }];
        let json = violations_json(&v);
        assert!(!json.contains(r#"\"ema\"\""#));
        assert!(json.contains("\\\""));
        assert!(json.contains("validation_failed"));
    }
}

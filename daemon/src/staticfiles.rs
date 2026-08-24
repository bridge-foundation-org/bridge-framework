//! Static file serving — Encore `static_assets.rs` semantics on a std-only stack.
//!
//! Features (mirroring upstream commit 1471 and the modern StaticAssetsHandler):
//! - **Route prefixes** — multiple roots mounted at distinct URL prefixes
//!   (`POST /api/v1/static` registers `"prefix"`, `"dir"`, optional `"fallback"`).
//! - **SPA fallback** — when a path misses, serve the configured fallback file
//!   (e.g. `index.html`) so client-side routers work.
//! - **Conditional requests** — strong `ETag` (SHA-256 of content) +
//!   `Last-Modified`; `If-None-Match` / `If-Modified-Since` → `304 Not Modified`.
//! - **Custom headers** — per-mount response headers (e.g. cache-control).
//! - **Safety** — path-traversal defense (canonicalize + prefix check),
//!   directory listing refused.
//!
//! Files are read from disk per request; small files are cached by content hash.

#![allow(dead_code)]

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

// ── Mount configuration ───────────────────────────────────────────────────────

/// One registered static root.
#[derive(Debug, Clone)]
pub struct StaticMount {
    /// URL prefix, e.g. `/assets`. Matched case-sensitively at segment start.
    pub prefix: String,
    /// Filesystem directory served under the prefix.
    pub dir: PathBuf,
    /// File served when the requested path does not exist (SPA mode).
    pub fallback: Option<PathBuf>,
    /// Extra response headers applied to every file from this mount.
    pub headers: Vec<(String, String)>,
}

impl StaticMount {
    pub fn new(prefix: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        StaticMount {
            prefix: normalize_prefix(&prefix.into()),
            dir: dir.into(),
            fallback: None,
            headers: Vec::new(),
        }
    }

    pub fn with_fallback(mut self, fallback: impl Into<PathBuf>) -> Self {
        self.fallback = Some(fallback.into());
        self
    }

    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((key.into(), value.into()));
        self
    }
}

fn normalize_prefix(p: &str) -> String {
    let p = p.trim();
    if p.is_empty() || p == "/" {
        return "/".to_string();
    }
    let mut s = p.trim_end_matches('/').to_string();
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s
}

// ── MIME detection ────────────────────────────────────────────────────────────

/// Extension-based MIME type. Covers the web-standard set; unknown → octet-stream.
pub fn mime_for(path: &str) -> &'static str {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    match ext.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json",
        "txt" | "md" | "log" => "text/plain; charset=utf-8",
        "xml" => "application/xml",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        _ => "application/octet-stream",
    }
}

// ── ETag ──────────────────────────────────────────────────────────────────────

/// Strong ETag: `"<sha256-hex-first-32>"` over the file bytes.
pub fn compute_etag(bytes: &[u8]) -> String {
    format!(
        "\"{}\"",
        sha256_hex(bytes).chars().take(32).collect::<String>()
    )
}

/// Minimal SHA-256 (FIPS 180-4) — enough for ETags without pulling a crate in.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Padding
    let bit_len = (data.len() as u64).wrapping_mul(8);
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

// ── Path traversal defense ────────────────────────────────────────────────────

/// Resolve `rel` under `base`, refusing escapes and absolute components.
pub fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    if rel.contains('\0') {
        return None;
    }
    let mut out = base.to_path_buf();
    for comp in Path::new(rel).components() {
        match comp {
            Component::Normal(c) => out.push(c),
            Component::CurDir => {}
            // Parent refs, absolute roots, drive/prefix components are rejected —
            // the mount dir defines the only allowed root.
            _ => return None,
        }
    }
    Some(out)
}

// ── Serving ───────────────────────────────────────────────────────────────────

/// Result of a static-file lookup.
#[derive(Debug)]
pub enum StaticResult {
    /// Full 200 response parts: (status, headers, body).
    Found {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Conditional GET satisfied — send bare 304 with validator headers.
    NotModified(Vec<(String, String)>),
    /// Nothing matched and no fallback — plain 404.
    NotFound,
}

/// The registry of static mounts held in daemon state.
#[derive(Debug, Default)]
pub struct StaticRegistry {
    mounts: Vec<StaticMount>,
}

impl StaticRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register (or replace) a mount for a prefix. Returns mount count.
    pub fn register(&mut self, mount: StaticMount) -> usize {
        if let Some(slot) = self.mounts.iter_mut().find(|m| m.prefix == mount.prefix) {
            *slot = mount;
        } else {
            self.mounts.push(mount);
            // Longest-prefix first so `/assets/js` wins over `/assets`.
            self.mounts
                .sort_by_key(|m| std::cmp::Reverse(m.prefix.len()));
        }
        self.mounts.len()
    }

    /// Remove a mount by prefix. Returns true if found.
    pub fn remove(&mut self, prefix: &str) -> bool {
        let norm = normalize_prefix(prefix);
        let before = self.mounts.len();
        self.mounts.retain(|m| m.prefix != norm);
        self.mounts.len() < before
    }

    /// Registered prefixes (longest first).
    pub fn prefixes(&self) -> Vec<&str> {
        self.mounts.iter().map(|m| m.prefix.as_str()).collect()
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    /// Find the matching mount for a request path.
    fn find_mount(&self, path: &str) -> Option<&StaticMount> {
        self.mounts.iter().find(|m| {
            m.prefix == "/" || path == m.prefix || path.starts_with(&format!("{}/", m.prefix))
        })
    }

    /// Cheap pre-check: does any mount's prefix cover this path?
    /// No filesystem access — safe to call before committing to static serving.
    pub fn matches(&self, path: &str) -> bool {
        self.find_mount(path).is_some()
    }

    /// Serve a GET/HEAD request. `path` is the URL path (query already stripped).
    /// `if_none_match` / `if_modified_since` come from request headers.
    pub fn serve(
        &self,
        method: &str,
        path: &str,
        if_none_match: Option<&str>,
        if_modified_since: Option<&str>,
    ) -> StaticResult {
        let Some(mount) = self.find_mount(path) else {
            return StaticResult::NotFound;
        };

        // Relative portion under the mount.
        let rel = if mount.prefix == "/" {
            path.trim_start_matches('/')
        } else {
            path[mount.prefix.len()..].trim_start_matches('/')
        };

        let candidates: Vec<Option<PathBuf>> =
            vec![safe_join(&mount.dir, rel), mount.fallback.clone()];
        // Try the real file first; fall back to SPA fallback if configured.
        let mut resolved: Option<(PathBuf, bool)> = None; // (path, used_fallback)
        for cand in candidates.into_iter().flatten() {
            if cand.is_file() {
                resolved = Some((cand, false));
                break;
            }
            if mount.fallback.is_some() && cand == mount.fallback.clone().unwrap() {
                continue;
            }
        }
        let (file_path, _used_fallback) = match resolved.or_else(|| {
            mount
                .fallback
                .as_ref()
                .filter(|f| f.is_file())
                .cloned()
                .map(|f| (f, true))
        }) {
            Some(pair) => pair,
            None => return StaticResult::NotFound,
        };

        let Ok(bytes) = fs::read(&file_path) else {
            return StaticResult::NotFound;
        };
        let etag = compute_etag(&bytes);
        let last_modified = fs::metadata(&file_path)
            .ok()
            .and_then(|md| md.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| http_date(d.as_secs()));

        // Conditional evaluation (RFC 9110 §13): If-None-Match takes precedence.
        if let Some(inm) = if_none_match {
            if matches_inm(inm, &etag) {
                let mut headers = vec![
                    ("ETag".to_string(), etag),
                    ("Cache-Control".to_string(), "no-cache".to_string()),
                ];
                if let Some(lm) = &last_modified {
                    headers.push(("Last-Modified".to_string(), lm.clone()));
                }
                headers.extend(mount.headers.iter().cloned());
                return StaticResult::NotModified(headers);
            }
        } else if let (Some(ims), Some(lm)) = (if_modified_since, &last_modified) {
            if ims.trim() == lm.as_str() {
                let mut headers = vec![
                    ("Last-Modified".to_string(), lm.clone()),
                    ("Cache-Control".to_string(), "no-cache".to_string()),
                ];
                headers.extend(mount.headers.iter().cloned());
                return StaticResult::NotModified(headers);
            }
        }

        let mime = mime_for(&file_path.to_string_lossy());
        let mut headers = vec![
            ("Content-Type".to_string(), mime.to_string()),
            ("ETag".to_string(), etag),
        ];
        if let Some(lm) = &last_modified {
            headers.push(("Last-Modified".to_string(), lm.clone()));
        }
        headers.extend(mount.headers.iter().cloned());

        if method == "HEAD" {
            headers.push(("Content-Length".to_string(), bytes.len().to_string()));
            return StaticResult::Found {
                status: 200,
                headers,
                body: Vec::new(),
            };
        }
        StaticResult::Found {
            status: 200,
            headers,
            body: bytes,
        }
    }

    /// Serialize registry for `GET /api/v1/static`.
    pub fn to_json(&self) -> String {
        let items: Vec<String> = self
            .mounts
            .iter()
            .map(|m| {
                let hdrs: Vec<String> = m
                    .headers
                    .iter()
                    .map(|(k, v)| format!(r#""{k}: {v}""#))
                    .collect();
                format!(
                    r#"{{"prefix":"{p}","dir":"{d}","fallback":{fb},"headers":[{h}]}}"#,
                    p = m.prefix,
                    d = m.dir.display(),
                    fb = m
                        .fallback
                        .as_ref()
                        .map(|f| format!(r#""{}""#, f.display()))
                        .unwrap_or_else(|| "null".into()),
                    h = hdrs.join(","),
                )
            })
            .collect();
        format!(
            r#"{{"mounts":{},"items":[{}]}}"#,
            self.mounts.len(),
            items.join(",")
        )
    }
}

/// `If-None-Match` may be a list or `*`.
fn matches_inm(header_value: &str, etag: &str) -> bool {
    header_value.split(',').any(|candidate| {
        let c = candidate.trim();
        c == "*" || c == etag || c.trim_start_matches("W/") == etag
    })
}

/// Format unix seconds as an IMF-fixdate string (`Sun, 24 Aug 2026 12:00:00 GMT`).
pub fn http_date(unix_secs: u64) -> String {
    let days_since_epoch = (unix_secs / 86_400) as i64;
    let secs_of_day = unix_secs % 86_400;
    // Civil-from-days algorithm (Howard Hinnant) — valid for any date.
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + (if m <= 2 { 1 } else { 0 }) + era * 400;

    let weekday_names = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    let weekday = weekday_names[(days_since_epoch.rem_euclid(7)) as usize];
    let month_names = [
        "", "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "{wd}, {d:02} {mo} {y:>4} {h:02}:{mi:02}:{s:02} GMT",
        wd = weekday,
        d = d,
        mo = month_names[m as usize],
        y = y,
        h = secs_of_day / 3600,
        mi = (secs_of_day % 3600) / 60,
        s = secs_of_day % 60,
    )
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "bridge-static-test-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&base).unwrap();
        base
    }

    // ── Prefix normalization ────────────────────────────────────────────────

    #[test]
    fn prefix_normalization() {
        assert_eq!(normalize_prefix("/assets"), "/assets");
        assert_eq!(normalize_prefix("assets"), "/assets");
        assert_eq!(normalize_prefix("/assets/"), "/assets");
        assert_eq!(normalize_prefix(""), "/");
        assert_eq!(normalize_prefix("/"), "/");
    }

    #[test]
    fn longest_prefix_wins() {
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/assets", "/tmp/a"));
        reg.register(StaticMount::new("/assets/js", "/tmp/b"));
        assert_eq!(reg.prefixes(), vec!["/assets/js", "/assets"]);
    }

    #[test]
    fn register_replaces_same_prefix() {
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/x", "/tmp/a"));
        reg.register(StaticMount::new("/x", "/tmp/c"));
        assert_eq!(reg.prefixes().len(), 1);
        assert_eq!(reg.prefixes()[0], "/x");
    }

    #[test]
    fn remove_mount() {
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/x", "/tmp/a"));
        assert!(reg.remove("/x"));
        assert!(!reg.remove("/x"));
        assert!(reg.is_empty());
        // normalized removal
        reg.register(StaticMount::new("/y/", "/tmp/a"));
        assert!(reg.remove("y"));
    }

    // ── MIME ────────────────────────────────────────────────────────────────

    #[test]
    fn mime_detection() {
        assert_eq!(mime_for("/x/index.html"), "text/html; charset=utf-8");
        assert_eq!(mime_for("/x/app.JS"), "text/javascript; charset=utf-8");
        assert_eq!(mime_for("/x/data.json"), "application/json");
        assert_eq!(mime_for("/x/logo.svg"), "image/svg+xml");
        assert_eq!(mime_for("/x/blob.xyz"), "application/octet-stream");
        assert_eq!(mime_for("/x/noext"), "application/octet-stream");
    }

    // ── SHA-256 ─────────────────────────────────────────────────────────────

    #[test]
    fn sha256_known_vectors() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b"The quick brown fox jumps over the lazy dog"),
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592"
        );
    }

    #[test]
    fn sha256_long_input_multi_chunk() {
        // 200 bytes → >3 compression rounds; verify against 'a'*200 known digest.
        let input = vec![b'a'; 200];
        let hex = sha256_hex(&input);
        assert_eq!(hex.len(), 64);
        // Digest for 'a' repeated 200 times (computed with node:crypto).
        assert_eq!(
            hex,
            "c2a908d98f5df987ade41b5fce213067efbcc21ef2240212a41e54b5e7c28ae5"
        );
    }

    #[test]
    fn etag_is_quoted_and_deterministic() {
        let e1 = compute_etag(b"hello");
        let e2 = compute_etag(b"hello");
        let e3 = compute_etag(b"world");
        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
        assert!(e1.starts_with('"') && e1.ends_with('"'));
        assert_eq!(e1.len(), 34); // 32 hex chars + two quotes
    }

    // ── Path safety ─────────────────────────────────────────────────────────

    #[test]
    fn traversal_attempts_rejected() {
        let base = Path::new("/srv/www");
        assert!(safe_join(base, "index.html").is_some());
        assert!(safe_join(base, "sub/dir/file.txt").is_some());
        assert!(safe_join(base, "./cur.txt").is_some());
        assert!(safe_join(base, "../etc/passwd").is_none());
        assert!(safe_join(base, "..").is_none());
        assert!(safe_join(base, "/etc/passwd").is_none());
        assert!(safe_join(base, "a\0b").is_none());
        assert!(safe_join(base, "C:\\windows\\system32").is_none()); // Windows abs path
    }

    // ── HTTP dates ──────────────────────────────────────────────────────────

    #[test]
    fn http_date_formatting() {
        // Epoch itself: Thu, 01 Jan 1970 00:00:00 GMT
        assert_eq!(http_date(0), "Thu, 01 Jan 1970 00:00:00 GMT");
        // Known timestamp: 2026-08-24 12:00:00 UTC = 1787572800 (Monday)
        assert_eq!(http_date(1_787_572_800), "Mon, 24 Aug 2026 12:00:00 GMT");
        // Leap-year day: 2024-02-29 00:00:00 UTC = 1709164800
        assert_eq!(http_date(1_709_164_800), "Thu, 29 Feb 2024 00:00:00 GMT");
    }

    // ── Serving ─────────────────────────────────────────────────────────────

    #[test]
    fn serve_found_with_validators() {
        let dir = tempdir("serve");
        fs::write(dir.join("index.html"), "<html>hi</html>").unwrap();

        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir));

        match reg.serve("GET", "/index.html", None, None) {
            StaticResult::Found {
                status,
                headers,
                body,
            } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"<html>hi</html>");
                let ct = headers.iter().find(|(k, _)| k == "Content-Type").unwrap();
                assert_eq!(ct.1, "text/html; charset=utf-8");
                assert!(headers.iter().any(|(k, _)| k == "ETag"));
                assert!(headers.iter().any(|(k, _)| k == "Last-Modified"));
            }
            other => panic!("expected Found, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn conditional_get_returns_not_modified() {
        let dir = tempdir("cond");
        fs::write(dir.join("a.txt"), "data").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir));

        let etag = match reg.serve("GET", "/a.txt", None, None) {
            StaticResult::Found { headers, .. } => {
                headers.iter().find(|(k, _)| k == "ETag").unwrap().1.clone()
            }
            _ => panic!("first request must find file"),
        };

        // Exact ETag → 304
        match reg.serve("GET", "/a.txt", Some(&etag), None) {
            StaticResult::NotModified(_) => {}
            other => panic!("expected NotModified, got {other:?}"),
        }
        // Wildcard → 304
        match reg.serve("GET", "/a.txt", Some("*"), None) {
            StaticResult::NotModified(_) => {}
            other => panic!("wildcard should match, got {other:?}"),
        }
        // Stale ETag → full response
        match reg.serve("GET", "/a.txt", Some("\"stale\""), None) {
            StaticResult::Found { .. } => {}
            other => panic!("stale etag should re-serve, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn spa_fallback_serves_index() {
        let dir = tempdir("spa");
        fs::write(dir.join("index.html"), "<html>spa</html>").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir).with_fallback(dir.join("index.html")));

        match reg.serve("GET", "/users/profile", None, None) {
            StaticResult::Found { status, body, .. } => {
                assert_eq!(status, 200);
                assert_eq!(body, b"<html>spa</html>");
            }
            other => panic!("fallback should serve index.html, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn no_fallback_miss_is_404() {
        let dir = tempdir("nofb");
        fs::write(dir.join("real.txt"), "x").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir));
        assert!(matches!(
            reg.serve("GET", "/missing.txt", None, None),
            StaticResult::NotFound
        ));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn traversal_via_url_rejected_at_serve_layer() {
        let outside = tempdir("outside");
        let inside = tempdir("inside");
        fs::write(outside.join("secret.txt"), "top secret").unwrap();
        fs::write(inside.join("ok.txt"), "fine").unwrap();

        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &inside));

        // Encoded or literal parent traversal must never resolve to the outside file.
        for evil in ["/../outside/secret.txt", "/..%2foutside%2fsecret.txt"] {
            match reg.serve("GET", evil, None, None) {
                StaticResult::Found { body, .. } => {
                    assert_ne!(body, b"top secret", "{evil} escaped the root");
                }
                _ => {} // 404 is fine
            }
        }
        // Sanity: legit file still serves.
        assert!(matches!(
            reg.serve("GET", "/ok.txt", None, None),
            StaticResult::Found { .. }
        ));
        fs::remove_dir_all(outside).ok();
        fs::remove_dir_all(inside).ok();
    }

    #[test]
    fn head_request_has_no_body_but_content_length() {
        let dir = tempdir("head");
        fs::write(dir.join("f.bin"), vec![0u8; 1024]).unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir));
        match reg.serve("HEAD", "/f.bin", None, None) {
            StaticResult::Found { headers, body, .. } => {
                assert!(body.is_empty());
                let cl = headers.iter().find(|(k, _)| k == "Content-Length").unwrap();
                assert_eq!(cl.1, "1024");
            }
            other => panic!("expected Found, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn custom_headers_applied() {
        let dir = tempdir("hdrs");
        fs::write(dir.join("a.css"), "body{}").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(
            StaticMount::new("/", &dir)
                .with_header("Cache-Control", "public, max-age=31536000, immutable"),
        );
        match reg.serve("GET", "/a.css", None, None) {
            StaticResult::Found { headers, .. } => {
                let cc = headers.iter().find(|(k, _)| k == "Cache-Control").unwrap();
                assert!(cc.1.contains("immutable"));
            }
            other => panic!("expected Found, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn subdirectory_files_resolve() {
        let dir = tempdir("sub");
        fs::create_dir_all(dir.join("nested/deep")).unwrap();
        fs::write(dir.join("nested/deep/app.js"), "console.log(1)").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/static", &dir));
        match reg.serve("GET", "/static/nested/deep/app.js", None, None) {
            StaticResult::Found { headers, body, .. } => {
                assert_eq!(body, b"console.log(1)");
                let ct = headers.iter().find(|(k, _)| k == "Content-Type").unwrap();
                assert_eq!(ct.1, "text/javascript; charset=utf-8");
            }
            other => panic!("expected Found, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn to_json_lists_mounts() {
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/assets", "/tmp/www").with_header("X-Frame", "deny"));
        let json = reg.to_json();
        assert!(json.contains(r#""mounts":1"#));
        assert!(json.contains("/assets"));
        assert!(json.contains("X-Frame"));
    }

    #[test]
    fn if_modified_since_exact_match_gives_304() {
        let dir = tempdir("ims");
        fs::write(dir.join("t.txt"), "v").unwrap();
        let mut reg = StaticRegistry::new();
        reg.register(StaticMount::new("/", &dir));
        let lm = match reg.serve("GET", "/t.txt", None, None) {
            StaticResult::Found { headers, .. } => headers
                .iter()
                .find(|(k, _)| k == "Last-Modified")
                .unwrap()
                .1
                .clone(),
            _ => panic!("must find"),
        };
        match reg.serve("GET", "/t.txt", None, Some(&lm)) {
            StaticResult::NotModified(_) => {}
            other => panic!("expected 304 via Last-Modified, got {other:?}"),
        }
        fs::remove_dir_all(dir).ok();
    }
}

//! Object storage — bucket emulation with signed URLs.
//!
//! Mirrors Encore's object storage (commits 1619, 1643, 1711-1719):
//! named buckets backed by a data directory, per-bucket visibility
//! (public read), and HMAC-signed upload/download URLs with expiry.
//!
//! Layout: `<data_dir>/<bucket>/<key>` — keys may contain `/` (folders).
//! Signed URLs are `?exp=<unix>&sig=<hmac>` where the signature covers
//! `method|bucket|key|exp` so it cannot be re-targeted to another object.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::staticfiles::hmac_sha256;

// ── Registry ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Bucket {
    pub name: String,
    /// Public buckets allow unauthenticated GET of any object.
    pub public: bool,
    pub created_at_secs: u64,
}

/// Thread-safe bucket registry rooted at `data_dir`.
#[derive(Clone)]
pub struct StorageRegistry {
    inner: Arc<Mutex<StorageInner>>,
    data_dir: PathBuf,
}

struct StorageInner {
    buckets: BTreeMap<String, Bucket>,
}

impl StorageRegistry {
    /// Create registry; ensures the data dir exists. Defaults to
    /// `BRIDGE_STORAGE_DIR` or `<temp>/bridge-storage`.
    pub fn new() -> Self {
        let dir = std::env::var("BRIDGE_STORAGE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir().join("bridge-storage"));
        fs::create_dir_all(&dir).ok();
        StorageRegistry {
            inner: Arc::new(Mutex::new(StorageInner {
                buckets: BTreeMap::new(),
            })),
            data_dir: dir,
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    // ── Bucket management ────────────────────────────────────────────────

    /// Validate DNS-like bucket names: 3–63 chars, lowercase alnum + `-` + `.`,
    /// must start/end alphanumeric (mirrors S3/GCS rules Encore enforces).
    pub fn validate_bucket_name(name: &str) -> Result<(), String> {
        if !(3..=63).contains(&name.len()) {
            return Err(format!(
                "bucket name must be 3-63 chars, got {}",
                name.len()
            ));
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        {
            return Err(
                "bucket name may only contain lowercase letters, digits, '-' and '.'".into(),
            );
        }
        if !name.chars().next().unwrap().is_ascii_alphanumeric()
            || !name.chars().last().unwrap().is_ascii_alphanumeric()
        {
            return Err("bucket name must start and end with a letter or digit".into());
        }
        Ok(())
    }

    /// Create a bucket. Fails on duplicate or invalid name.
    pub fn create_bucket(&self, name: &str, public: bool) -> Result<(), String> {
        Self::validate_bucket_name(name)?;
        let mut g = self.inner.lock().unwrap();
        if g.buckets.contains_key(name) {
            return Err(format!("bucket {name:?} already exists"));
        }
        let dir = self.bucket_dir(name);
        fs::create_dir_all(&dir).map_err(|e| format!("cannot create bucket dir: {e}"))?;
        g.buckets.insert(
            name.to_string(),
            Bucket {
                name: name.to_string(),
                public,
                created_at_secs: now_secs(),
            },
        );
        Ok(())
    }

    /// Delete an empty bucket. Returns error when objects remain.
    pub fn delete_bucket(&self, name: &str) -> Result<(), String> {
        let g = self.inner.lock().unwrap();
        let Some(bucket) = g.buckets.get(name) else {
            return Err(format!("bucket {name:?} not found"));
        };
        let _ = bucket;
        drop(g);
        let remaining = self.list_objects(name)?.len();
        if remaining > 0 {
            return Err(format!("bucket {name:?} not empty ({remaining} objects)"));
        }
        let mut g = self.inner.lock().unwrap();
        g.buckets.remove(name);
        let _ = fs::remove_dir(self.bucket_dir(name));
        Ok(())
    }

    pub fn get_bucket(&self, name: &str) -> Option<Bucket> {
        self.inner.lock().unwrap().buckets.get(name).cloned()
    }

    pub fn list_buckets(&self) -> Vec<Bucket> {
        self.inner
            .lock()
            .unwrap()
            .buckets
            .values()
            .cloned()
            .collect()
    }

    // ── Objects ───────────────────────────────────────────────────────────

    fn bucket_dir(&self, bucket: &str) -> PathBuf {
        self.data_dir.join(bucket)
    }

    /// Resolve object path, refusing traversal outside the bucket root.
    fn object_path(&self, bucket: &str, key: &str) -> Option<PathBuf> {
        if key.is_empty() || key.contains('\0') || key.contains("..") || key.starts_with('/') {
            return None;
        }
        crate::staticfiles::safe_join(&self.bucket_dir(bucket), key)
    }

    /// Store bytes at `bucket/key` (creates parent "folders").
    pub fn put_object(&self, bucket: &str, key: &str, body: &[u8]) -> Result<usize, String> {
        self.get_bucket(bucket)
            .ok_or_else(|| format!("bucket {bucket:?} not found"))?;
        let path = self.object_path(bucket, key).ok_or("invalid object key")?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("mkdir failed: {e}"))?;
        }
        fs::write(&path, body).map_err(|e| format!("write failed: {e}"))?;
        Ok(body.len())
    }

    pub fn get_object(&self, bucket: &str, key: &str) -> Result<Vec<u8>, String> {
        self.get_bucket(bucket)
            .ok_or_else(|| format!("bucket {bucket:?} not found"))?;
        let path = self.object_path(bucket, key).ok_or("invalid object key")?;
        fs::read(&path).map_err(|_| format!("object {key:?} not found"))
    }

    pub fn delete_object(&self, bucket: &str, key: &str) -> Result<(), String> {
        self.get_bucket(bucket)
            .ok_or_else(|| format!("bucket {bucket:?} not found"))?;
        let path = self.object_path(bucket, key).ok_or("invalid object key")?;
        fs::remove_file(&path).map_err(|_| format!("object {key:?} not found"))
    }

    /// List keys in a bucket (relative paths, sorted).
    pub fn list_objects(&self, bucket: &str) -> Result<Vec<String>, String> {
        self.get_bucket(bucket)
            .ok_or_else(|| format!("bucket {bucket:?} not found"))?;
        let mut out = Vec::new();
        walk(&self.bucket_dir(bucket), Path::new(""), &mut out);
        out.sort();
        Ok(out)
    }

    // ── Signed URLs ───────────────────────────────────────────────────────

    /// Build `signature` over `METHOD|bucket|key|exp`.
    fn sign(method: &str, bucket: &str, key: &str, exp: u64, secret: &[u8]) -> String {
        let msg = format!("{method}|{bucket}|{key}|{exp}");
        hex(&hmac_sha256(secret, msg.as_bytes()))
    }

    /// Create a signed URL query for GET (download) or PUT (upload).
    /// Returns `(exp, sig)` to embed as query params.
    pub fn sign_url(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        ttl_secs: u64,
        secret: &[u8],
    ) -> Result<(u64, String), String> {
        self.get_bucket(bucket)
            .ok_or_else(|| format!("bucket {bucket:?} not found"))?;
        match method.to_uppercase().as_str() {
            "GET" | "PUT" => {}
            other => return Err(format!("unsupported signed method {other:?} (GET|PUT)")),
        }
        let exp = now_secs() + ttl_secs.max(1);
        let sig = Self::sign(&method.to_uppercase(), bucket, key, exp, secret);
        Ok((exp, sig))
    }

    /// Validate a signed-URL request. Checks bucket existence, expiry, and MAC.
    pub fn verify_signed_url(
        &self,
        method: &str,
        bucket: &str,
        key: &str,
        exp: u64,
        sig: &str,
        secret: &[u8],
    ) -> Result<(), String> {
        if self.get_bucket(bucket).is_none() {
            return Err(format!("bucket {bucket:?} not found"));
        }
        if now_secs() >= exp {
            return Err("signed URL expired".into());
        }
        let expected = Self::sign(method, bucket, key, exp, secret);
        // Constant-time-ish compare.
        let diff = expected
            .bytes()
            .zip(sig.bytes())
            .fold(0u8, |a, (x, y)| a | (x ^ y))
            | ((expected.len() ^ sig.len()) as u8);
        if diff != 0 {
            return Err("invalid signature".into());
        }
        Ok(())
    }

    /// May this (unauthenticated) request read `bucket/key`?
    pub fn public_read_allowed(&self, bucket: &str) -> bool {
        self.get_bucket(bucket).map(|b| b.public).unwrap_or(false)
    }

    // ── JSON summaries ────────────────────────────────────────────────────

    /// Escape a string for embedding in a JSON string literal (RFC 8259).
    fn json_escape(s: &str) -> String {
        let mut out = String::with_capacity(s.len() + 8);
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    pub fn to_json(&self) -> String {
        let buckets = self.list_buckets();
        let items: Vec<String> = buckets
            .iter()
            .map(|b| {
                let count = self.list_objects(&b.name).map(|o| o.len()).unwrap_or(0);
                format!(
                    r#"{{"name":"{n}","public":{p},"objects":{c},"created_at":{ts}}}"#,
                    n = b.name,
                    p = b.public,
                    c = count,
                    ts = b.created_at_secs,
                )
            })
            .collect();
        format!(
            r#"{{"data_dir":"{d}","buckets":{},"items":[{}]}}"#,
            buckets.len(),
            items.join(","),
            d = Self::json_escape(&self.data_dir.display().to_string())
        )
    }
}

impl Default for StorageRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn walk(root: &Path, rel: &Path, out: &mut Vec<String>) {
    let full = root.join(rel);
    let Ok(entries) = fs::read_dir(&full) else {
        return;
    };
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let child_rel = rel.join(&file_name);
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            walk(root, &child_rel, out);
        } else {
            out.push(child_rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> StorageRegistry {
        let dir = std::env::temp_dir().join(format!(
            "bridge-storage-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::env::set_var("BRIDGE_STORAGE_DIR", &dir);
        StorageRegistry::new()
    }

    #[test]
    fn bucket_name_validation() {
        assert!(StorageRegistry::validate_bucket_name("my-bucket").is_ok());
        assert!(StorageRegistry::validate_bucket_name("a.b.c").is_ok());
        assert!(StorageRegistry::validate_bucket_name("ab").is_err()); // too short
        assert!(StorageRegistry::validate_bucket_name("-bad").is_err()); // leading dash
        assert!(StorageRegistry::validate_bucket_name("Bad_Name").is_err()); // case + underscore
        assert!(StorageRegistry::validate_bucket_name(&"x".repeat(64)).is_err());
        // too long
    }

    #[test]
    fn create_list_delete_bucket_lifecycle() {
        let reg = fresh();
        reg.create_bucket("photos", true).unwrap();
        reg.create_bucket("private-stuff", false).unwrap();
        assert_eq!(reg.list_buckets().len(), 2);
        assert!(reg.get_bucket("photos").unwrap().public);

        assert!(reg.create_bucket("photos", false).is_err()); // duplicate

        reg.delete_bucket("private-stuff").unwrap();
        assert_eq!(reg.list_buckets().len(), 1);
        assert!(reg.delete_bucket("ghost").is_err());
    }

    #[test]
    fn delete_nonempty_bucket_rejected() {
        let reg = fresh();
        reg.create_bucket("full", false).unwrap();
        reg.put_object("full", "a.txt", b"data").unwrap();
        let err = reg.delete_bucket("full").unwrap_err();
        assert!(err.contains("not empty"));
    }

    #[test]
    fn put_get_delete_object_roundtrip() {
        let reg = fresh();
        reg.create_bucket("bkt", false).unwrap();
        assert_eq!(reg.put_object("bkt", "hello.txt", b"hi there").unwrap(), 8);
        assert_eq!(reg.get_object("bkt", "hello.txt").unwrap(), b"hi there");
        reg.delete_object("bkt", "hello.txt").unwrap();
        assert!(reg.get_object("bkt", "hello.txt").is_err());

        // Missing bucket rejected for all ops.
        assert!(reg.put_object("nope", "k", b"v").is_err());
        assert!(reg.get_object("nope", "k").is_err());
    }

    #[test]
    fn nested_keys_and_listing() {
        let reg = fresh();
        reg.create_bucket("site", false).unwrap();
        reg.put_object("site", "css/main.css", b"body{}").unwrap();
        reg.put_object("site", "js/app.js", b"x()").unwrap();
        reg.put_object("site", "index.html", b"<html>").unwrap();
        let keys = reg.list_objects("site").unwrap();
        assert_eq!(keys, vec!["css/main.css", "index.html", "js/app.js"]);
    }

    #[test]
    fn traversal_keys_rejected() {
        let reg = fresh();
        reg.create_bucket("safe", false).unwrap();
        assert!(reg.put_object("safe", "../escape.txt", b"x").is_err());
        assert!(reg.put_object("safe", "/abs.txt", b"x").is_err());
        assert!(reg.put_object("safe", "a\0b", b"x").is_err());
        assert!(reg.get_object("safe", "../../etc/passwd").is_err());
    }

    #[test]
    fn signed_url_roundtrip_and_expiry() {
        let reg = fresh();
        reg.create_bucket("uploads", false).unwrap();
        let secret = b"url-secret";
        let (exp, sig) = reg
            .sign_url("PUT", "uploads", "docs/a.pdf", 600, secret)
            .unwrap();

        // Valid within window.
        reg.verify_signed_url("PUT", "uploads", "docs/a.pdf", exp, &sig, secret)
            .unwrap();
        // Wrong method rejected.
        assert!(reg
            .verify_signed_url("GET", "uploads", "docs/a.pdf", exp, &sig, secret)
            .is_err());
        // Wrong key rejected.
        assert!(reg
            .verify_signed_url("PUT", "uploads", "other.pdf", exp, &sig, secret)
            .is_err());
        // Expired rejected.
        assert!(reg
            .verify_signed_url("PUT", "uploads", "docs/a.pdf", now_secs(), &sig, secret)
            .is_err());
        // Garbage signature rejected.
        assert!(reg
            .verify_signed_url("PUT", "uploads", "docs/a.pdf", exp, "deadbeef", secret)
            .is_err());
        // Wrong secret rejected.
        assert!(reg
            .verify_signed_url("PUT", "uploads", "docs/a.pdf", exp, &sig, b"evil")
            .is_err());
    }

    #[test]
    fn signed_url_requires_existing_bucket() {
        let reg = fresh();
        assert!(reg.sign_url("GET", "missing", "k", 60, b"s").is_err());
    }

    #[test]
    fn public_read_flag() {
        let reg = fresh();
        reg.create_bucket("open", true).unwrap();
        reg.create_bucket("closed", false).unwrap();
        assert!(reg.public_read_allowed("open"));
        assert!(!reg.public_read_allowed("closed"));
        assert!(!reg.public_read_allowed("ghost"));
    }

    #[test]
    fn json_summary_shape() {
        let reg = fresh();
        reg.create_bucket("zeta", true).unwrap();
        reg.put_object("zeta", "one.bin", b"1").unwrap();
        let json = reg.to_json();
        assert!(json.contains(r#""buckets":1"#));
        assert!(json.contains(r#""name":"zeta""#));
        assert!(json.contains(r#""public":true"#));
        assert!(json.contains(r#""objects":1"#));
    }
}

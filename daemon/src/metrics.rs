//! Bridge Metrics — counters, gauges, histograms, Prometheus export.
//!
//! Inspired by Encore commits 1996 (runtimes-core Add metrics support)
//! and 1997 (runtimes-js add support for custom metrics).
//!
//! Zero external dependencies — all pure std.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A metric label set: `{service="hello", method="GET"}`.
pub type Labels = Vec<(String, String)>;

/// A single recorded sample.
#[derive(Debug, Clone)]
pub struct Sample {
    pub value:     f64,
    pub timestamp: u64, // unix seconds
    pub labels:    Labels,
}

/// All supported metric kinds.
#[derive(Debug, Clone)]
pub enum MetricValue {
    /// Monotonically increasing count (requests, errors…).
    Counter(f64),
    /// Arbitrary numeric value that can go up or down (goroutines, queue depth…).
    Gauge(f64),
    /// Distribution of values — stores sorted samples for percentile queries.
    Histogram(Vec<f64>),
}

/// A named metric with its samples.
#[derive(Debug, Clone)]
pub struct Metric {
    pub name:    String,
    pub help:    String,
    pub kind:    MetricKind,
    pub samples: Vec<Sample>,
}

/// The kind label used in Prometheus output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricKind {
    Counter,
    Gauge,
    Histogram,
}

impl MetricKind {
    pub fn as_str(self) -> &'static str {
        match self {
            MetricKind::Counter   => "counter",
            MetricKind::Gauge     => "gauge",
            MetricKind::Histogram => "histogram",
        }
    }
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Thread-safe metric registry shared across the daemon.
#[derive(Clone)]
pub struct Registry(Arc<Mutex<Inner>>);

struct Inner {
    metrics: HashMap<String, Metric>,
}

impl Registry {
    pub fn new() -> Self {
        Registry(Arc::new(Mutex::new(Inner {
            metrics: HashMap::new(),
        })))
    }

    // ── Write API ─────────────────────────────────────────────────────────

    /// Increment a counter by `delta` (default 1).
    pub fn counter_inc(&self, name: &str, help: &str, delta: f64, labels: Labels) {
        let mut inner = self.0.lock().unwrap();
        let m = inner.metrics.entry(name.to_string()).or_insert_with(|| Metric {
            name:    name.to_string(),
            help:    help.to_string(),
            kind:    MetricKind::Counter,
            samples: Vec::new(),
        });
        let prev = m.samples.last().map(|s| s.value).unwrap_or(0.0);
        m.samples.push(Sample { value: prev + delta, timestamp: now_secs(), labels });
    }

    /// Set a gauge to an absolute value.
    pub fn gauge_set(&self, name: &str, help: &str, value: f64, labels: Labels) {
        let mut inner = self.0.lock().unwrap();
        let m = inner.metrics.entry(name.to_string()).or_insert_with(|| Metric {
            name:    name.to_string(),
            help:    help.to_string(),
            kind:    MetricKind::Gauge,
            samples: Vec::new(),
        });
        m.samples.push(Sample { value, timestamp: now_secs(), labels });
    }

    /// Record a histogram observation.
    pub fn histogram_observe(&self, name: &str, help: &str, value: f64, labels: Labels) {
        let mut inner = self.0.lock().unwrap();
        let m = inner.metrics.entry(name.to_string()).or_insert_with(|| Metric {
            name:    name.to_string(),
            help:    help.to_string(),
            kind:    MetricKind::Histogram,
            samples: Vec::new(),
        });
        m.samples.push(Sample { value, timestamp: now_secs(), labels });
    }

    // ── Read API ──────────────────────────────────────────────────────────

    /// Get the latest value for a metric name. Returns `None` if not found.
    pub fn latest(&self, name: &str) -> Option<f64> {
        let inner = self.0.lock().unwrap();
        inner.metrics.get(name)?.samples.last().map(|s| s.value)
    }

    /// Return all metrics as a JSON summary string.
    pub fn to_json(&self) -> String {
        let inner = self.0.lock().unwrap();
        let mut parts = Vec::new();
        for m in inner.metrics.values() {
            let last = m.samples.last();
            let value = last.map(|s| s.value).unwrap_or(0.0);
            let ts    = last.map(|s| s.timestamp).unwrap_or(0);
            parts.push(format!(
                r#"{{"name":"{name}","kind":"{kind}","help":"{help}","value":{value},"timestamp":{ts},"samples":{count}}}"#,
                name  = m.name,
                kind  = m.kind.as_str(),
                help  = m.help,
                value = value,
                ts    = ts,
                count = m.samples.len(),
            ));
        }
        format!("[{}]", parts.join(","))
    }

    /// Prometheus text-format exposition.
    /// Compatible with Prometheus scrape endpoint (`/metrics`).
    pub fn to_prometheus(&self) -> String {
        let inner = self.0.lock().unwrap();
        let mut out = String::new();
        for m in inner.metrics.values() {
            out.push_str(&format!("# HELP {} {}\n", m.name, m.help));
            out.push_str(&format!("# TYPE {} {}\n", m.name, m.kind.as_str()));
            if let Some(s) = m.samples.last() {
                let labels = format_labels(&s.labels);
                match m.kind {
                    MetricKind::Counter | MetricKind::Gauge => {
                        out.push_str(&format!(
                            "{}{} {} {}\n",
                            m.name, labels, s.value, s.timestamp * 1000
                        ));
                    }
                    MetricKind::Histogram => {
                        // Emit count + sum + p50/p90/p99 buckets
                        let mut values: Vec<f64> = m.samples.iter().map(|s| s.value).collect();
                        values.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        let count = values.len() as f64;
                        let sum: f64 = values.iter().sum();
                        let p50 = percentile(&values, 0.50);
                        let p90 = percentile(&values, 0.90);
                        let p99 = percentile(&values, 0.99);
                        out.push_str(&format!("{}_bucket{{le=\"0.5\"{sep}{lb}}} {p50} {ts}\n",  m.name, sep = if s.labels.is_empty() { "" } else { "," }, lb = format_labels_inner(&s.labels), p50 = p50, ts = s.timestamp * 1000));
                        out.push_str(&format!("{}_bucket{{le=\"0.9\"{sep}{lb}}} {p90} {ts}\n",  m.name, sep = if s.labels.is_empty() { "" } else { "," }, lb = format_labels_inner(&s.labels), p90 = p90, ts = s.timestamp * 1000));
                        out.push_str(&format!("{}_bucket{{le=\"0.99\"{sep}{lb}}} {p99} {ts}\n", m.name, sep = if s.labels.is_empty() { "" } else { "," }, lb = format_labels_inner(&s.labels), p99 = p99, ts = s.timestamp * 1000));
                        out.push_str(&format!("{}_count{lb} {count} {ts}\n", m.name, lb = labels, count = count, ts = s.timestamp * 1000));
                        out.push_str(&format!("{}_sum{lb} {sum} {ts}\n",     m.name, lb = labels, sum = sum,     ts = s.timestamp * 1000));
                    }
                }
            }
        }
        out
    }

    /// Clear all metrics.
    pub fn clear(&self) {
        self.0.lock().unwrap().metrics.clear();
    }

    /// Total number of registered metrics.
    pub fn len(&self) -> usize {
        self.0.lock().unwrap().metrics.len()
    }

    pub fn is_empty(&self) -> bool { self.len() == 0 }
}

impl Default for Registry {
    fn default() -> Self { Self::new() }
}

// ── Built-in daemon metrics ───────────────────────────────────────────────────

/// Seed the registry with standard Bridge daemon metrics.
pub fn register_defaults(reg: &Registry) {
    reg.gauge_set("bridge_up", "Whether the Bridge daemon is running (1=up)", 1.0, vec![]);
    reg.counter_inc(
        "bridge_requests_total",
        "Total number of requests handled by the daemon",
        0.0, vec![],
    );
    reg.gauge_set(
        "bridge_goroutines",
        "Current number of active handler threads",
        0.0, vec![],
    );
    reg.histogram_observe(
        "bridge_request_duration_seconds",
        "Request duration in seconds",
        0.0, vec![],
    );
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() { return 0.0; }
    // Use ceiling-based nearest rank: ceil(p * n) then clamp
    let rank = (p * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

fn format_labels(labels: &[(String, String)]) -> String {
    if labels.is_empty() { return String::new(); }
    format!("{{{}}}", format_labels_inner(labels))
}

fn format_labels_inner(labels: &[(String, String)]) -> String {
    labels.iter()
        .map(|(k, v)| format!("{}=\"{}\"", k, v))
        .collect::<Vec<_>>()
        .join(",")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_increments() {
        let reg = Registry::new();
        reg.counter_inc("req", "Requests", 1.0, vec![]);
        reg.counter_inc("req", "Requests", 1.0, vec![]);
        reg.counter_inc("req", "Requests", 1.0, vec![]);
        assert_eq!(reg.latest("req"), Some(3.0));
    }

    #[test]
    fn gauge_set_overrides() {
        let reg = Registry::new();
        reg.gauge_set("cpu", "CPU usage", 0.5, vec![]);
        reg.gauge_set("cpu", "CPU usage", 0.8, vec![]);
        assert_eq!(reg.latest("cpu"), Some(0.8));
    }

    #[test]
    fn histogram_records_samples() {
        let reg = Registry::new();
        for v in [10.0, 20.0, 30.0, 40.0, 50.0] {
            reg.histogram_observe("latency", "Request latency ms", v, vec![]);
        }
        let json = reg.to_json();
        assert!(json.contains("\"name\":\"latency\""));
        assert!(json.contains("\"kind\":\"histogram\""));
        assert!(json.contains("\"samples\":5"));
    }

    #[test]
    fn prometheus_output_contains_help() {
        let reg = Registry::new();
        reg.counter_inc("http_reqs", "HTTP requests", 5.0, vec![
            ("method".into(), "GET".into()),
            ("status".into(), "200".into()),
        ]);
        let prom = reg.to_prometheus();
        assert!(prom.contains("# HELP http_reqs"));
        assert!(prom.contains("# TYPE http_reqs counter"));
        assert!(prom.contains("method=\"GET\""));
    }

    #[test]
    fn json_output_well_formed() {
        let reg = Registry::new();
        register_defaults(&reg);
        let json = reg.to_json();
        assert!(json.starts_with('['));
        assert!(json.ends_with(']'));
        assert!(json.contains("bridge_up"));
    }

    #[test]
    fn clear_empties_registry() {
        let reg = Registry::new();
        reg.counter_inc("x", "x", 1.0, vec![]);
        assert_eq!(reg.len(), 1);
        reg.clear();
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn percentile_works() {
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        assert_eq!(percentile(&data, 0.5), 5.0);
        assert_eq!(percentile(&data, 0.99), 10.0);
    }

    #[test]
    fn labels_format_empty() {
        assert_eq!(format_labels(&[]), "");
    }

    #[test]
    fn labels_format_with_values() {
        let labels = vec![("env".into(), "prod".into())];
        assert_eq!(format_labels(&labels), "{env=\"prod\"}");
    }
}

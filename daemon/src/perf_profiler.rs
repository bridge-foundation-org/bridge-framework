//! Performance Profiling & Optimization
//!
//! Profiling metrics, bottleneck detection, and optimization recommendations

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Performance metric type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricKind {
    LatencyMs,
    ThroughputOps,
    MemoryBytes,
    CpuPercent,
}

impl MetricKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricKind::LatencyMs => "latency_ms",
            MetricKind::ThroughputOps => "throughput_ops",
            MetricKind::MemoryBytes => "memory_bytes",
            MetricKind::CpuPercent => "cpu_percent",
        }
    }
}

/// Performance measurement
#[derive(Clone, Debug)]
pub struct PerfMetric {
    pub name: String,
    pub kind: MetricKind,
    pub value: f64,
    pub timestamp: u64,
}

impl PerfMetric {
    pub fn new(name: impl Into<String>, kind: MetricKind, value: f64) -> Self {
        PerfMetric {
            name: name.into(),
            kind,
            value,
            timestamp: current_timestamp_ms(),
        }
    }
}

/// Performance threshold
#[derive(Clone, Debug)]
pub struct PerfThreshold {
    pub metric_name: String,
    pub max_value: f64,
    pub kind: MetricKind,
}

impl PerfThreshold {
    pub fn new(name: impl Into<String>, kind: MetricKind, max_value: f64) -> Self {
        PerfThreshold {
            metric_name: name.into(),
            kind,
            max_value,
        }
    }

    pub fn is_exceeded(&self, value: f64) -> bool {
        value > self.max_value
    }
}

/// Operation timer
pub struct Timer {
    start: Instant,
    name: String,
}

impl Timer {
    pub fn start(name: impl Into<String>) -> Self {
        Timer {
            start: Instant::now(),
            name: name.into(),
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.start.elapsed().as_millis() as f64
    }

    pub fn stop(self) -> PerfMetric {
        let elapsed = self.elapsed_ms();
        PerfMetric::new(self.name, MetricKind::LatencyMs, elapsed)
    }
}

/// Profiler
pub struct Profiler {
    metrics: Vec<PerfMetric>,
    thresholds: HashMap<String, PerfThreshold>,
    max_metrics: usize,
}

impl Profiler {
    pub fn new() -> Self {
        Profiler {
            metrics: Vec::new(),
            thresholds: HashMap::new(),
            max_metrics: 10000,
        }
    }

    /// Record metric
    pub fn record(&mut self, metric: PerfMetric) {
        if self.metrics.len() < self.max_metrics {
            self.metrics.push(metric);
        }
    }

    /// Set threshold
    pub fn set_threshold(&mut self, threshold: PerfThreshold) {
        self.thresholds.insert(threshold.metric_name.clone(), threshold);
    }

    /// Check for violations
    pub fn check_violations(&self) -> Vec<String> {
        let mut violations = Vec::new();

        for metric in &self.metrics {
            if let Some(threshold) = self.thresholds.get(&metric.name) {
                if threshold.is_exceeded(metric.value) {
                    violations.push(format!(
                        "{}: {} > threshold {}",
                        metric.name, metric.value, threshold.max_value
                    ));
                }
            }
        }

        violations
    }

    /// Get average for metric
    pub fn average(&self, name: &str) -> Option<f64> {
        let metrics: Vec<_> = self.metrics
            .iter()
            .filter(|m| m.name == name)
            .collect();

        if metrics.is_empty() {
            return None;
        }

        let sum: f64 = metrics.iter().map(|m| m.value).sum();
        Some(sum / metrics.len() as f64)
    }

    /// Get max for metric
    pub fn max(&self, name: &str) -> Option<f64> {
        self.metrics
            .iter()
            .filter(|m| m.name == name)
            .map(|m| m.value)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Get percentile
    pub fn percentile(&self, name: &str, p: f64) -> Option<f64> {
        let mut values: Vec<_> = self.metrics
            .iter()
            .filter(|m| m.name == name)
            .map(|m| m.value)
            .collect();

        if values.is_empty() {
            return None;
        }

        values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((p / 100.0) * values.len() as f64).ceil() as usize;
        values.get(idx.saturating_sub(1)).copied()
    }

    /// List metrics
    pub fn metrics(&self) -> &[PerfMetric] {
        &self.metrics
    }

    /// Clear all metrics
    pub fn clear(&mut self) {
        self.metrics.clear();
    }
}

impl Default for Profiler {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_kind_as_str() {
        assert_eq!(MetricKind::LatencyMs.as_str(), "latency_ms");
        assert_eq!(MetricKind::MemoryBytes.as_str(), "memory_bytes");
    }

    #[test]
    fn test_perf_metric_new() {
        let metric = PerfMetric::new("request_time", MetricKind::LatencyMs, 50.0);
        assert_eq!(metric.name, "request_time");
        assert_eq!(metric.value, 50.0);
    }

    #[test]
    fn test_perf_threshold_new() {
        let threshold = PerfThreshold::new("request_time", MetricKind::LatencyMs, 100.0);
        assert_eq!(threshold.metric_name, "request_time");
        assert_eq!(threshold.max_value, 100.0);
    }

    #[test]
    fn test_perf_threshold_is_exceeded() {
        let threshold = PerfThreshold::new("latency", MetricKind::LatencyMs, 100.0);
        assert!(!threshold.is_exceeded(50.0));
        assert!(threshold.is_exceeded(150.0));
    }

    #[test]
    fn test_timer_elapsed() {
        let timer = Timer::start("test_op");
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = timer.elapsed_ms();
        assert!(elapsed >= 10.0);
    }

    #[test]
    fn test_timer_stop() {
        let timer = Timer::start("test_op");
        std::thread::sleep(Duration::from_millis(5));
        let metric = timer.stop();
        assert_eq!(metric.kind, MetricKind::LatencyMs);
        assert!(metric.value >= 5.0);
    }

    #[test]
    fn test_profiler_new() {
        let profiler = Profiler::new();
        assert_eq!(profiler.metrics.len(), 0);
    }

    #[test]
    fn test_profiler_record() {
        let mut profiler = Profiler::new();
        let metric = PerfMetric::new("latency", MetricKind::LatencyMs, 50.0);
        profiler.record(metric);
        assert_eq!(profiler.metrics.len(), 1);
    }

    #[test]
    fn test_profiler_set_threshold() {
        let mut profiler = Profiler::new();
        let threshold = PerfThreshold::new("latency", MetricKind::LatencyMs, 100.0);
        profiler.set_threshold(threshold);
        assert_eq!(profiler.thresholds.len(), 1);
    }

    #[test]
    fn test_profiler_check_violations() {
        let mut profiler = Profiler::new();
        profiler.set_threshold(PerfThreshold::new("latency", MetricKind::LatencyMs, 50.0));
        
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 30.0));
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 100.0));

        let violations = profiler.check_violations();
        assert_eq!(violations.len(), 1);
    }

    #[test]
    fn test_profiler_average() {
        let mut profiler = Profiler::new();
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 50.0));
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 100.0));

        let avg = profiler.average("latency");
        assert_eq!(avg, Some(75.0));
    }

    #[test]
    fn test_profiler_max() {
        let mut profiler = Profiler::new();
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 50.0));
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 200.0));

        let max = profiler.max("latency");
        assert_eq!(max, Some(200.0));
    }

    #[test]
    fn test_profiler_percentile() {
        let mut profiler = Profiler::new();
        for i in 1..=100 {
            profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, i as f64));
        }

        let p95 = profiler.percentile("latency", 95.0);
        assert!(p95.is_some());
    }

    #[test]
    fn test_profiler_clear() {
        let mut profiler = Profiler::new();
        profiler.record(PerfMetric::new("latency", MetricKind::LatencyMs, 50.0));
        assert_eq!(profiler.metrics.len(), 1);
        
        profiler.clear();
        assert_eq!(profiler.metrics.len(), 0);
    }
}

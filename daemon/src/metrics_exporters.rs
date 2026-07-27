//! Cloud metrics exporters - Multi-provider metrics export
//!
//! Export metrics to AWS CloudWatch, GCP Stackdriver, Azure Monitor, and Prometheus

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

/// Metric data type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetricType {
    Counter,
    Gauge,
    Histogram,
    Summary,
}

impl MetricType {
    pub fn as_str(&self) -> &'static str {
        match self {
            MetricType::Counter => "counter",
            MetricType::Gauge => "gauge",
            MetricType::Histogram => "histogram",
            MetricType::Summary => "summary",
        }
    }
}

/// Metric dimension (label/tag)
#[derive(Clone, Debug)]
pub struct MetricDimension {
    pub name: String,
    pub value: String,
}

impl MetricDimension {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        MetricDimension {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// Metric data point
#[derive(Clone, Debug)]
pub struct MetricDataPoint {
    pub name: String,
    pub metric_type: MetricType,
    pub value: f64,
    pub timestamp: u64,
    pub dimensions: Vec<MetricDimension>,
    pub unit: String,
}

impl MetricDataPoint {
    pub fn new(name: impl Into<String>, value: f64, metric_type: MetricType) -> Self {
        MetricDataPoint {
            name: name.into(),
            metric_type,
            value,
            timestamp: current_timestamp_ms(),
            dimensions: Vec::new(),
            unit: "Count".to_string(),
        }
    }

    pub fn add_dimension(mut self, dim: MetricDimension) -> Self {
        self.dimensions.push(dim);
        self
    }

    pub fn with_unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = unit.into();
        self
    }
}

/// Cloud provider for metrics
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloudProvider {
    CloudWatch,
    Stackdriver,
    AzureMonitor,
    Prometheus,
}

impl CloudProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            CloudProvider::CloudWatch => "cloudwatch",
            CloudProvider::Stackdriver => "stackdriver",
            CloudProvider::AzureMonitor => "azure-monitor",
            CloudProvider::Prometheus => "prometheus",
        }
    }
}

/// Metrics exporter trait
pub trait MetricsExporter: Send + Sync {
    /// Export a batch of metrics
    fn export(&self, metrics: &[MetricDataPoint]) -> Result<(), String>;

    /// Get provider name
    fn provider(&self) -> CloudProvider;

    /// Test connection
    fn health_check(&self) -> Result<(), String>;
}

/// AWS CloudWatch exporter
pub struct CloudWatchExporter {
    namespace: String,
    region: String,
}

impl CloudWatchExporter {
    pub fn new(namespace: impl Into<String>, region: impl Into<String>) -> Self {
        CloudWatchExporter {
            namespace: namespace.into(),
            region: region.into(),
        }
    }
}

impl MetricsExporter for CloudWatchExporter {
    fn export(&self, metrics: &[MetricDataPoint]) -> Result<(), String> {
        if metrics.is_empty() {
            return Ok(());
        }

        // In production, this would call AWS API
        // For now, validate the format
        for metric in metrics {
            if metric.name.is_empty() {
                return Err("Metric name cannot be empty".to_string());
            }
        }

        Ok(())
    }

    fn provider(&self) -> CloudProvider {
        CloudProvider::CloudWatch
    }

    fn health_check(&self) -> Result<(), String> {
        if self.region.is_empty() {
            return Err("Region not configured".to_string());
        }
        Ok(())
    }
}

/// GCP Stackdriver exporter
pub struct StackdriverExporter {
    project_id: String,
}

impl StackdriverExporter {
    pub fn new(project_id: impl Into<String>) -> Self {
        StackdriverExporter {
            project_id: project_id.into(),
        }
    }
}

impl MetricsExporter for StackdriverExporter {
    fn export(&self, metrics: &[MetricDataPoint]) -> Result<(), String> {
        if metrics.is_empty() {
            return Ok(());
        }

        // In production, this would call GCP API
        for metric in metrics {
            if metric.name.is_empty() {
                return Err("Metric name cannot be empty".to_string());
            }
        }

        Ok(())
    }

    fn provider(&self) -> CloudProvider {
        CloudProvider::Stackdriver
    }

    fn health_check(&self) -> Result<(), String> {
        if self.project_id.is_empty() {
            return Err("Project ID not configured".to_string());
        }
        Ok(())
    }
}

/// Azure Monitor exporter
pub struct AzureMonitorExporter {
    resource_id: String,
}

impl AzureMonitorExporter {
    pub fn new(resource_id: impl Into<String>) -> Self {
        AzureMonitorExporter {
            resource_id: resource_id.into(),
        }
    }
}

impl MetricsExporter for AzureMonitorExporter {
    fn export(&self, metrics: &[MetricDataPoint]) -> Result<(), String> {
        if metrics.is_empty() {
            return Ok(());
        }

        for metric in metrics {
            if metric.name.is_empty() {
                return Err("Metric name cannot be empty".to_string());
            }
        }

        Ok(())
    }

    fn provider(&self) -> CloudProvider {
        CloudProvider::AzureMonitor
    }

    fn health_check(&self) -> Result<(), String> {
        if self.resource_id.is_empty() {
            return Err("Resource ID not configured".to_string());
        }
        Ok(())
    }
}

/// Prometheus exporter (text format)
pub struct PrometheusExporter {
    namespace: String,
}

impl PrometheusExporter {
    pub fn new(namespace: impl Into<String>) -> Self {
        PrometheusExporter {
            namespace: namespace.into(),
        }
    }

    /// Generate Prometheus format text
    pub fn format_prometheus(&self, metrics: &[MetricDataPoint]) -> String {
        let mut result = String::new();

        for metric in metrics {
            // Add HELP line
            result.push_str(&format!(
                "# HELP {}_{} {}\n",
                self.namespace, metric.name, metric.metric_type.as_str()
            ));

            // Add TYPE line
            result.push_str(&format!(
                "# TYPE {}_{} {}\n",
                self.namespace, metric.name, metric.metric_type.as_str()
            ));

            // Add metric line with labels
            let labels = if metric.dimensions.is_empty() {
                String::new()
            } else {
                let label_strs: Vec<_> = metric
                    .dimensions
                    .iter()
                    .map(|d| format!("{}=\"{}\"", d.name, d.value))
                    .collect();
                format!("{{{}}}", label_strs.join(","))
            };

            result.push_str(&format!(
                "{}_{}{} {} {}\n",
                self.namespace, metric.name, labels, metric.value, metric.timestamp
            ));
        }

        result
    }
}

impl MetricsExporter for PrometheusExporter {
    fn export(&self, metrics: &[MetricDataPoint]) -> Result<(), String> {
        if metrics.is_empty() {
            return Ok(());
        }

        let _prometheus_text = self.format_prometheus(metrics);
        // In production, this would write to /metrics endpoint
        Ok(())
    }

    fn provider(&self) -> CloudProvider {
        CloudProvider::Prometheus
    }

    fn health_check(&self) -> Result<(), String> {
        if self.namespace.is_empty() {
            return Err("Namespace not configured".to_string());
        }
        Ok(())
    }
}

/// Metrics collector registry
pub struct MetricsRegistry {
    exporters: HashMap<String, Box<dyn MetricsExporter>>,
    buffered_metrics: Vec<MetricDataPoint>,
    max_buffer_size: usize,
}

impl MetricsRegistry {
    pub fn new() -> Self {
        MetricsRegistry {
            exporters: HashMap::new(),
            buffered_metrics: Vec::new(),
            max_buffer_size: 1000,
        }
    }

    /// Register an exporter
    pub fn register_exporter(&mut self, name: impl Into<String>, exporter: Box<dyn MetricsExporter>) {
        self.exporters.insert(name.into(), exporter);
    }

    /// Record a metric
    pub fn record_metric(&mut self, metric: MetricDataPoint) {
        if self.buffered_metrics.len() < self.max_buffer_size {
            self.buffered_metrics.push(metric);
        }
    }

    /// Export all buffered metrics
    pub fn export_all(&mut self) -> Result<(), String> {
        if self.buffered_metrics.is_empty() {
            return Ok(());
        }

        let mut errors = Vec::new();

        for (name, exporter) in self.exporters.iter() {
            if let Err(e) = exporter.export(&self.buffered_metrics) {
                errors.push(format!("{}: {}", name, e));
            }
        }

        self.buffered_metrics.clear();

        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!("Export errors: {}", errors.join("; ")))
        }
    }

    /// Health check all exporters
    pub fn health_check_all(&self) -> HashMap<String, Result<(), String>> {
        let mut results = HashMap::new();
        for (name, exporter) in self.exporters.iter() {
            results.insert(name.clone(), exporter.health_check());
        }
        results
    }
}

impl Default for MetricsRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn current_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metric_type_as_str() {
        assert_eq!(MetricType::Counter.as_str(), "counter");
        assert_eq!(MetricType::Gauge.as_str(), "gauge");
    }

    #[test]
    fn test_metric_dimension_new() {
        let dim = MetricDimension::new("service", "api");
        assert_eq!(dim.name, "service");
        assert_eq!(dim.value, "api");
    }

    #[test]
    fn test_metric_datapoint_new() {
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        assert_eq!(metric.name, "requests");
        assert_eq!(metric.value, 100.0);
        assert_eq!(metric.metric_type, MetricType::Counter);
    }

    #[test]
    fn test_metric_datapoint_with_dimension() {
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter)
            .add_dimension(MetricDimension::new("service", "api"));
        assert_eq!(metric.dimensions.len(), 1);
    }

    #[test]
    fn test_metric_datapoint_with_unit() {
        let metric = MetricDataPoint::new("latency", 50.0, MetricType::Gauge)
            .with_unit("Milliseconds");
        assert_eq!(metric.unit, "Milliseconds");
    }

    #[test]
    fn test_cloud_provider_as_str() {
        assert_eq!(CloudProvider::CloudWatch.as_str(), "cloudwatch");
        assert_eq!(CloudProvider::Stackdriver.as_str(), "stackdriver");
        assert_eq!(CloudProvider::AzureMonitor.as_str(), "azure-monitor");
    }

    #[test]
    fn test_cloudwatch_exporter_new() {
        let exporter = CloudWatchExporter::new("bridge", "us-east-1");
        assert_eq!(exporter.namespace, "bridge");
    }

    #[test]
    fn test_cloudwatch_exporter_export_empty() {
        let exporter = CloudWatchExporter::new("bridge", "us-east-1");
        let result = exporter.export(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cloudwatch_exporter_export_valid() {
        let exporter = CloudWatchExporter::new("bridge", "us-east-1");
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        let result = exporter.export(&[metric]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_cloudwatch_exporter_health_check() {
        let exporter = CloudWatchExporter::new("bridge", "us-east-1");
        assert!(exporter.health_check().is_ok());
    }

    #[test]
    fn test_stackdriver_exporter_new() {
        let exporter = StackdriverExporter::new("my-project");
        assert_eq!(exporter.project_id, "my-project");
    }

    #[test]
    fn test_stackdriver_exporter_export() {
        let exporter = StackdriverExporter::new("my-project");
        let metric = MetricDataPoint::new("errors", 5.0, MetricType::Counter);
        let result = exporter.export(&[metric]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_azure_exporter_new() {
        let exporter = AzureMonitorExporter::new("/subscriptions/sub123");
        assert_eq!(exporter.resource_id, "/subscriptions/sub123");
    }

    #[test]
    fn test_prometheus_exporter_new() {
        let exporter = PrometheusExporter::new("bridge");
        assert_eq!(exporter.namespace, "bridge");
    }

    #[test]
    fn test_prometheus_exporter_format() {
        let exporter = PrometheusExporter::new("bridge");
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter)
            .add_dimension(MetricDimension::new("service", "api"));

        let formatted = exporter.format_prometheus(&[metric]);
        assert!(formatted.contains("bridge_requests"));
        assert!(formatted.contains("service=\"api\""));
    }

    #[test]
    fn test_prometheus_exporter_export() {
        let exporter = PrometheusExporter::new("bridge");
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        let result = exporter.export(&[metric]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_registry_new() {
        let registry = MetricsRegistry::new();
        assert_eq!(registry.exporters.len(), 0);
    }

    #[test]
    fn test_metrics_registry_register() {
        let mut registry = MetricsRegistry::new();
        let exporter = Box::new(PrometheusExporter::new("bridge"));
        registry.register_exporter("prometheus", exporter);
        assert_eq!(registry.exporters.len(), 1);
    }

    #[test]
    fn test_metrics_registry_record_metric() {
        let mut registry = MetricsRegistry::new();
        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        registry.record_metric(metric);
        assert_eq!(registry.buffered_metrics.len(), 1);
    }

    #[test]
    fn test_metrics_registry_export_empty() {
        let mut registry = MetricsRegistry::new();
        let result = registry.export_all();
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_registry_export_with_exporter() {
        let mut registry = MetricsRegistry::new();
        let exporter = Box::new(PrometheusExporter::new("bridge"));
        registry.register_exporter("prometheus", exporter);

        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        registry.record_metric(metric);

        let result = registry.export_all();
        assert!(result.is_ok());
        assert_eq!(registry.buffered_metrics.len(), 0);
    }

    #[test]
    fn test_metrics_registry_health_check() {
        let mut registry = MetricsRegistry::new();
        let exporter = Box::new(PrometheusExporter::new("bridge"));
        registry.register_exporter("prometheus", exporter);

        let results = registry.health_check_all();
        assert_eq!(results.len(), 1);
        assert!(results.get("prometheus").unwrap().is_ok());
    }

    #[test]
    fn test_metrics_registry_multiple_exporters() {
        let mut registry = MetricsRegistry::new();
        registry.register_exporter("prometheus", Box::new(PrometheusExporter::new("bridge")));
        registry.register_exporter("cloudwatch", Box::new(CloudWatchExporter::new("bridge", "us-east-1")));

        let metric = MetricDataPoint::new("requests", 100.0, MetricType::Counter);
        registry.record_metric(metric);

        let result = registry.export_all();
        assert!(result.is_ok());
    }
}

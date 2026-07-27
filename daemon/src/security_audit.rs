//! Security Audit & Hardening
//!
//! Security checks, vulnerability scanning, and compliance validation

use std::collections::HashMap;

/// Vulnerability severity
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn as_str(&self) -> &'static str {
        match self {
            Severity::Info => "info",
            Severity::Low => "low",
            Severity::Medium => "medium",
            Severity::High => "high",
            Severity::Critical => "critical",
        }
    }

    pub fn score(&self) -> u8 {
        match self {
            Severity::Info => 1,
            Severity::Low => 2,
            Severity::Medium => 5,
            Severity::High => 8,
            Severity::Critical => 10,
        }
    }
}

/// Security check type
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckType {
    AuthN,
    AuthZ,
    Encryption,
    InputValidation,
    RateLimit,
    Audit,
    Transport,
}

impl CheckType {
    pub fn as_str(&self) -> &'static str {
        match self {
            CheckType::AuthN => "authentication",
            CheckType::AuthZ => "authorization",
            CheckType::Encryption => "encryption",
            CheckType::InputValidation => "input_validation",
            CheckType::RateLimit => "rate_limiting",
            CheckType::Audit => "audit_logging",
            CheckType::Transport => "transport_security",
        }
    }
}

/// Security vulnerability
#[derive(Clone, Debug)]
pub struct Vulnerability {
    pub id: String,
    pub check_type: CheckType,
    pub severity: Severity,
    pub description: String,
    pub remediation: String,
}

impl Vulnerability {
    pub fn new(
        id: impl Into<String>,
        check_type: CheckType,
        severity: Severity,
        description: impl Into<String>,
    ) -> Self {
        Vulnerability {
            id: id.into(),
            check_type,
            severity,
            description: description.into(),
            remediation: String::new(),
        }
    }

    pub fn with_remediation(mut self, remediation: impl Into<String>) -> Self {
        self.remediation = remediation.into();
        self
    }
}

/// Security audit result
#[derive(Clone, Debug)]
pub struct AuditResult {
    pub passed: bool,
    pub check_type: CheckType,
    pub message: String,
}

impl AuditResult {
    pub fn pass(check_type: CheckType, message: impl Into<String>) -> Self {
        AuditResult {
            passed: true,
            check_type,
            message: message.into(),
        }
    }

    pub fn fail(check_type: CheckType, message: impl Into<String>) -> Self {
        AuditResult {
            passed: false,
            check_type,
            message: message.into(),
        }
    }
}

/// Security compliance checker
pub struct SecurityAuditor {
    vulnerabilities: Vec<Vulnerability>,
    checks: HashMap<CheckType, Box<dyn Fn() -> bool + Send + Sync>>,
}

impl SecurityAuditor {
    pub fn new() -> Self {
        SecurityAuditor {
            vulnerabilities: Vec::new(),
            checks: HashMap::new(),
        }
    }

    /// Report vulnerability
    pub fn report_vulnerability(&mut self, vuln: Vulnerability) {
        self.vulnerabilities.push(vuln);
    }

    /// Get vulnerabilities by severity
    pub fn get_by_severity(&self, severity: Severity) -> Vec<&Vulnerability> {
        self.vulnerabilities
            .iter()
            .filter(|v| v.severity == severity)
            .collect()
    }

    /// Get critical vulnerabilities
    pub fn get_critical(&self) -> Vec<&Vulnerability> {
        self.get_by_severity(Severity::Critical)
    }

    /// Count vulnerabilities
    pub fn count(&self) -> usize {
        self.vulnerabilities.len()
    }

    /// Count by severity
    pub fn count_by_severity(&self, severity: Severity) -> usize {
        self.vulnerabilities
            .iter()
            .filter(|v| v.severity == severity)
            .count()
    }

    /// Risk score (0-100)
    pub fn risk_score(&self) -> u16 {
        let total_score: u16 = self.vulnerabilities
            .iter()
            .map(|v| v.severity.score() as u16)
            .sum();

        std::cmp::min(100, (total_score / self.vulnerabilities.len().max(1) as u16) * 10)
    }

    /// Is compliant (no critical/high)
    pub fn is_compliant(&self) -> bool {
        self.vulnerabilities
            .iter()
            .all(|v| v.severity < Severity::High)
    }
}

impl Default for SecurityAuditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_severity_as_str() {
        assert_eq!(Severity::Critical.as_str(), "critical");
        assert_eq!(Severity::Medium.as_str(), "medium");
    }

    #[test]
    fn test_severity_score() {
        assert_eq!(Severity::Info.score(), 1);
        assert_eq!(Severity::Critical.score(), 10);
    }

    #[test]
    fn test_severity_ordering() {
        assert!(Severity::Low < Severity::Medium);
        assert!(Severity::Medium < Severity::Critical);
    }

    #[test]
    fn test_check_type_as_str() {
        assert_eq!(CheckType::AuthN.as_str(), "authentication");
        assert_eq!(CheckType::Encryption.as_str(), "encryption");
    }

    #[test]
    fn test_vulnerability_new() {
        let vuln = Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::High,
            "Missing authentication check",
        );
        assert_eq!(vuln.id, "CVE-001");
        assert_eq!(vuln.severity, Severity::High);
    }

    #[test]
    fn test_vulnerability_with_remediation() {
        let vuln = Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::High,
            "Missing auth",
        )
        .with_remediation("Add JWT validation");

        assert_eq!(vuln.remediation, "Add JWT validation");
    }

    #[test]
    fn test_audit_result_pass() {
        let result = AuditResult::pass(CheckType::Encryption, "TLS enabled");
        assert!(result.passed);
        assert_eq!(result.check_type, CheckType::Encryption);
    }

    #[test]
    fn test_audit_result_fail() {
        let result = AuditResult::fail(CheckType::AuthZ, "No RBAC implemented");
        assert!(!result.passed);
    }

    #[test]
    fn test_security_auditor_new() {
        let auditor = SecurityAuditor::new();
        assert_eq!(auditor.count(), 0);
    }

    #[test]
    fn test_security_auditor_report() {
        let mut auditor = SecurityAuditor::new();
        let vuln = Vulnerability::new("CVE-001", CheckType::AuthN, Severity::High, "Test");
        auditor.report_vulnerability(vuln);
        assert_eq!(auditor.count(), 1);
    }

    #[test]
    fn test_security_auditor_get_by_severity() {
        let mut auditor = SecurityAuditor::new();
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::Critical,
            "Critical",
        ));
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-002",
            CheckType::AuthZ,
            Severity::Medium,
            "Medium",
        ));

        let critical = auditor.get_by_severity(Severity::Critical);
        assert_eq!(critical.len(), 1);
    }

    #[test]
    fn test_security_auditor_count_by_severity() {
        let mut auditor = SecurityAuditor::new();
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::High,
            "High",
        ));
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-002",
            CheckType::AuthZ,
            Severity::High,
            "High",
        ));

        assert_eq!(auditor.count_by_severity(Severity::High), 2);
    }

    #[test]
    fn test_security_auditor_risk_score() {
        let mut auditor = SecurityAuditor::new();
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::Critical,
            "Critical",
        ));

        let score = auditor.risk_score();
        assert!(score > 0);
    }

    #[test]
    fn test_security_auditor_is_compliant() {
        let mut auditor = SecurityAuditor::new();
        assert!(auditor.is_compliant());

        auditor.report_vulnerability(Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::Medium,
            "Medium",
        ));
        assert!(auditor.is_compliant());

        auditor.report_vulnerability(Vulnerability::new(
            "CVE-002",
            CheckType::AuthZ,
            Severity::Critical,
            "Critical",
        ));
        assert!(!auditor.is_compliant());
    }

    #[test]
    fn test_security_auditor_get_critical() {
        let mut auditor = SecurityAuditor::new();
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-001",
            CheckType::AuthN,
            Severity::Critical,
            "Critical",
        ));
        auditor.report_vulnerability(Vulnerability::new(
            "CVE-002",
            CheckType::AuthZ,
            Severity::Low,
            "Low",
        ));

        let critical = auditor.get_critical();
        assert_eq!(critical.len(), 1);
    }
}

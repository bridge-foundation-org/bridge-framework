//! Cron Job Scheduling - Schedule tasks with cron expressions
//!
//! Parse and execute cron jobs on a schedule

use std::collections::HashMap;

/// Cron field (minute, hour, day, month, weekday)
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CronField {
    Any,
    Specific(u32),
    Range(u32, u32),
    Step(u32, u32), // value/step
    List(Vec<u32>),
}

impl CronField {
    /// Check if a value matches this cron field
    pub fn matches(&self, value: u32) -> bool {
        match self {
            CronField::Any => true,
            CronField::Specific(v) => *v == value,
            CronField::Range(start, end) => value >= *start && value <= *end,
            CronField::Step(step, max) => value % step == 0 && value <= *max,
            CronField::List(values) => values.contains(&value),
        }
    }
}

/// Cron expression (minute hour day month weekday)
#[derive(Clone, Debug)]
pub struct CronExpression {
    pub minute: CronField,
    pub hour: CronField,
    pub day: CronField,
    pub month: CronField,
    pub weekday: CronField,
}

impl CronExpression {
    /// Create a new cron expression
    pub fn new(
        minute: CronField,
        hour: CronField,
        day: CronField,
        month: CronField,
        weekday: CronField,
    ) -> Self {
        CronExpression {
            minute,
            hour,
            day,
            month,
            weekday,
        }
    }

    /// Parse a cron expression string (5 fields)
    pub fn parse(expr: &str) -> Result<Self, String> {
        let parts: Vec<&str> = expr.split_whitespace().collect();
        if parts.len() != 5 {
            return Err("Cron expression must have 5 fields".to_string());
        }

        Ok(CronExpression::new(
            Self::parse_field(parts[0], 0, 59)?,
            Self::parse_field(parts[1], 0, 23)?,
            Self::parse_field(parts[2], 1, 31)?,
            Self::parse_field(parts[3], 1, 12)?,
            Self::parse_field(parts[4], 0, 6)?,
        ))
    }

    fn parse_field(field: &str, min: u32, max: u32) -> Result<CronField, String> {
        if field == "*" {
            Ok(CronField::Any)
        } else if let Ok(v) = field.parse::<u32>() {
            if v >= min && v <= max {
                Ok(CronField::Specific(v))
            } else {
                Err(format!("Value {} out of range [{}, {}]", v, min, max))
            }
        } else if field.contains('-') {
            let parts: Vec<&str> = field.split('-').collect();
            if parts.len() != 2 {
                return Err("Invalid range format".to_string());
            }
            let start = parts[0].parse::<u32>().map_err(|_| "Invalid start")?;
            let end = parts[1].parse::<u32>().map_err(|_| "Invalid end")?;
            Ok(CronField::Range(start, end))
        } else if field.contains('/') {
            let parts: Vec<&str> = field.split('/').collect();
            if parts.len() != 2 {
                return Err("Invalid step format".to_string());
            }
            let step = parts[1].parse::<u32>().map_err(|_| "Invalid step")?;
            Ok(CronField::Step(step, max))
        } else if field.contains(',') {
            let values: Result<Vec<u32>, _> = field.split(',').map(|v| v.parse::<u32>()).collect();
            Ok(CronField::List(values.map_err(|_| "Invalid list")?))
        } else {
            Err(format!("Invalid cron field: {}", field))
        }
    }

    /// Check if the expression matches a given time (minute, hour, day, month, weekday)
    pub fn matches(&self, minute: u32, hour: u32, day: u32, month: u32, weekday: u32) -> bool {
        self.minute.matches(minute)
            && self.hour.matches(hour)
            && self.day.matches(day)
            && self.month.matches(month)
            && self.weekday.matches(weekday)
    }
}

/// Scheduled job
#[derive(Clone, Debug)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub schedule: CronExpression,
    pub handler: String, // Handler function name
    pub enabled: bool,
}

impl Job {
    /// Create a new job
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        schedule: CronExpression,
        handler: impl Into<String>,
    ) -> Self {
        Job {
            id: id.into(),
            name: name.into(),
            schedule,
            handler: handler.into(),
            enabled: true,
        }
    }

    /// Disable the job
    pub fn disable(mut self) -> Self {
        self.enabled = false;
        self
    }
}

/// Cron scheduler
pub struct Scheduler {
    jobs: HashMap<String, Job>,
}

impl Scheduler {
    /// Create a new scheduler
    pub fn new() -> Self {
        Scheduler {
            jobs: HashMap::new(),
        }
    }

    /// Add a job
    pub fn add_job(&mut self, job: Job) {
        self.jobs.insert(job.id.clone(), job);
    }

    /// Get a job by ID
    pub fn get_job(&self, id: &str) -> Option<&Job> {
        self.jobs.get(id)
    }

    /// List all jobs
    pub fn list_jobs(&self) -> Vec<&Job> {
        self.jobs.values().collect()
    }

    /// Remove a job
    pub fn remove_job(&mut self, id: &str) -> bool {
        self.jobs.remove(id).is_some()
    }

    /// Get jobs due at a specific time
    pub fn jobs_due(
        &self,
        minute: u32,
        hour: u32,
        day: u32,
        month: u32,
        weekday: u32,
    ) -> Vec<&Job> {
        self.jobs
            .values()
            .filter(|j| j.enabled && j.schedule.matches(minute, hour, day, month, weekday))
            .collect()
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_field_any() {
        let field = CronField::Any;
        assert!(field.matches(0));
        assert!(field.matches(59));
    }

    #[test]
    fn test_cron_field_specific() {
        let field = CronField::Specific(30);
        assert!(field.matches(30));
        assert!(!field.matches(31));
    }

    #[test]
    fn test_cron_field_range() {
        let field = CronField::Range(9, 17);
        assert!(field.matches(9));
        assert!(field.matches(12));
        assert!(field.matches(17));
        assert!(!field.matches(8));
        assert!(!field.matches(18));
    }

    #[test]
    fn test_cron_field_step() {
        let field = CronField::Step(15, 59);
        assert!(field.matches(0));
        assert!(field.matches(15));
        assert!(field.matches(30));
        assert!(field.matches(45));
        assert!(!field.matches(7));
    }

    #[test]
    fn test_cron_field_list() {
        let field = CronField::List(vec![1, 3, 5]);
        assert!(field.matches(1));
        assert!(field.matches(3));
        assert!(field.matches(5));
        assert!(!field.matches(2));
    }

    #[test]
    fn test_cron_expression_parse_any() {
        let expr = CronExpression::parse("* * * * *").unwrap();
        assert!(expr.matches(0, 0, 1, 1, 0));
        assert!(expr.matches(59, 23, 31, 12, 6));
    }

    #[test]
    fn test_cron_expression_parse_specific() {
        let expr = CronExpression::parse("30 9 * * *").unwrap();
        assert!(expr.matches(30, 9, 1, 1, 0));
        assert!(!expr.matches(31, 9, 1, 1, 0));
    }

    #[test]
    fn test_cron_expression_parse_range() {
        let expr = CronExpression::parse("* 9-17 * * *").unwrap();
        assert!(expr.matches(0, 9, 1, 1, 0));
        assert!(expr.matches(0, 17, 1, 1, 0));
        assert!(!expr.matches(0, 8, 1, 1, 0));
    }

    #[test]
    fn test_cron_expression_parse_step() {
        let expr = CronExpression::parse("*/15 * * * *").unwrap();
        assert!(expr.matches(0, 0, 1, 1, 0));
        assert!(expr.matches(15, 0, 1, 1, 0));
        assert!(expr.matches(30, 0, 1, 1, 0));
        assert!(!expr.matches(7, 0, 1, 1, 0));
    }

    #[test]
    fn test_cron_expression_parse_list() {
        let expr = CronExpression::parse("0 9,12,18 * * *").unwrap();
        assert!(expr.matches(0, 9, 1, 1, 0));
        assert!(expr.matches(0, 12, 1, 1, 0));
        assert!(expr.matches(0, 18, 1, 1, 0));
        assert!(!expr.matches(0, 10, 1, 1, 0));
    }

    #[test]
    fn test_cron_expression_invalid_fields() {
        assert!(CronExpression::parse("* * *").is_err());
        assert!(CronExpression::parse("* * * * * *").is_err());
    }

    #[test]
    fn test_job_new() {
        let schedule = CronExpression::parse("0 * * * *").unwrap();
        let job = Job::new("job1", "Hourly job", schedule, "handle_hourly");
        assert_eq!(job.id, "job1");
        assert!(job.enabled);
    }

    #[test]
    fn test_job_disable() {
        let schedule = CronExpression::parse("0 * * * *").unwrap();
        let job = Job::new("job1", "Test", schedule, "handler").disable();
        assert!(!job.enabled);
    }

    #[test]
    fn test_scheduler_new() {
        let scheduler = Scheduler::new();
        assert_eq!(scheduler.list_jobs().len(), 0);
    }

    #[test]
    fn test_scheduler_add_job() {
        let mut scheduler = Scheduler::new();
        let schedule = CronExpression::parse("0 * * * *").unwrap();
        let job = Job::new("job1", "Test", schedule, "handler");
        scheduler.add_job(job);
        assert_eq!(scheduler.list_jobs().len(), 1);
    }

    #[test]
    fn test_scheduler_get_job() {
        let mut scheduler = Scheduler::new();
        let schedule = CronExpression::parse("0 * * * *").unwrap();
        let job = Job::new("job1", "Test", schedule, "handler");
        scheduler.add_job(job);
        assert!(scheduler.get_job("job1").is_some());
    }

    #[test]
    fn test_scheduler_remove_job() {
        let mut scheduler = Scheduler::new();
        let schedule = CronExpression::parse("0 * * * *").unwrap();
        let job = Job::new("job1", "Test", schedule, "handler");
        scheduler.add_job(job);
        assert!(scheduler.remove_job("job1"));
        assert!(scheduler.get_job("job1").is_none());
    }

    #[test]
    fn test_scheduler_jobs_due() {
        let mut scheduler = Scheduler::new();
        let schedule = CronExpression::parse("0 9 * * *").unwrap();
        let job = Job::new("job1", "Morning job", schedule, "handler");
        scheduler.add_job(job);

        assert_eq!(scheduler.jobs_due(0, 9, 1, 1, 0).len(), 1);
        assert_eq!(scheduler.jobs_due(0, 10, 1, 1, 0).len(), 0);
    }

    #[test]
    fn test_scheduler_disabled_job() {
        let mut scheduler = Scheduler::new();
        let schedule = CronExpression::parse("0 9 * * *").unwrap();
        let job = Job::new("job1", "Test", schedule, "handler").disable();
        scheduler.add_job(job);

        assert_eq!(scheduler.jobs_due(0, 9, 1, 1, 0).len(), 0);
    }
}

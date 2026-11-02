use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Call {
    pub provider: String,
    pub model: String,
    pub project: String,
    pub session: String,
    pub at: DateTime<Utc>,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub cost: f64,
}

pub fn tokens_of(value: Option<f64>) -> u64 {
    match value {
        Some(value) if value.is_finite() && value > 0.0 => value.min(u64::MAX as f64) as u64,
        _ => 0,
    }
}

pub fn project_name(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    trimmed
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(trimmed)
        .to_string()
}

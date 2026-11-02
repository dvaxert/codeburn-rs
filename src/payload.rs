use chrono::Utc;
use serde::Serialize;

use crate::report::{self, Report};
use crate::scan::Scan;
use crate::usage::Call;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Period {
    Today,
    Week,
    Days30,
    Month,
    All,
}

impl Period {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "today" => Some(Self::Today),
            "week" => Some(Self::Week),
            "30days" => Some(Self::Days30),
            "month" => Some(Self::Month),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Week => "week",
            Self::Days30 => "30days",
            Self::Month => "month",
            Self::All => "all",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "Today",
            Self::Week => "7 days",
            Self::Days30 => "30 days",
            Self::Month => "Month",
            Self::All => "All",
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProviderJson<'a> {
    name: &'a str,
    located: bool,
    files: u64,
    parsed: bool,
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    calls: u64,
    sessions: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CurrentJson<'a> {
    label: &'a str,
    cost: f64,
    calls: u64,
    sessions: u64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
    top_models: Vec<ModelJson>,
    projects: Vec<ProjectJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelJson {
    name: String,
    cost: f64,
    tokens: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectJson {
    name: String,
    cost: f64,
    tokens: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DayJson {
    date: String,
    cost: f64,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_write_tokens: u64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryJson<'a> {
    label: &'a str,
    daily: Vec<DayJson>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Payload<'a> {
    generated: String,
    period: &'a str,
    providers: Vec<ProviderJson<'a>>,
    current: CurrentJson<'a>,
    history: HistoryJson<'a>,
}

/// Status JSON the Noctalia plugin renders.
pub fn menubar_json(scan: &Scan, calls: &[Call], period: Period) -> String {
    let report = report::build(calls, period, Utc::now());
    serde_json::to_string(&payload_from(scan, &report, period)).expect("payload is a plain struct")
}

fn payload_from<'a>(scan: &'a Scan, report: &'a Report, period: Period) -> Payload<'a> {
    Payload {
        generated: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        period: period.as_str(),
        providers: scan
            .providers
            .iter()
            .map(|provider| {
                let rollup = report.providers.iter().find(|row| row.name == provider.name);
                ProviderJson {
                    name: provider.name,
                    located: provider.located,
                    files: provider.files,
                    parsed: true,
                    cost: rollup.map(|row| row.cost).unwrap_or(0.0),
                    input_tokens: rollup.map(|row| row.input).unwrap_or(0),
                    output_tokens: rollup.map(|row| row.output).unwrap_or(0),
                    cache_read_tokens: rollup.map(|row| row.cache_read).unwrap_or(0),
                    cache_write_tokens: rollup.map(|row| row.cache_write).unwrap_or(0),
                    calls: rollup.map(|row| row.calls).unwrap_or(0),
                    sessions: rollup.map(|row| row.sessions).unwrap_or(0),
                }
            })
            .collect(),
        current: CurrentJson {
            label: period.label(),
            cost: report.cost,
            calls: report.calls,
            sessions: report.sessions,
            input_tokens: report.input,
            output_tokens: report.output,
            cache_read_tokens: report.cache_read,
            cache_write_tokens: report.cache_write,
            top_models: report
                .models
                .iter()
                .map(|model| ModelJson {
                    name: model.name.clone(),
                    cost: model.cost,
                    tokens: model.tokens,
                })
                .collect(),
            projects: report
                .projects
                .iter()
                .map(|project| ProjectJson {
                    name: project.name.clone(),
                    cost: project.cost,
                    tokens: project.tokens,
                })
                .collect(),
        },
        history: HistoryJson {
            label: report.chart_label,
            daily: report
                .days
                .iter()
                .map(|day| DayJson {
                    date: day.date.format("%Y-%m-%d").to_string(),
                    cost: day.cost,
                    input_tokens: day.input,
                    output_tokens: day.output,
                    cache_read_tokens: day.cache_read,
                    cache_write_tokens: day.cache_write,
                })
                .collect(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{ProviderScan, Scan};

    #[test]
    fn rejects_unknown_periods() {
        assert!(Period::parse("today").is_some());
        assert!(Period::parse("lifetime").is_none());
        assert!(Period::parse("copilot").is_none());
    }

    #[test]
    fn json_keeps_zero_cost_until_parsed() {
        let scan = Scan {
            providers: vec![ProviderScan {
                name: "claude",
                located: true,
                files: 4,
                parsed: false,
            }],
        };
        let value: serde_json::Value =
            serde_json::from_str(&menubar_json(&scan, &[], Period::Today)).unwrap();
        assert_eq!(value["period"], "today");
        assert_eq!(value["current"]["cost"], 0.0);
        assert_eq!(value["current"]["inputTokens"], 0);
        assert_eq!(value["history"]["daily"].as_array().unwrap().len(), 14);
        assert_eq!(value["providers"][0]["name"], "claude");
        assert_eq!(value["providers"][0]["files"], 4);
        assert_eq!(value["providers"][0]["parsed"], true);
        assert!(value.get("copilot").is_none());
    }
}

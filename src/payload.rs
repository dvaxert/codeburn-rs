use chrono::Utc;
use serde::Serialize;

use crate::scan::Scan;

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
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectJson {
    name: String,
    cost: f64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Payload<'a> {
    generated: String,
    period: &'a str,
    providers: Vec<ProviderJson<'a>>,
    current: CurrentJson<'a>,
}

/// Status JSON the Noctalia plugin renders. Costs stay zero until a provider
/// reports `parsed: true`.
pub fn menubar_json(scan: &Scan, period: Period) -> String {
    let payload = Payload {
        generated: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        period: period.as_str(),
        providers: scan
            .providers
            .iter()
            .map(|provider| ProviderJson {
                name: provider.name,
                located: provider.located,
                files: provider.files,
                parsed: provider.parsed,
            })
            .collect(),
        current: CurrentJson {
            label: period.label(),
            cost: 0.0,
            calls: 0,
            sessions: 0,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            top_models: Vec::new(),
            projects: Vec::new(),
        },
    };
    serde_json::to_string(&payload).expect("payload is a plain struct")
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
            serde_json::from_str(&menubar_json(&scan, Period::Today)).unwrap();
        assert_eq!(value["period"], "today");
        assert_eq!(value["current"]["cost"], 0.0);
        assert_eq!(value["current"]["inputTokens"], 0);
        assert_eq!(value["current"]["topModels"], serde_json::json!([]));
        assert_eq!(value["providers"][0]["name"], "claude");
        assert_eq!(value["providers"][0]["files"], 4);
        assert_eq!(value["providers"][0]["parsed"], false);
        assert!(value.get("copilot").is_none());
    }
}

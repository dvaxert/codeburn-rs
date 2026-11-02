use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, Utc};

use crate::payload::Period;
use crate::usage::Call;

#[derive(Debug, Clone)]
pub struct DayPoint {
    pub date: NaiveDate,
    pub cost: f64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

#[derive(Debug, Clone)]
pub struct Bucket {
    pub name: String,
    pub cost: f64,
    pub tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderRollup {
    pub name: String,
    pub cost: f64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub calls: u64,
    pub sessions: u64,
}

#[derive(Debug, Clone)]
pub struct Report {
    pub chart_label: &'static str,
    pub days: Vec<DayPoint>,
    pub cost: f64,
    pub calls: u64,
    pub sessions: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub models: Vec<Bucket>,
    pub projects: Vec<Bucket>,
    pub providers: Vec<ProviderRollup>,
}

pub fn build(calls: &[Call], period: Period, now: DateTime<Utc>) -> Report {
    let today = now.with_timezone(&Local).date_naive();
    let (chart_start, chart_label) = chart_window(period, today);
    let mut days = HashMap::<NaiveDate, DayPoint>::new();
    let mut cursor = chart_start;
    while cursor <= today {
        days.insert(cursor, empty_day(cursor));
        cursor += Duration::days(1);
    }

    let mut models: HashMap<String, Bucket> = HashMap::new();
    let mut projects: HashMap<String, Bucket> = HashMap::new();
    let mut providers: HashMap<String, ProviderRollup> = HashMap::new();
    let mut sessions: HashMap<String, HashSet<String>> = HashMap::new();
    let mut cost = 0.0;
    let mut call_count = 0u64;
    let mut input = 0u64;
    let mut output = 0u64;
    let mut cache_read = 0u64;
    let mut cache_write = 0u64;
    let mut period_sessions = HashSet::new();

    for call in calls {
        let date = call.at.with_timezone(&Local).date_naive();
        if (chart_start..=today).contains(&date) {
            let day = days.entry(date).or_insert_with(|| empty_day(date));
            add_day(day, call);
        }
        if !in_period(period, date, today) {
            continue;
        }
        cost += call.cost;
        call_count += 1;
        input += call.input;
        output += call.output;
        cache_read += call.cache_read;
        cache_write += call.cache_write;
        period_sessions.insert(format!("{}:{}", call.provider, call.session));
        let model = models
            .entry(display_model(&call.model))
            .or_insert_with(|| Bucket {
                name: display_model(&call.model),
                cost: 0.0,
                tokens: 0,
            });
        model.cost += call.cost;
        model.tokens += call.input + call.output + call.cache_read + call.cache_write;
        let project = projects
            .entry(call.project.clone())
            .or_insert_with(|| Bucket {
                name: call.project.clone(),
                cost: 0.0,
                tokens: 0,
            });
        project.cost += call.cost;
        project.tokens += call.input + call.output + call.cache_read + call.cache_write;
        let provider = providers
            .entry(call.provider.clone())
            .or_insert_with(|| ProviderRollup {
                name: call.provider.clone(),
                cost: 0.0,
                input: 0,
                output: 0,
                cache_read: 0,
                cache_write: 0,
                calls: 0,
                sessions: 0,
            });
        provider.cost += call.cost;
        provider.input += call.input;
        provider.output += call.output;
        provider.cache_read += call.cache_read;
        provider.cache_write += call.cache_write;
        provider.calls += 1;
        sessions
            .entry(call.provider.clone())
            .or_default()
            .insert(call.session.clone());
    }
    for (name, ids) in sessions {
        if let Some(provider) = providers.get_mut(&name) {
            provider.sessions = ids.len() as u64;
        }
    }

    let mut days: Vec<_> = days.into_values().collect();
    days.sort_by_key(|day| day.date);
    let mut models: Vec<_> = models.into_values().collect();
    models.sort_by(|a, b| b.cost.total_cmp(&a.cost).then(b.tokens.cmp(&a.tokens)));
    models.truncate(8);
    let mut projects: Vec<_> = projects.into_values().collect();
    projects.sort_by(|a, b| b.cost.total_cmp(&a.cost).then(b.tokens.cmp(&a.tokens)));
    projects.truncate(8);

    Report {
        chart_label,
        days,
        cost: money(cost),
        calls: call_count,
        sessions: period_sessions.len() as u64,
        input,
        output,
        cache_read,
        cache_write,
        models,
        projects,
        providers: providers.into_values().collect(),
    }
}

fn chart_window(period: Period, today: NaiveDate) -> (NaiveDate, &'static str) {
    match period {
        Period::Today => (today - Duration::days(13), "14 days"),
        Period::Week => (today - Duration::days(6), "7 days"),
        Period::Days30 => (today - Duration::days(29), "30 days"),
        Period::Month => (today.with_day(1).unwrap_or(today), "Month"),
        Period::All => (today - Duration::days(89), "90 days"),
    }
}

fn in_period(period: Period, date: NaiveDate, today: NaiveDate) -> bool {
    match period {
        Period::Today => date == today,
        Period::Week => date > today - Duration::days(7) && date <= today,
        Period::Days30 => date > today - Duration::days(30) && date <= today,
        Period::Month => {
            date.year() == today.year() && date.month() == today.month() && date <= today
        }
        Period::All => date <= today,
    }
}

fn empty_day(date: NaiveDate) -> DayPoint {
    DayPoint {
        date,
        cost: 0.0,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
    }
}

fn add_day(day: &mut DayPoint, call: &Call) {
    day.cost += call.cost;
    day.input += call.input;
    day.output += call.output;
    day.cache_read += call.cache_read;
    day.cache_write += call.cache_write;
}

fn money(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn display_model(model: &str) -> String {
    if let Some(rest) = model.strip_prefix("claude-") {
        let (family, version) = rest.split_once('-').unwrap_or((rest, ""));
        let mut family_chars = family.chars();
        let family = match family_chars.next() {
            Some(first) => first.to_uppercase().collect::<String>() + family_chars.as_str(),
            None => String::new(),
        };
        let version = version.replace('-', ".");
        return if version.is_empty() {
            family
        } else {
            format!("{family} {version}")
        };
    }
    if let Some(rest) = model.strip_prefix("gpt-") {
        return format!("GPT-{rest}");
    }
    model.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn call(day: u32, cost: f64) -> Call {
        Call {
            provider: "claude".to_string(),
            model: "claude-opus-5-5".to_string(),
            project: "demo".to_string(),
            session: "s".to_string(),
            at: Utc.with_ymd_and_hms(2026, 11, day, 12, 0, 0).unwrap(),
            input: 10,
            output: 2,
            cache_read: 0,
            cache_write: 0,
            cost,
        }
    }

    #[test]
    fn today_keeps_a_two_week_chart_and_one_day_of_spend() {
        let now = Utc.with_ymd_and_hms(2026, 11, 2, 15, 0, 0).unwrap();
        let report = build(&[call(1, 1.0), call(2, 3.0)], Period::Today, now);
        assert_eq!(report.cost, 3.0);
        assert_eq!(report.calls, 1);
        assert_eq!(report.days.len(), 14);
        assert_eq!(
            report.days.last().unwrap().date,
            NaiveDate::from_ymd_opt(2026, 11, 2).unwrap()
        );
        assert!(report.days.iter().any(|day| day.cost == 1.0));
        assert_eq!(report.models[0].name, "Opus 5.5");
    }
}

use std::sync::OnceLock;

use serde_json::Value;

const SNAPSHOT: &str = include_str!("../data/prices.json");
const ONE_HOUR_CACHE_WRITE: f64 = 1.6;

#[derive(Clone, Debug)]
struct Tier {
    threshold: f64,
    input: f64,
    output: f64,
    cache_write: Option<f64>,
    cache_read: Option<f64>,
}

#[derive(Clone, Debug)]
struct Rate {
    input: f64,
    output: f64,
    cache_write: f64,
    cache_read: f64,
    cache_write_explicit: bool,
    fast: f64,
    tier: Option<Tier>,
}

struct Book {
    rates: Vec<(String, Rate)>,
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 0.0)
}

fn rate_from_array(values: &[Value]) -> Option<Rate> {
    let input = number(values.first())?;
    let output = number(values.get(1))?;
    let cache_write = number(values.get(2));
    let cache_read = number(values.get(3));
    let fast = number(values.get(4)).unwrap_or(1.0);
    let tier = values.get(5).and_then(Value::as_object).and_then(|tier| {
        let threshold = number(tier.get("threshold"))?;
        let input = number(tier.get("input"))?;
        let output = number(tier.get("output"))?;
        Some(Tier {
            threshold,
            input,
            output,
            cache_write: number(tier.get("cacheWrite")),
            cache_read: number(tier.get("cacheRead")),
        })
    });
    Some(Rate {
        input,
        output,
        cache_write: cache_write.unwrap_or(input * 1.25),
        cache_read: cache_read.unwrap_or(input * 0.1),
        cache_write_explicit: cache_write.is_some(),
        fast,
        tier,
    })
}

fn book() -> &'static Book {
    static BOOK: OnceLock<Book> = OnceLock::new();
    BOOK.get_or_init(|| {
        let raw: serde_json::Map<String, Value> =
            serde_json::from_str(SNAPSHOT).expect("pricing snapshot is a JSON object");
        let mut rates = Vec::new();
        for (name, value) in raw {
            let Some(row) = value.as_array().and_then(|row| rate_from_array(row)) else {
                continue;
            };
            rates.push((name, row));
        }
        for (name, row) in [
            ("composer-2.5", [0.5e-6, 2.5e-6, 0.5e-6, 0.2e-6]),
            ("composer-2", [0.5e-6, 2.5e-6, 0.5e-6, 0.2e-6]),
            ("composer-1.5", [3.5e-6, 17.5e-6, 3.5e-6, 0.35e-6]),
            ("composer-1", [1.25e-6, 10e-6, 1.25e-6, 0.125e-6]),
        ] {
            let row = rate_from_array(
                &row.into_iter()
                    .map(Value::from)
                    .collect::<Vec<_>>(),
            )
            .expect("override row");
            if let Some(existing) = rates.iter_mut().find(|(key, _)| key == name) {
                existing.1 = row;
            } else {
                rates.push((name.to_string(), row));
            }
        }
        rates.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Book { rates }
    })
}

fn boundary_match(model: &str, key: &str) -> bool {
    let Some(rest) = model.get(key.len()..) else {
        return false;
    };
    model.as_bytes().get(..key.len()) == Some(key.as_bytes()) && (rest.is_empty() || rest.starts_with('-'))
}

fn find_rate(model: &str) -> Option<Rate> {
    let book = book();
    if let Some((_, rate)) = book.rates.iter().find(|(key, _)| key == model) {
        return Some(rate.clone());
    }
    let stripped = model
        .rsplit_once('-')
        .filter(|(_, tail)| tail.len() == 8 && tail.bytes().all(|b| b.is_ascii_digit()))
        .map(|(head, _)| head);
    if let Some(stripped) = stripped {
        if let Some((_, rate)) = book.rates.iter().find(|(key, _)| key == stripped) {
            return Some(rate.clone());
        }
    }
    book.rates
        .iter()
        .find(|(key, _)| boundary_match(model, key) || stripped.is_some_and(|id| boundary_match(id, key)))
        .map(|(_, rate)| rate.clone())
        .or_else(|| {
            book.rates
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(model))
                .map(|(_, rate)| rate.clone())
        })
}

/// USD for one call. Cache-write tokens are the 5-minute portion; `cache_write_1h`
/// is billed at 1.6× that rate. Codex must pass input with cached tokens already
/// removed, and cache writes only when the rate is explicit.
pub fn cost(
    model: &str,
    input: u64,
    output: u64,
    cache_write: u64,
    cache_read: u64,
    cache_write_1h: u64,
    fast: bool,
) -> f64 {
    let Some(mut rate) = find_rate(model) else {
        return 0.0;
    };
    let prompt = input.saturating_add(cache_write).saturating_add(cache_write_1h).saturating_add(cache_read) as f64;
    if let Some(tier) = rate.tier.clone() {
        if prompt > tier.threshold {
            rate.input = tier.input;
            rate.output = tier.output;
            if let Some(write) = tier.cache_write {
                rate.cache_write = write;
                rate.cache_write_explicit = true;
            }
            if let Some(read) = tier.cache_read {
                rate.cache_read = read;
            }
        }
    }
    let multiplier = if fast { rate.fast } else { 1.0 };
    multiplier
        * (input as f64 * rate.input
            + output as f64 * rate.output
            + cache_write as f64 * rate.cache_write
            + cache_write_1h as f64 * rate.cache_write * ONE_HOUR_CACHE_WRITE
            + cache_read as f64 * rate.cache_read)
}

pub fn codex_buckets(input: u64, cached: u64, cache_write: u64, model: &str) -> (u64, u64, u64) {
    let explicit = find_rate(model).is_some_and(|rate| rate.cache_write_explicit);
    let uncached = input.saturating_sub(cached);
    let write = if explicit {
        cache_write.min(uncached)
    } else {
        0
    };
    (uncached.saturating_sub(write), write, cached)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prices_claude_opus_from_the_snapshot() {
        let usd = cost("claude-opus-5-5", 1_000_000, 0, 0, 0, 0, false);
        assert!(usd > 1.0, "{usd}");
    }

    #[test]
    fn prices_composer_override() {
        let usd = cost("composer-2.5", 1_000_000, 0, 0, 0, 0, false);
        assert!((usd - 0.5).abs() < 1e-9, "{usd}");
    }

    #[test]
    fn does_not_prefix_match_across_a_dot() {
        assert!(find_rate("gpt-5.4").is_some());
        let exact = cost("gpt-5.4", 1_000_000, 0, 0, 0, 0, false);
        let bare = cost("gpt-5", 1_000_000, 0, 0, 0, 0, false);
        assert!((exact - bare).abs() > 1e-9 || exact == 0.0);
    }
}

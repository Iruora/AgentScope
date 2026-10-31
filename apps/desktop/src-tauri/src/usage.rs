//! Cost estimates from the token counts harnesses report. Prices are list
//! API prices from the bundled `prices.json`, optionally overridden by
//! `<data dir>/prices.json`. Subscription plans are not billed per token, so
//! every figure is shown as an estimate.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use agentscope_core::store::UsageRow;
use agentscope_core::{Session, Tokens};
use serde::{Deserialize, Serialize};
use specta::Type;

const BUNDLED: &str = include_str!("../prices.json");

#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cached_read: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct PriceFile {
    #[serde(default)]
    checked: Option<String>,
    #[serde(default)]
    models: HashMap<String, Price>,
}

#[derive(Debug, Clone)]
pub struct Prices {
    models: Vec<(String, Price)>,
    pub checked: String,
    /// The override file failed to parse.
    pub override_error: Option<String>,
}

fn normalize(model: &str) -> String {
    model
        .trim()
        .to_ascii_lowercase()
        .replace(['.', '_'], "-")
        // Claude Code tags long-context sessions as `claude-opus-5-5[1m]`.
        .split('[')
        .next()
        .unwrap_or_default()
        .trim_start_matches("anthropic/")
        .trim_start_matches("openai/")
        .to_owned()
}

impl Prices {
    pub fn load(data_dir: &Path) -> Self {
        let bundled: PriceFile =
            serde_json::from_str(BUNDLED).expect("bundled prices.json is valid");
        let mut models: HashMap<String, Price> = bundled.models;
        let mut checked = bundled.checked.unwrap_or_default();
        let mut override_error = None;
        match std::fs::read_to_string(data_dir.join("prices.json")) {
            Ok(text) => match serde_json::from_str::<PriceFile>(&text) {
                Ok(file) => {
                    models.extend(file.models);
                    checked = file.checked.unwrap_or(checked);
                }
                Err(err) => override_error = Some(err.to_string()),
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => override_error = Some(err.to_string()),
        }
        let mut models: Vec<(String, Price)> = models
            .into_iter()
            .map(|(k, v)| (normalize(&k), v))
            .collect();
        // Longest prefix first, so `gpt-5-codex` wins over `gpt-5`.
        models.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Self {
            models,
            checked,
            override_error,
        }
    }

    pub fn price(&self, model: &str) -> Option<Price> {
        let model = normalize(model);
        self.models
            .iter()
            .find(|(prefix, _)| {
                model == *prefix
                    || model
                        .strip_prefix(prefix.as_str())
                        .is_some_and(|rest| rest.starts_with('-'))
            })
            .map(|(_, p)| *p)
    }

    /// Estimated USD, or `None` when the model has no price.
    pub fn cost(&self, model: Option<&str>, tokens: &Tokens) -> Option<f64> {
        let price = self.price(model?)?;
        let cached = tokens.cached.unwrap_or(0) as f64;
        let read = price.cached_read.unwrap_or(price.input / 10.0);
        Some(
            (tokens.input as f64 * price.input
                + tokens.output as f64 * price.output
                + cached * read)
                / 1_000_000.0,
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotal {
    pub key: String,
    #[specta(type = f64)]
    pub input: u64,
    #[specta(type = f64)]
    pub output: u64,
    #[specta(type = f64)]
    pub cached: u64,
    /// Estimated USD for the priced part.
    pub cost: f64,
    /// Tokens of models without a price, left out of `cost`.
    #[specta(type = f64)]
    pub unpriced_tokens: u64,
}

impl UsageTotal {
    fn add(&mut self, tokens: &Tokens, cost: Option<f64>) {
        self.input += tokens.input;
        self.output += tokens.output;
        self.cached += tokens.cached.unwrap_or(0);
        match cost {
            Some(c) => self.cost += c,
            None => self.unpriced_tokens += tokens.input + tokens.output,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    pub days: u32,
    pub total: UsageTotal,
    /// One entry per day in the range, oldest first, including empty days.
    pub by_day: Vec<UsageTotal>,
    pub by_harness: Vec<UsageTotal>,
    pub by_repo: Vec<UsageTotal>,
    pub by_model: Vec<UsageTotal>,
    pub unpriced_models: Vec<String>,
    pub prices_checked: String,
    pub prices_error: Option<String>,
}

fn sorted(map: BTreeMap<String, UsageTotal>) -> Vec<UsageTotal> {
    let mut v: Vec<UsageTotal> = map.into_values().collect();
    v.sort_by(|a, b| {
        b.cost
            .total_cmp(&a.cost)
            .then((b.input + b.output).cmp(&(a.input + a.output)))
    });
    v
}

pub fn report(rows: &[UsageRow], day_keys: Vec<String>, prices: &Prices) -> UsageReport {
    let mut total = UsageTotal {
        key: "total".into(),
        ..Default::default()
    };
    let mut by_day: BTreeMap<String, UsageTotal> = day_keys
        .iter()
        .map(|d| {
            (
                d.clone(),
                UsageTotal {
                    key: d.clone(),
                    ..Default::default()
                },
            )
        })
        .collect();
    let mut by_harness = BTreeMap::new();
    let mut by_repo = BTreeMap::new();
    let mut by_model = BTreeMap::new();
    let mut unpriced = std::collections::BTreeSet::new();
    let group = |map: &mut BTreeMap<String, UsageTotal>, key: String, t: &Tokens, c| {
        map.entry(key.clone())
            .or_insert_with(|| UsageTotal {
                key,
                ..Default::default()
            })
            .add(t, c);
    };
    for row in rows {
        let cost = prices.cost(Some(&row.model), &row.tokens);
        if cost.is_none() {
            unpriced.insert(row.model.clone());
        }
        total.add(&row.tokens, cost);
        if let Some(day) = by_day.get_mut(&row.day) {
            day.add(&row.tokens, cost);
        }
        group(
            &mut by_harness,
            row.harness.as_str().to_owned(),
            &row.tokens,
            cost,
        );
        group(
            &mut by_repo,
            row.repo.clone().unwrap_or_else(|| "(no repo)".into()),
            &row.tokens,
            cost,
        );
        group(&mut by_model, row.model.clone(), &row.tokens, cost);
    }
    UsageReport {
        days: day_keys.len() as u32,
        total,
        by_day: by_day.into_values().collect(),
        by_harness: sorted(by_harness),
        by_repo: sorted(by_repo),
        by_model: sorted(by_model),
        unpriced_models: unpriced.into_iter().collect(),
        prices_checked: prices.checked.clone(),
        prices_error: prices.override_error.clone(),
    }
}

/// The last `days` local dates, oldest first.
pub fn day_keys(days: u32) -> Vec<String> {
    let today = chrono::Local::now().date_naive();
    (0..days)
        .rev()
        .map(|i| {
            (today - chrono::Duration::days(i.into()))
                .format("%Y-%m-%d")
                .to_string()
        })
        .collect()
}

/// Estimated cost per session, for cards and History. Sessions without a
/// priced model are left out.
pub fn session_costs<'a>(
    sessions: impl IntoIterator<Item = &'a Session>,
    prices: &Prices,
) -> HashMap<String, f64> {
    sessions
        .into_iter()
        .filter_map(|s| {
            let cost = prices.cost(s.model.as_deref(), s.tokens.as_ref()?)?;
            Some((s.id.clone(), cost))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use agentscope_core::HarnessId;

    fn prices() -> Prices {
        Prices::load(Path::new("/nonexistent"))
    }

    #[test]
    fn matches_models_by_longest_prefix() {
        let p = prices();
        assert_eq!(p.price("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(p.price("claude-opus-5-5[1m]").unwrap().input, 4.0);
        assert_eq!(p.price("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(p.price("claude-sonnet-4-5-20250929").unwrap().input, 3.0);
        assert_eq!(
            p.price("claude-sonnet-4.5").unwrap().input,
            3.0,
            "Copilot uses dots"
        );
        assert_eq!(p.price("gpt-5-codex").unwrap().output, 10.0);
        assert_eq!(p.price("gpt-5.3-codex").unwrap().output, 14.0);
        assert_eq!(p.price("gpt-5").unwrap().input, 1.25);
        assert!(
            p.price("claude-opus-50").is_none(),
            "prefix must end at a dash"
        );
        assert!(p.price("mystery-model").is_none());
    }

    #[test]
    fn costs_include_cache_reads() {
        let p = prices();
        let t = Tokens {
            input: 1_000_000,
            output: 100_000,
            cached: Some(2_000_000),
        };
        // 4 + 2 + 0.4
        let cost = p.cost(Some("claude-opus-5-5"), &t).unwrap();
        assert!((cost - 6.4).abs() < 1e-9, "{cost}");
        // Default cache read is 10% of input: 5 + 2.5 + 1.
        let cost = p.cost(Some("claude-opus-5"), &t).unwrap();
        assert!((cost - 8.5).abs() < 1e-9, "{cost}");
        assert!(p.cost(None, &t).is_none());
    }

    #[test]
    fn override_file_extends_and_replaces() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("prices.json"),
            r#"{"models": {"my-model": {"input": 1, "output": 1}, "gpt-5": {"input": 9, "output": 9}}}"#,
        )
        .unwrap();
        let p = Prices::load(dir.path());
        assert!(p.override_error.is_none());
        assert_eq!(p.price("my-model-2").unwrap().input, 1.0);
        assert_eq!(p.price("gpt-5").unwrap().input, 9.0);
        std::fs::write(dir.path().join("prices.json"), "{oops").unwrap();
        assert!(Prices::load(dir.path()).override_error.is_some());
    }

    #[test]
    fn reports_group_and_fill_empty_days() {
        let p = prices();
        let row = |day: &str, model: &str| UsageRow {
            day: day.into(),
            session_id: "s".into(),
            harness: HarnessId::ClaudeCode,
            model: model.into(),
            repo: Some("/w/shop".into()),
            tokens: Tokens {
                input: 1_000_000,
                output: 0,
                cached: Some(0),
            },
        };
        let rows = [
            row("2026-10-30", "claude-opus-5-5"),
            row("2026-10-31", "mystery"),
        ];
        let r = report(
            &rows,
            vec![
                "2026-10-29".into(),
                "2026-10-30".into(),
                "2026-10-31".into(),
            ],
            &p,
        );
        assert_eq!(r.by_day.len(), 3);
        assert_eq!(r.by_day[0].input, 0);
        assert!((r.total.cost - 4.0).abs() < 1e-9);
        assert_eq!(r.total.unpriced_tokens, 1_000_000);
        assert_eq!(r.unpriced_models, ["mystery"]);
        assert_eq!(r.by_repo.len(), 1);
        assert_eq!(r.by_model[0].key, "claude-opus-5-5");
    }
}

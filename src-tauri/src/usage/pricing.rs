// Public API token-equivalent prices, USD per million tokens, checked 2026-10-23.
pub const PRICE_DATE: &str = "2026-10-23";
pub const POLICY_VERSION: &str = "local-zero-v1";
pub const PRICE_URL: &str = "https://developers.openai.com/api/docs/pricing";

pub fn estimate(
    model: Option<&str>,
    tier: Option<&str>,
    input: u64,
    cached: u64,
    output: u64,
    writes: Option<u64>,
) -> Option<f64> {
    if cached > input || writes.is_some_and(|n| n > input - cached) {
        return None;
    }
    // User accounting policy: retain usage, but do not charge these records locally.
    if model.is_none_or(|name| {
        name.trim().is_empty()
            || matches!(
                name,
                "gpt-reserve" | "codex-auto-review" | "gpt-5.3-codex-spark"
            )
    }) {
        return Some(0.0);
    }
    let model = model?;
    let (input_rate, cache_rate, output_rate, long_context, write_rate) = match model {
        "gpt-6-astra" => (10.0, 1.0, 50.0, true, true),
        "gpt-6.1-sol" | "gpt-6.1-sol-latest" => (2.0, 0.1, 10.0, true, true),
        "gpt-6-sol" => (2.0, 0.2, 10.0, true, true),
        "gpt-6-luna" => (0.1, 0.01, 0.5, true, true),
        "gpt-5.6-sol" => (4.0, 0.4, 20.0, true, true),
        "gpt-5.6-terra" => (2.0, 0.2, 12.0, true, true),
        "gpt-5.6-luna" => (0.2, 0.02, 1.2, true, true),
        "gpt-5.5" => (5.0, 0.5, 30.0, true, false),
        "gpt-5.4" => (2.5, 0.25, 15.0, true, false),
        "gpt-5.4-mini" => (0.75, 0.075, 4.5, false, false),
        "gpt-5.4-nano" => (0.2, 0.02, 1.25, false, false),
        "gpt-5.2" => (1.75, 0.175, 14.0, false, false),
        "gpt-5.1" => (1.25, 0.125, 10.0, false, false),
        "gpt-5" => (1.25, 0.125, 10.0, false, false),
        _ => return None,
    };
    let multiplier = match tier {
        Some("priority" | "fast") => 2.0,
        Some("ultrafast")
            if matches!(model, "gpt-6-astra" | "gpt-6.1-sol" | "gpt-6.1-sol-latest") =>
        {
            6.0
        }
        Some("flex") => 0.5,
        None | Some("default" | "standard" | "auto") => 1.0,
        _ => return None,
    };
    let long = long_context && input > 272000;
    let input_multiplier = if long { 2.0 } else { 1.0 };
    let output_multiplier = if long { 1.5 } else { 1.0 };
    // Cache write TTL is absent in Codex logs; use the published 30m rate as the estimate.
    let write_premium = if write_rate {
        writes.unwrap_or(0) as f64 * input_rate * 0.25
    } else {
        0.0
    };
    Some(
        (((input - cached) as f64 * input_rate + cached as f64 * cache_rate + write_premium)
            * input_multiplier
            + output as f64 * output_rate * output_multiplier)
            * multiplier
            / 1_000_000.0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prices_cached_input_and_already_inclusive_output_without_double_counting() {
        let cost = estimate(
            Some("gpt-6.1-sol"),
            Some("default"),
            100000,
            90000,
            1000,
            Some(0),
        )
        .unwrap();
        assert!((cost - 0.039).abs() < 1e-9);
        assert_eq!(
            estimate(
                Some("gpt-6.1-sol"),
                Some("fast"),
                100000,
                90000,
                1000,
                Some(0)
            ),
            Some(cost * 2.0)
        );
        let long = estimate(Some("gpt-6.1-sol"), None, 300000, 0, 1000, Some(10000)).unwrap();
        assert!((long - 1.225).abs() < 1e-9);
        assert!(estimate(Some("unknown"), None, 100, 0, 10, None).is_none());
        assert!(estimate(Some("gpt-6-sol"), None, 100, 101, 10, None).is_none());
    }
    #[test]
    fn explicitly_zero_rated_models_keep_usage_without_becoming_unpriced() {
        for model in [
            None,
            Some(""),
            Some("gpt-reserve"),
            Some("codex-auto-review"),
            Some("gpt-5.3-codex-spark"),
        ] {
            assert_eq!(
                estimate(model, None, 100, 90, 10, None),
                Some(0.0),
                "{model:?}"
            );
            assert_eq!(
                estimate(model, Some("priority"), 100, 90, 10, None),
                Some(0.0)
            );
        }
        assert!(estimate(Some("unrecognized-model"), None, 100, 90, 10, None).is_none());
        assert!(estimate(None, None, 100, 101, 10, None).is_none());
    }
    #[test]
    #[ignore = "reprices the active existing request cache read-only; never scans or changes logs"]
    fn live_zero_policy_reprices_cached_requests() {
        use rusqlite::{Connection, OpenFlags};
        use serde_json::Value;
        let home = crate::source::codex_home().unwrap();
        let root = home.join(".codex-chat-pane");
        let db = Connection::open_with_flags(
            root.join("usage.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let scope: String = db
            .query_row(
                "SELECT scope FROM quota_derivations WHERE scope LIKE '%|live' LIMIT 1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let identity = scope.strip_suffix("|live").unwrap();
        let (mut requests, mut zeros, mut unpriced) = (0, 0, 0);
        for entry in std::fs::read_dir(root).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy();
            if !name.starts_with("usage-cache-") || !name.ends_with(".sqlite") {
                continue;
            }
            let cache =
                Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
            let saved: String = cache
                .query_row("SELECT value FROM cache_identity LIMIT 1", [], |r| r.get(0))
                .unwrap();
            if saved != identity {
                continue;
            }
            let mut stmt = cache.prepare("SELECT data FROM usage_requests").unwrap();
            for body in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap() {
                let v: Value = serde_json::from_str(&body.unwrap()).unwrap();
                let model = v["model"].as_str();
                let tier = v["tier"].as_str();
                let cost = estimate(
                    model,
                    tier,
                    v["input"].as_u64().unwrap(),
                    v["cached"].as_u64().unwrap(),
                    v["output"].as_u64().unwrap(),
                    v["writes"].as_u64(),
                );
                requests += 1;
                if model.is_none_or(|m| {
                    matches!(
                        m,
                        "gpt-reserve" | "codex-auto-review" | "gpt-5.3-codex-spark"
                    )
                }) {
                    assert_eq!(cost, Some(0.0));
                    zeros += 1;
                }
                unpriced += usize::from(cost.is_none());
            }
        }
        assert!(requests > 0 && zeros > 0);
        assert_eq!(
            unpriced, 0,
            "current cached model families should all be accounted for"
        );
        println!(
            "cached_requests={requests} zero_policy_requests={zeros} remaining_unpriced={unpriced}"
        );
    }
}

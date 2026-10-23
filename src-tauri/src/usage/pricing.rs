// Public API token-equivalent prices, USD per million tokens, checked 2026-10-23.
pub const PRICE_DATE: &str = "2026-10-23";
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
}

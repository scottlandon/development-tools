//! API-equivalent cost estimation. Prices are $/million tokens, matched by
//! substring against the model id (first match wins). Subscription plans don't
//! bill per token, so treat the result as "what this would cost on the API",
//! not an invoice.

/// When the built-in table was last checked against Anthropic's published pricing
/// (<https://platform.claude.com/docs/en/about-claude/pricing>). Shown in `--help` and the `?` overlay
/// so a stale table is visible. Bump it whenever you re-verify the entries below.
pub const PRICES_VERIFIED: &str = "2026-11-06";

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Price {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub write_5m: f64,
    pub write_1h: f64,
    /// `(threshold, multiplier)`: a message whose whole prompt (input + cache reads + cache writes)
    /// exceeds `threshold` tokens bills every rate at `multiplier`x. Haiku 5.5 is tiered this way.
    pub long_prompt: Option<(u64, f64)>,
}

impl Price {
    /// Standard multipliers: 5-minute cache writes cost 1.25x input and
    /// 1-hour writes 2x input. Cache reads differ per model, so are explicit.
    const fn new(input: f64, output: f64, cache_read: f64) -> Self {
        Price {
            input,
            output,
            cache_read,
            write_5m: input * 1.25,
            write_1h: input * 2.0,
            long_prompt: None,
        }
    }

    const fn long_prompt(mut self, threshold: u64, multiplier: f64) -> Self {
        self.long_prompt = Some((threshold, multiplier));
        self
    }

    pub fn cost(&self, i: u64, o: u64, cr: u64, cw5: u64, cw1: u64) -> f64 {
        let tier = match self.long_prompt {
            Some((threshold, mult)) if i + cr + cw5 + cw1 > threshold => mult,
            _ => 1.0,
        };
        tier * (i as f64 * self.input
            + o as f64 * self.output
            + cr as f64 * self.cache_read
            + cw5 as f64 * self.write_5m
            + cw1 as f64 * self.write_1h)
            / 1_000_000.0
    }
}

/// Order matters: most specific patterns first.
const TABLE: &[(&str, Price)] = &[
    ("fable-5-2", Price::new(8.0, 40.0, 0.20)),
    ("fable-5-1", Price::new(10.0, 50.0, 0.25)),
    ("fable", Price::new(10.0, 50.0, 1.0)),
    ("mythos-5-2", Price::new(8.0, 40.0, 0.20)),
    ("mythos-5-1", Price::new(10.0, 50.0, 0.25)),
    ("mythos", Price::new(10.0, 50.0, 1.0)),
    ("opus-5-5", Price::new(4.0, 20.0, 0.20)),
    ("opus-4-1", Price::new(15.0, 75.0, 1.50)),
    ("opus-4-2025", Price::new(15.0, 75.0, 1.50)),
    ("3-opus", Price::new(15.0, 75.0, 1.50)),
    ("opus", Price::new(5.0, 25.0, 0.50)),
    ("sonnet-5-5", Price::new(2.0, 10.0, 0.10)),
    ("sonnet-5", Price::new(2.0, 10.0, 0.20)),
    ("sonnet", Price::new(3.0, 15.0, 0.30)),
    ("haiku-5", Price::new(0.10, 0.50, 0.01).long_prompt(100_000, 5.0)),
    ("haiku-4", Price::new(1.0, 5.0, 0.10)),
    ("3-5-haiku", Price::new(0.80, 4.0, 0.08)),
    ("haiku", Price::new(0.25, 1.25, 0.03)),
];

pub fn lookup(model: &str) -> Option<Price> {
    TABLE.iter().find(|(pat, _)| model.contains(pat)).map(|(_, p)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_most_specific() {
        assert_eq!(lookup("claude-opus-5-5").unwrap().input, 4.0);
        assert_eq!(lookup("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(lookup("claude-opus-4-8").unwrap().input, 5.0);
        assert_eq!(lookup("claude-opus-4-1-20250805").unwrap().input, 15.0);
        assert_eq!(lookup("claude-sonnet-5-5").unwrap().input, 2.0);
        assert_eq!(lookup("claude-sonnet-4-6").unwrap().input, 3.0);
        assert_eq!(lookup("claude-haiku-5-5").unwrap().input, 0.10);
        assert_eq!(lookup("claude-haiku-4-5-20251001").unwrap().input, 1.0);
        assert_eq!(lookup("claude-fable-5-1").unwrap().cache_read, 0.25);
        for m in ["claude-fable-5-2", "claude-mythos-5-2"] {
            let p = lookup(m).unwrap();
            assert_eq!((p.input, p.output), (8.0, 40.0), "{m}");
        }
        assert_eq!(lookup("claude-fable-5").unwrap().input, 10.0);
        assert_eq!(lookup("claude-mythos-5").unwrap().input, 10.0);
        assert!(lookup("gpt-9").is_none());
    }

    /// Spot-checks against the published table, including the cache-read rates that differ from 0.1x.
    #[test]
    fn matches_published_cache_rates() {
        assert_eq!(lookup("claude-sonnet-5-5").unwrap().cache_read, 0.10);
        assert_eq!(lookup("claude-sonnet-5").unwrap().cache_read, 0.20);
        assert_eq!(lookup("claude-mythos-5-1").unwrap().cache_read, 0.25);
        assert_eq!(lookup("claude-mythos-5").unwrap().cache_read, 1.0);
        for m in ["claude-fable-5-2", "claude-mythos-5-2"] {
            let p = lookup(m).unwrap();
            assert_eq!(p.cache_read, 0.20, "{m}");
            assert_eq!((p.write_5m, p.write_1h), (10.0, 16.0), "{m}");
        }
        let p = lookup("claude-sonnet-5-5").unwrap();
        assert_eq!((p.write_5m, p.write_1h), (2.5, 4.0));
    }

    #[test]
    fn haiku_long_prompts_bill_at_the_higher_tier() {
        let p = lookup("claude-haiku-5-5").unwrap();
        // 100k prompt exactly: base tier. One token over: 5x on everything.
        let base = p.cost(100_000, 1_000, 0, 0, 0);
        let long = p.cost(100_001, 1_000, 0, 0, 0);
        assert!((base - (100_000.0 * 0.10 + 1_000.0 * 0.50) / 1e6).abs() < 1e-12);
        assert!((long - 5.0 * (100_001.0 * 0.10 + 1_000.0 * 0.50) / 1e6).abs() < 1e-12);
        // Cache reads count towards the prompt length.
        assert!((p.cost(10, 0, 150_000, 0, 0) - 5.0 * (10.0 * 0.10 + 150_000.0 * 0.01) / 1e6).abs() < 1e-12);
        // Other models are never tiered.
        let s = lookup("claude-sonnet-4-6").unwrap();
        assert!((s.cost(500_000, 0, 0, 0, 0) - 1.5).abs() < 1e-9);
    }

    #[test]
    fn cost_math() {
        let p = Price::new(2.0, 10.0, 0.2);
        // 1M of each: 2 + 10 + 0.2 + 2.5 + 4
        let c = p.cost(1_000_000, 1_000_000, 1_000_000, 1_000_000, 1_000_000);
        assert!((c - 18.7).abs() < 1e-9);
    }

    #[test]
    fn haiku_prompt_length_counts_every_input_kind() {
        let p = lookup("claude-haiku-5-5").unwrap();
        // 1 + 60_000 + 60_000 = 120_001 prompt tokens, split across cache writes only.
        let long = p.cost(1, 0, 0, 60_000, 60_000);
        let base = (1.0 * 0.10 + 60_000.0 * 0.125 + 60_000.0 * 0.20) / 1e6;
        assert!((long - 5.0 * base).abs() < 1e-12, "{long} vs {}", 5.0 * base);
        // 99_999 in total is still the base tier.
        let short = p.cost(1, 0, 0, 49_999, 50_000);
        assert!((short - (0.10 + 49_999.0 * 0.125 + 50_000.0 * 0.20) / 1e6).abs() < 1e-12);
    }
}

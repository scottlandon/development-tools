//! Synthetic usage data for `--demo` and the README screenshots. Everything here is invented:
//! project names, token counts and timings come from a fixed-seed generator, so a demo never
//! reads (or can leak) a real transcript.

use crate::cast;
use crate::data::{Dataset, ModelInfo, Row, short_model};
use crate::pricing;
use crate::timeutil::{self, LocalDays};

/// Days of history to fabricate.
const DAYS: i32 = 75;

const PROJECTS: [(&str, f64); 5] = [
    ("acme-api", 0.34),
    ("storefront-web", 0.26),
    ("data-pipeline", 0.19),
    ("infra-scripts", 0.12),
    ("docs-site", 0.09),
];

/// (model id, weight). Heavier models are rarer.
const MODELS: [(&str, f64); 4] = [
    ("claude-sonnet-5-5", 0.52),
    ("claude-opus-5-5", 0.30),
    ("claude-haiku-5-5", 0.14),
    ("claude-fable-5-1", 0.04),
];

/// xorshift64*: tiny, fast and, above all, reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in `[0, 1)`.
    fn f(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn between(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.f()
    }
    fn pick(&mut self, weights: &[f64]) -> usize {
        let total: f64 = weights.iter().sum();
        let mut x = self.f() * total;
        for (i, w) in weights.iter().enumerate() {
            if x < *w {
                return i;
            }
            x -= w;
        }
        weights.len() - 1
    }
}

struct Builder {
    rng: Rng,
    rows: Vec<Row>,
    models: Vec<ModelInfo>,
    days: LocalDays,
    session: u32,
    now: i64,
}

impl Builder {
    /// One burst of messages in a single session: a message every 20-150 s from `start` to `end`.
    fn block(&mut self, start: i64, end: i64, project: usize, ramp: f64) {
        let session = self.session;
        self.session += 1;
        // Each session leans on one model, with the odd detour (a haiku sub-agent, say).
        let main = self.rng.pick(&MODELS.map(|m| m.1));
        let mut t = start;
        while t < end.min(self.now) {
            let model = if self.rng.f() < 0.8 {
                main
            } else {
                self.rng.pick(&MODELS.map(|m| m.1))
            };
            let heavy = [1.0, 1.6, 0.5, 2.0][model];
            let o = cast::f64_to_u32(self.rng.between(180.0, 2400.0) * heavy * ramp);
            let i = cast::f64_to_u32(self.rng.between(2.0, 900.0));
            let cr = cast::f64_to_u32(self.rng.between(25_000.0, 190_000.0) * ramp);
            let cw5 = if self.rng.f() < 0.35 {
                cast::f64_to_u32(self.rng.between(500.0, 14_000.0))
            } else {
                0
            };
            let price = self.models[model].price;
            self.rows.push(Row {
                ts: t,
                day: self.days.day(t),
                model: cast::to_u32(model),
                project: cast::to_u32(project),
                session,
                i,
                o,
                cr,
                cw5,
                cw1: 0,
                cost: price.map_or(0.0, |p| {
                    p.cost(u64::from(i), u64::from(o), u64::from(cr), u64::from(cw5), 0)
                }),
                priced: price.is_some(),
                fast: false,
            });
            t += cast::f64_to_i64(self.rng.between(20.0, 150.0));
        }
    }
}

/// A believable ~11 weeks of usage ending at `now` (UTC epoch seconds).
pub fn dataset(now: i64) -> Dataset {
    let models: Vec<ModelInfo> = MODELS
        .iter()
        .map(|(n, _)| ModelInfo {
            name: (*n).to_string(),
            short: short_model(n),
            price: pricing::lookup(n),
        })
        .collect();
    let mut b = Builder {
        rng: Rng(0x9E37_79B9_7F4A_7C15),
        rows: Vec::new(),
        models,
        days: LocalDays::default(),
        session: 0,
        now,
    };

    let today = b.days.day(now);
    let project_w = PROJECTS.map(|p| p.1);
    for age in (0..DAYS).rev() {
        let day = today - age;
        let weekend = matches!(timeutil::weekday(i64::from(day)), 0 | 6);
        if b.rng.f() < if weekend { 0.7 } else { 0.06 } {
            continue; // a day off
        }
        // Usage creeps up over the period, with a good day now and then.
        let trend = 0.55 + 0.45 * (1.0 - f64::from(age) / f64::from(DAYS));
        let ramp = trend * b.rng.between(0.6, 1.5);
        let midnight = i64::from(day) * 86_400 - b.days.offset_secs(i64::from(day) * 86_400 + 43_200);
        for _ in 0..cast::f64_to_u32(b.rng.between(1.0, 4.0)) {
            let start = midnight + cast::f64_to_i64(b.rng.between(8.5, 19.0) * 3600.0);
            let end = start + cast::f64_to_i64(b.rng.between(25.0, 140.0) * 60.0);
            let project = b.rng.pick(&project_w);
            b.block(start, end, project, ramp);
        }
    }

    // Whatever the wall clock says, keep the Live view lively: a session that's still going.
    b.block(now - 100 * 60, now - 8, 0, 1.0);

    let Builder {
        mut rows,
        models,
        session,
        ..
    } = b;
    rows.sort_unstable_by_key(|r| r.ts);
    Dataset {
        rows,
        models,
        projects: PROJECTS.iter().map(|p| p.0.to_string()).collect(),
        project_keys: PROJECTS.iter().map(|p| format!("-demo-{}", p.0)).collect(),
        sessions: session,
        files: session as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    #[test]
    fn is_deterministic() {
        let (a, b) = (dataset(NOW), dataset(NOW));
        assert_eq!(a.rows.len(), b.rows.len());
        assert!(
            a.rows
                .iter()
                .zip(&b.rows)
                .all(|(x, y)| x.ts == y.ts && x.o == y.o && x.cr == y.cr)
        );
    }

    #[test]
    fn is_plausible_and_never_in_the_future() {
        let ds = dataset(NOW);
        assert!(ds.rows.len() > 2_000);
        assert!(ds.rows.windows(2).all(|w| w[0].ts <= w[1].ts));
        assert!(ds.rows.iter().all(|r| r.ts <= NOW));
        assert!(ds.rows.iter().all(|r| r.priced && r.cost > 0.0));
        assert!(ds.rows.iter().all(|r| (r.project as usize) < ds.projects.len()));
        assert!(ds.rows.iter().all(|r| r.session < ds.sessions));
        // The Live view needs recent traffic.
        assert!(ds.rows.iter().filter(|r| r.ts > NOW - 3600).count() > 20);
    }

    #[test]
    fn only_invented_names() {
        let ds = dataset(NOW);
        assert!(ds.projects.iter().all(|p| PROJECTS.iter().any(|(n, _)| n == p)));
    }
}

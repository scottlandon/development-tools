//! Aggregations over the row table for a given filter. One pass produces
//! everything the views need, so switching tabs never rescans.

use crate::cast;
use crate::data::{Dataset, Row};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Range {
    Today,
    Week,
    Month,
    Quarter,
    All,
}

impl Range {
    pub const ALL: [Range; 5] = [Range::Today, Range::Week, Range::Month, Range::Quarter, Range::All];

    pub fn days(self) -> Option<i32> {
        match self {
            Range::Today => Some(1),
            Range::Week => Some(7),
            Range::Month => Some(30),
            Range::Quarter => Some(90),
            Range::All => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Range::Today => "Today",
            Range::Week => "7 days",
            Range::Month => "30 days",
            Range::Quarter => "90 days",
            Range::All => "All time",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            Range::Today => "1d",
            Range::Week => "7d",
            Range::Month => "30d",
            Range::Quarter => "90d",
            Range::All => "all",
        }
    }
    pub fn next(self) -> Range {
        let i = Range::ALL.iter().position(|r| *r == self).unwrap_or(0);
        Range::ALL[(i + 1) % Range::ALL.len()]
    }
}

/// Sliding window shown by the Live view. Always `LIVE_BUCKETS` bars, so the
/// bar width is window / 60: 15 s, 1 min, 6 min, 24 min.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LiveWindow {
    M15,
    #[default]
    H1,
    H6,
    H24,
}

pub const LIVE_BUCKETS: usize = 60;

impl LiveWindow {
    pub const ALL: [LiveWindow; 4] = [LiveWindow::M15, LiveWindow::H1, LiveWindow::H6, LiveWindow::H24];

    pub fn secs(self) -> i64 {
        match self {
            LiveWindow::M15 => 15 * 60,
            LiveWindow::H1 => 3600,
            LiveWindow::H6 => 6 * 3600,
            LiveWindow::H24 => 24 * 3600,
        }
    }
    pub fn step(self) -> i64 {
        self.secs() / cast::to_i64(LIVE_BUCKETS)
    }
    pub fn label(self) -> &'static str {
        match self {
            LiveWindow::M15 => "15 min",
            LiveWindow::H1 => "1 hour",
            LiveWindow::H6 => "6 hours",
            LiveWindow::H24 => "24 hours",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            LiveWindow::M15 => "15m",
            LiveWindow::H1 => "1h",
            LiveWindow::H6 => "6h",
            LiveWindow::H24 => "24h",
        }
    }
    fn shift(self, by: usize) -> LiveWindow {
        let i = LiveWindow::ALL.iter().position(|w| *w == self).unwrap_or(0);
        LiveWindow::ALL[(i + by) % LiveWindow::ALL.len()]
    }
    pub fn next(self) -> LiveWindow {
        self.shift(1)
    }
    pub fn prev(self) -> LiveWindow {
        self.shift(LiveWindow::ALL.len() - 1)
    }
}

#[derive(Clone, Debug, Default)]
pub struct LiveBucket {
    /// Epoch seconds at the start of the bucket.
    pub start: i64,
    pub agg: Agg,
    pub models: Vec<(u16, Agg)>,
}

#[derive(Clone, Debug, Default)]
pub struct Live {
    pub window: LiveWindow,
    pub step: i64,
    pub buckets: Vec<LiveBucket>,
    pub total: Agg,
    /// Last 5 minutes (for the "now" rate) and last 15 minutes (for $/hr).
    pub recent: Agg,
    pub quarter: Agg,
    /// Seconds since the newest message inside the window.
    pub last_msg_ago: Option<i64>,
}

/// Bucket usage into a trailing window ending at `now` (the final bar is the
/// still-filling current one). Respects model/project filters, ignores range.
pub fn compute_live(ds: &Dataset, f: Filter, window: LiveWindow, now: i64) -> Live {
    let step = window.step();
    let end = (now.div_euclid(step) + 1) * step;
    let start = end - step * cast::to_i64(LIVE_BUCKETS);
    let mut live = Live {
        window,
        step,
        buckets: (0..cast::to_i64(LIVE_BUCKETS))
            .map(|i| LiveBucket {
                start: start + i * step,
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let lo = ds.rows.partition_point(|r| r.ts < start);
    let mut newest: Option<i64> = None;
    for r in &ds.rows[lo..] {
        if f.model.is_some_and(|m| m != r.model) || f.project.is_some_and(|p| p != r.project) {
            continue;
        }
        // Rows stamped slightly in the future (clock skew) land in the last bar.
        let idx = usize::try_from((r.ts - start) / step)
            .unwrap_or(LIVE_BUCKETS - 1)
            .min(LIVE_BUCKETS - 1);
        let b = &mut live.buckets[idx];
        b.agg.add(r);
        if let Some((_, a)) = b.models.iter_mut().find(|(m, _)| *m == r.model) {
            a.add(r);
        } else {
            let mut a = Agg::default();
            a.add(r);
            b.models.push((r.model, a));
        }
        live.total.add(r);
        if r.ts >= now - 300 {
            live.recent.add(r);
        }
        if r.ts >= now - 900 {
            live.quarter.add(r);
        }
        newest = Some(newest.map_or(r.ts, |n| n.max(r.ts)));
    }
    live.last_msg_ago = newest.map(|t| (now - t).max(0));
    live
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Filter {
    pub range_days: Option<i32>,
    pub model: Option<u16>,
    pub project: Option<u16>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Agg {
    pub msgs: u64,
    pub i: u64,
    pub o: u64,
    pub cr: u64,
    pub cw: u64,
    pub cost: f64,
    /// Messages whose model has no known price (excluded from `cost`).
    pub unpriced: u64,
}

impl Agg {
    pub fn add(&mut self, r: &Row) {
        self.msgs += 1;
        self.i += u64::from(r.i);
        self.o += u64::from(r.o);
        self.cr += u64::from(r.cr);
        self.cw += u64::from(r.cw5 + r.cw1);
        self.cost += r.cost;
        self.unpriced += u64::from(!r.priced);
    }
    pub fn total(&self) -> u64 {
        self.i + self.o + self.cr + self.cw
    }
    /// Share of prompt tokens served from cache.
    pub fn cache_hit(&self) -> Option<f64> {
        let denom = self.cr + self.cw + self.i;
        (denom > 0).then(|| self.cr as f64 / denom as f64)
    }
}

#[derive(Clone, Debug)]
pub struct DayAgg {
    pub day: i32,
    pub agg: Agg,
    /// Per-model breakdown, sorted by model id.
    pub models: Vec<(u16, Agg)>,
}

#[derive(Clone, Debug)]
pub struct SessionAgg {
    pub project: u16,
    pub first: i64,
    pub last: i64,
    pub agg: Agg,
    /// Model with the most cost/tokens in this session.
    pub top_model: u16,
    models: Vec<(u16, u64)>,
}

#[derive(Default, Debug)]
pub struct Summary {
    pub today: i32,
    pub days: Vec<DayAgg>,
    pub total: Agg,
    pub by_model: Vec<(u16, Agg)>,
    pub by_project: Vec<(u16, Agg)>,
    pub sessions: Vec<SessionAgg>,
    /// Models ordered by usage (for stable chart colours).
    pub model_rank: Vec<u16>,
}

#[expect(
    clippy::too_many_lines,
    reason = "single pass over the rows that builds every summary table"
)]
pub fn compute(ds: &Dataset, f: Filter, today: i32) -> Summary {
    let start_day = f.range_days.map(|n| today - n + 1);
    let mut by_day: HashMap<i32, DayAgg> = HashMap::new();
    let mut by_model: HashMap<u16, Agg> = HashMap::new();
    let mut by_project: HashMap<u16, Agg> = HashMap::new();
    let mut sess: HashMap<u32, SessionAgg> = HashMap::new();
    let mut total = Agg::default();

    for r in &ds.rows {
        if start_day.is_some_and(|s| r.day < s) || r.day > today {
            continue;
        }
        let model_ok = f.model.is_none_or(|m| m == r.model);
        let proj_ok = f.project.is_none_or(|p| p == r.project);
        // Each breakdown ignores its own filter so you can still see the alternatives.
        if proj_ok {
            by_model.entry(r.model).or_default().add(r);
        }
        if model_ok {
            by_project.entry(r.project).or_default().add(r);
        }
        if !(model_ok && proj_ok) {
            continue;
        }
        total.add(r);
        let d = by_day.entry(r.day).or_insert_with(|| DayAgg {
            day: r.day,
            agg: Agg::default(),
            models: Vec::new(),
        });
        d.agg.add(r);
        if let Some((_, a)) = d.models.iter_mut().find(|(m, _)| *m == r.model) {
            a.add(r);
        } else {
            let mut a = Agg::default();
            a.add(r);
            d.models.push((r.model, a));
        }
        let s = sess.entry(r.session).or_insert_with(|| SessionAgg {
            project: r.project,
            first: r.ts,
            last: r.ts,
            agg: Agg::default(),
            top_model: r.model,
            models: Vec::new(),
        });
        s.first = s.first.min(r.ts);
        s.last = s.last.max(r.ts);
        s.agg.add(r);
        let w = u64::from(r.i) + u64::from(r.o) + u64::from(r.cw5) + u64::from(r.cw1) + u64::from(r.cr) / 10;
        match s.models.iter_mut().find(|(m, _)| *m == r.model) {
            Some((_, t)) => *t += w,
            None => s.models.push((r.model, w)),
        }
    }

    // Contiguous day axis, including empty days, so gaps are visible.
    let first_day = match start_day {
        Some(s) => Some(s),
        None => by_day.keys().min().copied(),
    };
    let mut days = Vec::new();
    if let Some(first) = first_day {
        for d in first..=today {
            days.push(
                by_day
                    .remove(&d)
                    .map(|mut x| {
                        x.models.sort_by_key(|(m, _)| *m);
                        x
                    })
                    .unwrap_or(DayAgg {
                        day: d,
                        agg: Agg::default(),
                        models: Vec::new(),
                    }),
            );
        }
    }

    let sort_desc = |v: &mut Vec<(u16, Agg)>| {
        v.sort_by(|a, b| {
            b.1.cost
                .partial_cmp(&a.1.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(b.1.total().cmp(&a.1.total()))
        });
    };
    let mut by_model: Vec<_> = by_model.into_iter().collect();
    let mut by_project: Vec<_> = by_project.into_iter().collect();
    sort_desc(&mut by_model);
    sort_desc(&mut by_project);

    let mut sessions: Vec<SessionAgg> = sess
        .into_values()
        .map(|mut s| {
            if let Some((m, _)) = s.models.iter().max_by_key(|(_, w)| *w) {
                s.top_model = *m;
            }
            s
        })
        .collect();
    sessions.sort_by_key(|s| std::cmp::Reverse(s.last));

    let model_rank = by_model.iter().map(|(m, _)| *m).collect();
    Summary {
        today,
        days,
        total,
        by_model,
        by_project,
        sessions,
        model_rank,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ModelInfo;

    fn row(day: i32, model: u16, project: u16, o: u32, cost: f64) -> Row {
        Row {
            ts: i64::from(day) * 86_400,
            day,
            model,
            project,
            session: u32::from(project),
            i: 1,
            o,
            cr: 10,
            cw5: 2,
            cw1: 3,
            cost,
            priced: true,
        }
    }

    fn ds() -> Dataset {
        Dataset {
            rows: vec![
                row(100, 0, 0, 5, 1.0),
                row(100, 1, 1, 7, 2.0),
                row(102, 0, 1, 9, 4.0),
                row(50, 0, 0, 1, 9.0),
            ],
            models: vec![
                ModelInfo {
                    short: "a".into(),
                    price: None,
                },
                ModelInfo {
                    short: "b".into(),
                    price: None,
                },
            ],
            projects: vec!["p0".into(), "p1".into()],
            sessions: 2,
            files: 2,
        }
    }

    #[test]
    fn live_window_buckets_rates_and_filters() {
        let now = 120_030; // 30 s into a minute-aligned bucket
        let at = |age: i64, model: u16, o: u32| {
            let mut r = row(0, model, 0, o, 1.0);
            r.ts = now - age;
            r
        };
        let mut d = ds();
        d.rows = vec![at(4000, 0, 99), at(70, 1, 7), at(10, 0, 5), at(-5, 0, 3)]; // oldest first; one "future" row
        d.rows.sort_by_key(|r| r.ts);
        let l = compute_live(&d, Filter::default(), LiveWindow::H1, now);
        assert_eq!(l.buckets.len(), LIVE_BUCKETS);
        assert_eq!(l.step, 60);
        assert_eq!(l.total.msgs, 3, "the 4000 s old row is outside a 1 h window");
        assert_eq!(l.buckets[58].agg.msgs, 1); // 70 s ago
        assert_eq!(l.buckets[59].agg.msgs, 2); // 10 s ago + clamped future row
        assert_eq!(l.buckets[59].start + l.step, (now / 60 + 1) * 60);
        assert_eq!(l.recent.msgs, 3, "70 s, 10 s and the skewed row are all within 5 min");
        assert_eq!(l.last_msg_ago, Some(0));
        // The model filter applies; the long window sees the old row too.
        let f = Filter {
            model: Some(0),
            ..Default::default()
        };
        assert_eq!(compute_live(&d, f, LiveWindow::H1, now).total.msgs, 2);
        assert_eq!(compute_live(&d, Filter::default(), LiveWindow::H6, now).total.msgs, 4);
        // Nothing recent -> no "last message".
        d.rows.clear();
        assert_eq!(
            compute_live(&d, Filter::default(), LiveWindow::M15, now).last_msg_ago,
            None
        );
    }

    #[test]
    fn live_window_cycles() {
        assert_eq!(LiveWindow::H24.next(), LiveWindow::M15);
        assert_eq!(LiveWindow::M15.prev(), LiveWindow::H24);
        for w in LiveWindow::ALL {
            assert_eq!(w.step() * cast::to_i64(LIVE_BUCKETS), w.secs());
        }
    }

    #[test]
    fn windowing_and_gap_fill() {
        let s = compute(
            &ds(),
            Filter {
                range_days: Some(5),
                ..Default::default()
            },
            102,
        );
        assert_eq!(s.days.len(), 5);
        assert_eq!(
            s.days.iter().map(|d| d.day).collect::<Vec<_>>(),
            vec![98, 99, 100, 101, 102]
        );
        assert_eq!(s.total.msgs, 3, "old row excluded");
        assert!((s.total.cost - 7.0).abs() < 1e-9);
        assert_eq!(s.days[3].agg.msgs, 0, "empty day kept");
    }

    #[test]
    fn all_time_starts_at_first_row() {
        let s = compute(&ds(), Filter::default(), 102);
        assert_eq!(s.days.first().unwrap().day, 50);
        assert_eq!(s.days.last().unwrap().day, 102);
    }

    #[test]
    fn breakdowns_ignore_their_own_filter() {
        let f = Filter {
            range_days: Some(10),
            model: Some(0),
            project: None,
        };
        let s = compute(&ds(), f, 102);
        assert_eq!(s.total.msgs, 2);
        assert_eq!(s.by_model.len(), 2, "model table still lists every model");
        assert_eq!(s.by_project.len(), 2);
        assert_eq!(s.by_project.iter().map(|(_, a)| a.msgs).sum::<u64>(), 2);
    }

    #[test]
    fn totals_split_cache_types() {
        let s = compute(
            &ds(),
            Filter {
                range_days: Some(1),
                ..Default::default()
            },
            102,
        );
        assert_eq!((s.total.i, s.total.o, s.total.cr, s.total.cw), (1, 9, 10, 5));
        assert_eq!(s.total.total(), 25);
        assert!((s.total.cache_hit().unwrap() - 10.0 / 16.0).abs() < 1e-9);
    }

    #[test]
    fn live_windows_are_consistent() {
        let labels: std::collections::HashSet<_> = LiveWindow::ALL.iter().map(|w| w.label()).collect();
        let shorts: std::collections::HashSet<_> = LiveWindow::ALL.iter().map(|w| w.short()).collect();
        assert_eq!((labels.len(), shorts.len()), (4, 4), "every window has its own name");
        for w in LiveWindow::ALL {
            assert_eq!(w.step() * cast::to_i64(LIVE_BUCKETS), w.secs(), "{w:?}: 60 equal bars");
            assert_eq!(w.next().prev(), w);
            assert_eq!(w.prev().next(), w);
        }
        assert_eq!(LiveWindow::M15.secs(), 900);
        assert_eq!(LiveWindow::H24.secs(), 86_400);
        assert_eq!(LiveWindow::H24.next(), LiveWindow::M15, "wraps");
    }

    fn models_ab() -> Vec<ModelInfo> {
        ["a", "b"]
            .map(|s| ModelInfo {
                short: s.into(),
                price: None,
            })
            .to_vec()
    }

    fn dataset_of(rows: Vec<Row>) -> Dataset {
        Dataset {
            rows,
            models: models_ab(),
            projects: vec!["p0".into(), "p1".into(), "p2".into()],
            sessions: 3,
            files: 1,
        }
    }

    #[test]
    fn filters_narrow_the_totals_but_each_breakdown_ignores_its_own_filter() {
        // rows: (day 100, m0, p0) (100, m1, p1) (102, m0, p1) (50, m0, p0)
        let d = ds();
        let none = Filter::default();
        let all = compute(&d, none, 102);

        let only_m0 = compute(&d, Filter { model: Some(0), ..none }, 102);
        assert_eq!(
            only_m0.total.msgs, 3,
            "m0 has three rows (days 50, 100, 102) and the default range is 'all'"
        );
        assert_eq!(
            only_m0.by_model.len(),
            all.by_model.len(),
            "model list still shows every model"
        );

        let only_p1 = compute(
            &d,
            Filter {
                project: Some(1),
                ..none
            },
            102,
        );
        assert_eq!(only_p1.total.msgs, 2, "rows 2 and 3");
        assert_eq!(
            only_p1.by_project.len(),
            all.by_project.len(),
            "project list still shows every project"
        );
        let p0 = only_p1.by_project.iter().find(|(p, _)| *p == 0).unwrap().1.msgs;
        assert_eq!(p0, all.by_project.iter().find(|(p, _)| *p == 0).unwrap().1.msgs);

        let both = compute(
            &d,
            Filter {
                model: Some(0),
                project: Some(1),
                ..none
            },
            102,
        );
        assert_eq!(both.total.msgs, 1, "only (102, m0, p1)");
        assert_eq!(both.days.iter().filter(|x| x.agg.msgs > 0).count(), 1);
        // The model list is narrowed by the project filter, and the project list by the model filter.
        assert_eq!(
            both.by_model.iter().map(|(_, a)| a.msgs).sum::<u64>(),
            2,
            "project filter applies to models"
        );
        assert_eq!(
            both.by_project.iter().map(|(_, a)| a.msgs).sum::<u64>(),
            3,
            "model filter applies to projects (m0 has three rows)"
        );
    }

    #[test]
    fn a_day_sums_rows_of_the_same_model_into_one_entry() {
        let d = dataset_of(vec![
            row(100, 0, 0, 5, 1.0),
            row(100, 0, 0, 6, 1.0),
            row(100, 1, 0, 7, 1.0),
        ]);
        let s = compute(&d, Filter::default(), 100);
        let day = s.days.last().unwrap();
        assert_eq!(day.models.len(), 2, "one entry per model, not per row");
        assert_eq!(day.models[0].0, 0);
        assert_eq!((day.models[0].1.msgs, day.models[0].1.o), (2, 11));
        assert_eq!((day.models[1].1.msgs, day.models[1].1.o), (1, 7));
        assert_eq!(day.agg.msgs, 3);
    }

    #[test]
    fn a_sessions_top_model_is_the_one_with_the_most_weighted_tokens() {
        // Weight = input + output + cache writes + cache reads / 10 (reads are cheap, so they count for less).
        let mut rows = Vec::new();
        let with = |day, model, project, o, cr| {
            let mut r = row(day, model, project, o, 1.0);
            r.cr = cr;
            r
        };
        // Session of project 0: big output on m0 beats a huge cache read on m1.
        rows.push(with(100, 0, 0, 20_000, 10));
        rows.push(with(100, 1, 0, 0, 100_000));
        // Session of project 1: m1's cache reads (100_000 / 10 = 10_000) outweigh m0's 5_000 output.
        rows.push(with(100, 0, 1, 5_000, 10));
        rows.push(with(100, 1, 1, 0, 100_000));
        // Session of project 2: m0 wins on the sum of three modest rows, though m1's single row is larger.
        for _ in 0..3 {
            rows.push(with(100, 0, 2, 400, 0));
        }
        rows.push(with(100, 1, 2, 1_100, 0));
        let s = compute(&dataset_of(rows), Filter::default(), 100);
        let top = |project: u16| s.sessions.iter().find(|x| x.project == project).unwrap().top_model;
        assert_eq!(top(0), 0);
        assert_eq!(top(1), 1);
        assert_eq!(top(2), 0, "weights add up per model across a session's rows");
    }

    #[test]
    fn cache_hit_needs_prompt_tokens() {
        assert_eq!(Agg::default().cache_hit(), None);
        let only_output = Agg {
            o: 500,
            ..Default::default()
        };
        assert_eq!(only_output.cache_hit(), None, "no prompt tokens, no ratio");
        let all_cached = Agg {
            cr: 10,
            ..Default::default()
        };
        assert_eq!(all_cached.cache_hit(), Some(1.0));
    }

    fn live_row(ts: i64, model: u16, project: u16, o: u32) -> Row {
        Row {
            ts,
            day: 0,
            model,
            project,
            session: 0,
            i: 1,
            o,
            cr: 0,
            cw5: 0,
            cw1: 0,
            cost: 0.5,
            priced: true,
        }
    }

    // H1 window: 60 s bars. now = 6_000_030 -> the current bar is [6_000_000, 6_000_060),
    // the window is [5_996_460, 6_000_060).
    const NOW: i64 = 6_000_030;
    const START: i64 = 5_996_460;

    #[test]
    fn live_buckets_cover_the_window_and_bin_rows_exactly() {
        let rows = vec![
            live_row(START - 1, 0, 0, 1),  // just before the window: ignored
            live_row(START, 0, 0, 2),      // first instant: bar 0
            live_row(START + 59, 1, 0, 3), // last instant of bar 0
            live_row(START + 60, 0, 0, 4), // bar 1
            live_row(NOW - 301, 0, 0, 5),  // 5 min + 1 s ago
            live_row(NOW - 300, 0, 0, 6),  // exactly 5 min ago: counts as "recent"
            live_row(NOW - 299, 0, 0, 7),
            live_row(NOW + 500, 0, 0, 8), // future (clock skew): lands in the last bar
        ];
        let mut rows = rows;
        rows.sort_by_key(|r| r.ts);
        let d = dataset_of(rows);
        let l = compute_live(&d, Filter::default(), LiveWindow::H1, NOW);

        assert_eq!((l.window, l.step), (LiveWindow::H1, 60));
        assert_eq!(l.buckets.len(), LIVE_BUCKETS);
        assert_eq!(l.buckets[0].start, START);
        assert_eq!(l.buckets[59].start, START + 59 * 60);
        assert!(l.buckets.windows(2).all(|w| w[1].start - w[0].start == 60));

        assert_eq!(l.total.msgs, 7, "everything but the row before the window");
        assert_eq!(l.buckets[0].agg.msgs, 2, "START and START+59 share bar 0");
        assert_eq!(l.buckets[0].agg.o, 5);
        assert_eq!(l.buckets[0].models.len(), 2, "split by model inside the bar");
        assert_eq!(l.buckets[1].agg.msgs, 1);
        assert_eq!(l.buckets[59].agg.msgs, 1, "the future row");
        let idx_301 = usize::try_from((NOW - 301 - START) / 60).unwrap();
        assert_eq!(
            l.buckets[idx_301].agg.o,
            5 + 6 + 7,
            "NOW-301, NOW-300 and NOW-299 share one 60 s bar"
        );

        assert_eq!(
            l.recent.msgs, 3,
            "NOW-300, NOW-299 and the future row are within 5 minutes"
        );
        assert_eq!(l.quarter.msgs, 4, "plus NOW-301 within 15 minutes");
        assert_eq!(l.last_msg_ago, Some(0), "a future timestamp never gives a negative age");
    }

    #[test]
    fn live_last_message_age_and_fifteen_minute_boundary() {
        let rows = vec![
            live_row(NOW - 901, 0, 0, 1),
            live_row(NOW - 900, 0, 0, 2),
            live_row(NOW - 42, 0, 0, 3),
        ];
        let l = compute_live(&dataset_of(rows), Filter::default(), LiveWindow::H1, NOW);
        assert_eq!(l.last_msg_ago, Some(42));
        assert_eq!(l.quarter.msgs, 2, "NOW-900 is inside, NOW-901 is not");
        assert_eq!(l.recent.msgs, 1);
        let empty = compute_live(&dataset_of(vec![]), Filter::default(), LiveWindow::H1, NOW);
        assert_eq!(empty.last_msg_ago, None);
        assert_eq!(empty.total.msgs, 0);
    }

    #[test]
    fn live_respects_model_and_project_filters_but_not_the_range() {
        let rows = vec![
            live_row(NOW - 10, 0, 0, 1),
            live_row(NOW - 20, 1, 0, 2),
            live_row(NOW - 30, 0, 1, 3),
            live_row(NOW - 40, 1, 1, 4),
        ];
        let d = dataset_of(rows);
        let f = |model, project| Filter {
            range_days: Some(0),
            model,
            project,
        };
        assert_eq!(
            compute_live(&d, f(None, None), LiveWindow::H1, NOW).total.msgs,
            4,
            "range is ignored"
        );
        assert_eq!(compute_live(&d, f(Some(1), None), LiveWindow::H1, NOW).total.o, 6);
        assert_eq!(compute_live(&d, f(None, Some(1)), LiveWindow::H1, NOW).total.o, 7);
        assert_eq!(compute_live(&d, f(Some(0), Some(1)), LiveWindow::H1, NOW).total.o, 3);
    }

    #[test]
    fn session_weight_adds_every_token_kind() {
        // m0: 100 output. m1: 60 input + 60 5-minute writes + 60 1-hour writes = 180.
        // Any operator mix-up in the weight changes which is bigger.
        let mut a = row(100, 0, 0, 100, 1.0);
        (a.i, a.cw5, a.cw1, a.cr) = (0, 0, 0, 0);
        let mut b = row(100, 1, 0, 0, 1.0);
        (b.i, b.cw5, b.cw1, b.cr) = (60, 60, 60, 0);
        let s = compute(&dataset_of(vec![a, b]), Filter::default(), 100);
        assert_eq!(s.sessions[0].top_model, 1);
        // And the reverse: a big output beats a modest sum of the others.
        let mut a = row(100, 0, 0, 200, 1.0);
        (a.i, a.cw5, a.cw1, a.cr) = (0, 0, 0, 0);
        let mut b = row(100, 1, 0, 0, 1.0);
        (b.i, b.cw5, b.cw1, b.cr) = (60, 60, 60, 0);
        let s = compute(&dataset_of(vec![a, b]), Filter::default(), 100);
        assert_eq!(s.sessions[0].top_model, 0);
    }
}

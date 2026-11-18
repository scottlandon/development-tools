//! `--print`: a plain-text report for pipes, scripts and quick checks.

use crate::agg::{self, Filter};
use crate::data::Dataset;
use crate::timeutil::{WEEKDAYS, fmt_ymd, weekday};
use crate::ui::fmt::{tok_full, usd};
use std::fmt::Write;

pub fn render(ds: &Dataset, days: i32, today: i32) -> String {
    let s = agg::compute(
        ds,
        Filter {
            range_days: Some(days),
            ..Default::default()
        },
        today,
    );
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{:<14} {:>14} {:>12} {:>14} {:>16} {:>16} {:>10} {:>7}",
        "Date", "Input", "Output", "CacheWrite", "CacheRead", "Total", "Cost", "Msgs"
    );
    let row = |label: String, a: &agg::Agg| {
        format!(
            "{:<14} {:>14} {:>12} {:>14} {:>16} {:>16} {:>10} {:>7}",
            label,
            tok_full(a.i),
            tok_full(a.o),
            tok_full(a.cw),
            tok_full(a.cr),
            tok_full(a.total()),
            if a.cost == 0.0 && a.unpriced > 0 {
                "n/a".into()
            } else {
                usd(a.cost)
            },
            tok_full(a.msgs),
        )
    };
    for d in &s.days {
        let wd = WEEKDAYS[weekday(d.day as i64) as usize];
        let _ = writeln!(out, "{}", row(format!("{} {}", fmt_ymd(d.day), wd), &d.agg));
    }
    let _ = writeln!(out, "{}", row("TOTAL".into(), &s.total));
    if !s.by_model.is_empty() {
        let _ = writeln!(out, "\nBy model");
        for (m, a) in &s.by_model {
            let _ = writeln!(out, "{}", row(ds.models[*m as usize].short.clone(), a));
        }
    }
    if ds.looks_unparsed() {
        let _ = writeln!(
            out,
            "\nnote: found {} transcript files but no usage records in them; either the sessions are new \
             or Claude Code's log format has changed",
            ds.files
        );
    }
    if s.total.unpriced > 0 {
        let _ = writeln!(
            out,
            "\nnote: {} messages use models with no known price (cost excludes them)",
            s.total.unpriced
        );
    }
    out
}

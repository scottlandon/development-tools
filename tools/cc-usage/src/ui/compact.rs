//! Dense fallback for slivers of a pane (height < 9 or width < 36): no
//! borders, no tabs — a one-line summary plus as many recent days as fit.

use super::theme::{pal, series};
use super::{chart, fmt, views};
use crate::app::{App, Metric, View};
use crate::timeutil::fmt_md;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

fn val(m: Metric, cost: f64, tokens: u64) -> String {
    match m {
        Metric::Cost => fmt::usd(cost),
        Metric::Tokens => fmt::tok(tokens),
    }
}

/// Live tab in a sliver: headline rate, then a chart (or a one-line sparkline
/// when there's no room for one).
fn render_live(f: &mut Frame, area: Rect, app: &App) {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let w = area.width as usize;
    let lv = &app.live;
    let rate = fmt::tok(lv.recent.total() / 5);
    let long = format!("live {} · {rate}/min", lv.window.short());
    let head = if long.chars().count() < w {
        long
    } else {
        format!("{rate}/min")
    };
    let dot = if lv.last_msg_ago.is_some_and(|s| s < 90) {
        pal().cost
    } else {
        pal().dim
    };
    let head_line = Line::from(vec![
        Span::styled(" ● ", Style::default().fg(dot)),
        Span::styled(head, Style::default().fg(pal().accent).add_modifier(Modifier::BOLD)),
    ]);
    let (top, rest) = (
        Rect::new(area.x, area.y, area.width, 1),
        Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(1)),
    );
    f.render_widget(Paragraph::new(head_line), top);
    if rest.height >= 3 && rest.width >= 8 {
        let buckets = views::live_buckets(app, &|a| a.total());
        chart::render(f.buffer_mut(), rest, &buckets, &|v| fmt::tok(v as u64), &pal().series);
    } else if rest.height >= 1 {
        let vals: Vec<u64> = lv.buckets.iter().map(|b| b.agg.total()).collect();
        let keep = w.saturating_sub(2).min(vals.len());
        let tail = &vals[vals.len() - keep..];
        let max = tail.iter().copied().max().unwrap_or(0).max(1);
        let line: String = tail
            .iter()
            .map(|&v| {
                if v == 0 {
                    ' '
                } else {
                    LEVELS[((v * 7 / max) as usize).min(7)]
                }
            })
            .collect();
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::raw(" "),
                Span::styled(line, Style::default().fg(series(0))),
            ])),
            Rect::new(rest.x, rest.y, rest.width, 1),
        );
    }
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    if app.view == View::Live {
        return render_live(f, area, app);
    }
    let w = area.width as usize;
    let t = &app.summary.total;
    let m = app.metric;

    // Line 0: the most information that fits.
    let both = format!("{} · {} · {}", app.range.short(), fmt::tok(t.total()), fmt::usd(t.cost));
    let one = format!("{} {}", app.range.short(), val(m, t.cost, t.total()));
    let head = if both.chars().count() < w {
        both
    } else if one.chars().count() < w {
        one
    } else {
        val(m, t.cost, t.total())
    };
    let mut lines: Vec<Line> = vec![Line::from(vec![
        Span::styled(" ", Style::default()),
        Span::styled(head, Style::default().fg(pal().accent).add_modifier(Modifier::BOLD)),
    ])];

    let rows = (area.height as usize).saturating_sub(1);
    let days = &app.summary.days;
    let maxv = days
        .iter()
        .map(|d| match m {
            Metric::Cost => d.agg.cost,
            Metric::Tokens => d.agg.total() as f64,
        })
        .fold(0.0, f64::max);

    // Day rows (newest first): "MM-DD ████ $1.23"
    let show_days = rows.min(days.len());
    let vw = 7usize;
    for d in days.iter().rev().take(show_days) {
        let v = match m {
            Metric::Cost => d.agg.cost,
            Metric::Tokens => d.agg.total() as f64,
        };
        let txt = val(m, d.agg.cost, d.agg.total());
        let label = fmt_md(d.day);
        let mut spans = vec![Span::styled(format!(" {label} "), Style::default().fg(pal().dim))];
        let used = 1 + label.len() + 1 + vw + 1;
        if w > used + 2 {
            let bw = (w - used).min(40);
            let frac = if maxv > 0.0 { v / maxv } else { 0.0 };
            spans.push(Span::styled(fmt::hbar(frac, bw), Style::default().fg(series(0))));
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(format!("{txt:>vw$}"), Style::default().fg(pal().cost)));
        lines.push(Line::from(spans));
    }

    // Spare height after the days (e.g. the "today" range): top models.
    let spare = rows.saturating_sub(show_days);
    for (pos, (id, a)) in app.models_sorted.iter().enumerate().take(spare) {
        let name = &app.ds.models[*id as usize].short;
        let color = series(app.summary.model_rank.iter().position(|r| r == id).unwrap_or(pos));
        let txt = val(m, a.cost, a.total());
        let name_w = w.saturating_sub(vw + 5).max(4);
        lines.push(Line::from(vec![
            Span::styled(" ■ ", Style::default().fg(color)),
            Span::raw(format!("{:<name_w$}", fmt::trunc(name, name_w))),
            Span::styled(format!("{txt:>vw$}"), Style::default().fg(pal().cost)),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

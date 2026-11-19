//! Rendering. Everything is derived from the area ratatui hands us each
//! frame, so resizing a pane (herdr or otherwise) re-flows the UI live.
//!
//! Layout tiers (width): Narrow < 80 <= Medium < 130 <= Wide.
//! Compact mode (height < 9 or width < 36) replaces the whole chrome with a
//! dense text summary that still shows something useful in a sliver of a pane.

mod chart;
mod compact;
pub mod fmt;
mod models;
pub mod theme;
mod views;

use crate::app::{App, Metric, View};
use crate::cast;
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, Paragraph, Wrap};
use theme::{fill_style, pal};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Tier {
    Narrow,
    Medium,
    Wide,
}

pub fn tier(width: u16) -> Tier {
    match width {
        0..=79 => Tier::Narrow,
        80..=129 => Tier::Medium,
        _ => Tier::Wide,
    }
}

pub fn is_compact(a: Rect) -> bool {
    a.height < 9 || a.width < 36
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stats {
    Cards,
    Strip,
    None,
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    if area.width == 0 || area.height == 0 {
        return;
    }
    theme::set_current(theme::resolve(app.theme, app.truecolor));
    f.render_widget(Block::default().style(fill_style()), area);
    if !app.have_data {
        message(f, area, "⟳ scanning transcripts…", pal().dim);
        return;
    }
    if app.ds.rows.is_empty() {
        let roots = if app.roots.is_empty() {
            "no transcript directory found".to_string()
        } else {
            app.roots.join(", ")
        };
        let text = if app.ds.looks_unparsed() {
            format!(
                "Found {} transcript files but no usage records in them.\n\
                 Either the sessions are new, or Claude Code's log format has changed.\n\
                 Looked in: {roots}",
                app.ds.files
            )
        } else {
            format!("No Claude Code usage found.\nLooked in: {roots}")
        };
        message(f, area, &text, pal().dim);
        return;
    }
    if is_compact(area) {
        compact::render(f, area, app);
    } else {
        full(f, area, app);
    }
    if app.show_help && area.width >= 30 && area.height >= 12 {
        help(f, area);
    }
}

fn message(f: &mut Frame, area: Rect, text: &str, color: Color) {
    let lines = cast::to_u16(text.lines().count());
    let y = area.y + area.height.saturating_sub(lines) / 2;
    let r = Rect::new(area.x, y, area.width, lines.min(area.height));
    f.render_widget(
        Paragraph::new(text.to_string())
            .alignment(Alignment::Center)
            .style(Style::default().fg(color))
            .wrap(Wrap { trim: true }),
        r,
    );
}

fn split_top(r: Rect, n: u16) -> (Rect, Rect) {
    let n = n.min(r.height);
    (
        Rect::new(r.x, r.y, r.width, n),
        Rect::new(r.x, r.y + n, r.width, r.height - n),
    )
}

fn full(f: &mut Frame, area: Rect, app: &mut App) {
    let (w, h) = (area.width, area.height);
    let tier = tier(w);
    let show_tabs = h >= 12;
    // The Live tab carries its own headline strip; range totals would mislead there.
    let stats = if app.view == View::Live {
        Stats::None
    } else if h >= 24 && w >= 60 {
        Stats::Cards
    } else if h >= 12 {
        Stats::Strip
    } else {
        Stats::None
    };
    let footer_h = u16::from(h >= 10);

    let (header, rest) = split_top(area, 1);
    let (tabs, rest) = split_top(rest, u16::from(show_tabs));
    let (stats_r, rest) = split_top(
        rest,
        match stats {
            Stats::Cards => 3,
            Stats::Strip => 1,
            Stats::None => 0,
        },
    );
    let body_h = rest.height.saturating_sub(footer_h);
    let (body, footer) = split_top(rest, body_h);

    render_header(f, header, app, !show_tabs);
    if show_tabs {
        render_tabs(f, tabs, app);
    }
    match stats {
        Stats::Cards => render_cards(f, stats_r, app),
        Stats::Strip => render_strip(f, stats_r, app),
        Stats::None => {}
    }
    match app.view {
        View::Daily => views::daily(f, body, app, tier),
        View::Models | View::Projects => views::breakdown(f, body, app, tier),
        View::Sessions => views::sessions(f, body, app),
        View::Live => views::live(f, body, app),
    }
    if footer_h > 0 {
        render_footer(f, footer, app);
    }
}

fn metric_label(m: Metric) -> &'static str {
    match m {
        Metric::Cost => "cost",
        Metric::Tokens => "tokens",
    }
}

fn render_header(f: &mut Frame, area: Rect, app: &App, with_view: bool) {
    let mut left = vec![
        Span::styled(
            " cc-usage ",
            Style::default()
                .fg(pal().on_accent)
                .bg(pal().accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            if app.view == View::Live {
                app.live_window.label()
            } else {
                app.range.label()
            },
            Style::default().add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            if app.view == View::Live {
                " · tokens/min".to_string()
            } else {
                format!(" · by {}", metric_label(app.metric))
            },
            Style::default().fg(pal().dim),
        ),
    ];
    if with_view {
        left.push(Span::styled(
            format!(" · {}", app.view.label()),
            Style::default().fg(pal().accent),
        ));
    }
    let chip = Style::default().fg(pal().on_accent).bg(pal().caution);
    if let Some(m) = app.model_filter {
        left.push(Span::styled(
            format!(" model:{} ", app.ds.models[m as usize].short),
            chip,
        ));
    }
    if let Some(p) = app.project_filter {
        left.push(Span::styled(format!(" project:{} ", app.ds.projects[p as usize]), chip));
    }
    let lw: usize = left.iter().map(|s| s.content.chars().count()).sum();
    f.render_widget(Paragraph::new(Line::from(left)), area);

    let right = if app.scanning {
        "⟳ scanning ".to_string()
    } else {
        format!("{} msgs · {} files ", fmt::tok(app.ds.rows.len() as u64), app.ds.files)
    };
    let rw = right.chars().count();
    if lw + rw + 2 <= area.width as usize {
        f.render_widget(
            Paragraph::new(Span::styled(right, Style::default().fg(pal().dim))).alignment(Alignment::Right),
            area,
        );
    }
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let variants: [Vec<String>; 3] = [
        View::ALL
            .iter()
            .map(|v| format!(" {} {} ", v.idx() + 1, v.label()))
            .collect(),
        View::ALL.iter().map(|v| format!(" {} ", v.label())).collect(),
        View::ALL.iter().map(|v| format!(" {} ", v.short())).collect(),
    ];
    let fits = |v: &Vec<String>| v.iter().map(|s| s.chars().count() + 1).sum::<usize>() <= area.width as usize;
    let labels = variants.iter().find(|v| fits(v)).unwrap_or(&variants[2]);
    let mut spans = Vec::new();
    for (v, text) in View::ALL.iter().zip(labels) {
        let st = if *v == app.view {
            Style::default()
                .fg(pal().on_accent)
                .bg(pal().accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(pal().dim)
        };
        spans.push(Span::styled(text.clone(), st));
        spans.push(Span::raw(" "));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn stat_values(app: &App) -> [(&'static str, String, String); 4] {
    let t = &app.summary.total;
    let days = app.summary.days.len().max(1) as f64;
    let cost = if t.cost == 0.0 && t.unpriced > 0 {
        "n/a".to_string()
    } else {
        fmt::usd(t.cost)
    };
    let cost_sub = if t.unpriced > 0 && t.cost > 0.0 {
        format!("+{} unpriced", t.unpriced)
    } else {
        format!("{}/day", fmt::usd(t.cost / days))
    };
    let hit = t.cache_hit().map_or_else(|| "–".into(), fmt::pct);
    [
        (
            "Tokens",
            fmt::tok(t.total()),
            format!("in {} · out {}", fmt::tok(t.i + t.cw), fmt::tok(t.o)),
        ),
        ("Est. cost", cost, cost_sub),
        ("Cache hit", hit, format!("{} read", fmt::tok(t.cr))),
        (
            "Activity",
            format!("{} msgs", fmt::tok(t.msgs)),
            format!("{} sessions", app.summary.sessions.len()),
        ),
    ]
}

fn render_cards(f: &mut Frame, area: Rect, app: &App) {
    let vals = stat_values(app);
    let cols = Layout::horizontal([Constraint::Fill(1); 4]).split(area);
    for (r, (title, val, sub)) in cols.iter().zip(vals) {
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(pal().border))
            .title(Span::styled(format!(" {title} "), Style::default().fg(pal().dim)));
        let avail = r.width.saturating_sub(2) as usize;
        let fits = val.chars().count() + 2 + sub.chars().count() <= avail;
        let mut spans = vec![Span::styled(val, Style::default().add_modifier(Modifier::BOLD))];
        if fits {
            spans.push(Span::styled(format!("  {sub}"), Style::default().fg(pal().dim)));
        }
        let line = Line::from(spans);
        f.render_widget(Paragraph::new(line).block(block), *r);
    }
}

fn render_strip(f: &mut Frame, area: Rect, app: &App) {
    let vals = stat_values(app);
    let mut spans = vec![Span::raw(" ")];
    // Whole items only: a value cut off at the pane edge ("Activity 1" for 180 msgs) is worse than none.
    let mut used = 1;
    for (i, (title, val, _)) in vals.iter().enumerate() {
        let sep = if i > 0 { 3 } else { 0 };
        used += sep + title.chars().count() + 1 + val.chars().count();
        if used > area.width as usize {
            break;
        }
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(pal().border)));
        }
        spans.push(Span::styled(format!("{title} "), Style::default().fg(pal().dim)));
        spans.push(Span::styled(val.clone(), Style::default().add_modifier(Modifier::BOLD)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_footer(f: &mut Frame, area: Rect, app: &App) {
    if let Some(n) = &app.notice {
        f.render_widget(
            Paragraph::new(Span::styled(format!(" {n}"), Style::default().fg(pal().caution))),
            area,
        );
        return;
    }
    let sets: [&[(&str, &str)]; 4] = [
        &[
            ("q", "quit"),
            ("⇥", "view"),
            ("↑↓", "move"),
            ("t", "range"),
            ("c", "cost/tokens"),
            ("a", "theme"),
            ("m", "model"),
            ("p", "project"),
            ("s", "sort"),
            ("⏎", "filter"),
            ("r", "rescan"),
            ("?", "help"),
        ],
        &[
            ("q", "quit"),
            ("⇥", "view"),
            ("t", "range"),
            ("c", "metric"),
            ("a", "theme"),
            ("m", "model"),
            ("p", "proj"),
            ("?", "help"),
        ],
        &[("q", "quit"), ("⇥", "view"), ("t", "range"), ("?", "help")],
        &[("q", "quit"), ("?", "help")],
    ];
    let width = |s: &[(&str, &str)]| {
        s.iter()
            .map(|(k, d)| k.chars().count() + d.chars().count() + 3)
            .sum::<usize>()
            + 1
    };
    let chosen = sets
        .iter()
        .find(|s| width(s) <= area.width as usize)
        .unwrap_or(&sets[3]);
    let mut spans = vec![Span::raw(" ")];
    for (k, d) in *chosen {
        spans.push(Span::styled(
            *k,
            Style::default().fg(pal().accent).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {d}  "), Style::default().fg(pal().dim)));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// `(keys, description)` rows of the `?` overlay; every key `App::on_key` handles must appear here.
const HELP_ROWS: [(&str, &str); 15] = [
    ("Tab / Shift-Tab", "next / previous view (also ← →  h l)"),
    ("1-5", "jump to Daily/Models/Projects/Sessions/Live"),
    ("↑ ↓  j k", "move selection"),
    ("PgUp PgDn", "move 10 rows (also Ctrl-d / Ctrl-u)"),
    ("Home End  g G", "jump to first / last row"),
    ("t / T", "cycle time range (Live: window) fwd / back"),
    ("c", "toggle cost ⇄ tokens (chart, sort)"),
    ("m / p", "cycle model / project filter"),
    ("Enter", "filter by selected model/project"),
    ("Esc", "clear filters"),
    ("a / A", "next / previous colour theme"),
    ("s", "sort sessions"),
    ("r", "rescan now"),
    ("? / F1", "show this help"),
    ("q  Ctrl-C", "quit"),
];

fn help(f: &mut Frame, area: Rect) {
    let rows = HELP_ROWS;
    let h = (cast::to_u16(rows.len()) + 4).min(area.height);
    let w = 64u16.min(area.width);
    let r = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    f.render_widget(Clear, r);
    f.render_widget(Block::default().style(fill_style()), r);
    let mut lines = vec![Line::from("")];
    for (k, d) in rows {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {k:<17}"),
                Style::default().fg(pal().accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(d),
        ]));
    }
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal().accent))
        .title(" Keys ")
        .title_bottom(Line::from(format!(" prices checked {} ", crate::pricing::PRICES_VERIFIED)).left_aligned())
        .title_bottom(Line::from(" any key to close ").right_aligned());
    f.render_widget(Paragraph::new(lines).block(block), r);
}

#[cfg(test)]
mod tests;

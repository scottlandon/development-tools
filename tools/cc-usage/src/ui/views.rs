//! The four tab bodies. Each picks its columns / sub-panels from the area it
//! is given, so a narrow pane gets fewer columns and a big one gets a chart,
//! table and sidebar.

use super::chart::{self, Bucket};
use super::theme::{pal, series};
use super::{Tier, fmt, split_top};
use crate::agg::Agg;
use crate::app::{App, Metric, View};
use crate::timeutil::{WEEKDAYS, fmt_local_dt, fmt_local_hm, fmt_md, weekday};
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Paragraph, Row, Table, TableState};

// ------------------------------------------------------------ table plumbing

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum C {
    Label,
    When,
    Project,
    Model,
    In,
    Out,
    Cache,
    Cw,
    Cr,
    Total,
    Cost,
    Msgs,
    Dur,
    Share,
}

#[derive(Clone, Copy, Debug)]
pub struct Spec {
    pub c: C,
    pub w: Constraint,
    pub head: &'static str,
}

fn spec(c: C) -> Spec {
    let (w, head) = match c {
        C::Label => (Constraint::Length(9), ""),
        C::When => (Constraint::Length(11), "Last active"),
        C::Project => (Constraint::Min(10), "Project"),
        C::Model => (Constraint::Length(11), "Model"),
        C::In => (Constraint::Length(7), "Input"),
        C::Out => (Constraint::Length(7), "Output"),
        C::Cache => (Constraint::Length(7), "Cache"),
        C::Cw => (Constraint::Length(7), "CacheW"),
        C::Cr => (Constraint::Length(7), "CacheR"),
        C::Total => (Constraint::Length(7), "Total"),
        C::Cost => (Constraint::Length(8), "Cost"),
        C::Msgs => (Constraint::Length(6), "Msgs"),
        C::Dur => (Constraint::Length(7), "Span"),
        C::Share => (Constraint::Min(8), "Share"),
    };
    Spec { c, w, head }
}

fn with_w(mut s: Spec, w: Constraint) -> Spec {
    s.w = w;
    s
}

fn share_width(s: &Spec) -> usize {
    match s.w {
        Constraint::Length(n) | Constraint::Min(n) => n as usize,
        _ => 8,
    }
}

fn right_aligned(c: C) -> bool {
    !matches!(c, C::Label | C::When | C::Project | C::Model | C::Share)
}

fn cell(text: String, right: bool, style: Style) -> Cell<'static> {
    let line = Line::from(Span::styled(text, style));
    Cell::from(if right { line.right_aligned() } else { line })
}

/// Text for the numeric columns; `None` for the descriptive ones.
fn num_text(c: C, a: &Agg) -> Option<String> {
    Some(match c {
        C::In => fmt::tok(a.i),
        C::Out => fmt::tok(a.o),
        C::Cache => fmt::tok(a.cr + a.cw),
        C::Cw => fmt::tok(a.cw),
        C::Cr => fmt::tok(a.cr),
        C::Total => fmt::tok(a.total()),
        C::Cost => {
            if a.cost == 0.0 && a.unpriced > 0 {
                "n/a".to_string()
            } else {
                fmt::usd(a.cost)
            }
        }
        C::Msgs => fmt::tok(a.msgs),
        _ => return None,
    })
}

fn num_style(c: C, dim: bool) -> Style {
    if dim {
        return Style::default().fg(pal().dim);
    }
    match c {
        C::Cost => Style::default().fg(pal().cost),
        C::Total => Style::default().add_modifier(Modifier::BOLD),
        _ => Style::default(),
    }
}

fn value(m: Metric, a: &Agg) -> f64 {
    match m {
        Metric::Cost => a.cost,
        Metric::Tokens => a.total() as f64,
    }
}

fn agg_cells(specs: &[Spec], a: &Agg, label: Line<'static>, frac: f64, dim: bool) -> Vec<Cell<'static>> {
    specs
        .iter()
        .map(|s| match s.c {
            C::Label => Cell::from(label.clone()),
            C::Share => cell(
                fmt::hbar(frac, share_width(s)),
                false,
                Style::default().fg(if dim { pal().dim } else { pal().accent }),
            ),
            c => cell(num_text(c, a).unwrap_or_default(), right_aligned(c), num_style(c, dim)),
        })
        .collect()
}

struct TableCfg<'a> {
    title: Line<'static>,
    bottom: Option<Line<'static>>,
    specs: &'a [Spec],
    footer: Option<Vec<Cell<'static>>>,
}

/// Renders only the visible window of rows so huge tables stay O(screen).
fn render_table(
    f: &mut Frame,
    area: Rect,
    cfg: TableCfg,
    n: usize,
    state: &mut TableState,
    row: &dyn Fn(usize) -> Row<'static>,
) {
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal().border))
        .title(cfg.title);
    let inner = block.inner(area);
    let show_footer = cfg.footer.is_some() && inner.height >= 5;
    let rows_h = inner.height.saturating_sub(1 + show_footer as u16) as usize;
    if inner.width >= 20
        && let Some(b) = cfg.bottom
    {
        block = block.title_bottom(b);
    }
    if rows_h == 0 || n == 0 {
        f.render_widget(block, area);
        return;
    }
    let sel = state.selected().unwrap_or(0).min(n - 1);
    let mut off = state.offset().min(n - 1);
    if sel < off {
        off = sel;
    } else if sel >= off + rows_h {
        off = sel + 1 - rows_h;
    }
    off = off.min(n.saturating_sub(rows_h));
    *state.offset_mut() = off;
    if n > rows_h {
        block = block.title(Line::from(format!(" {}/{} ", sel + 1, n)).right_aligned());
    }

    let rows: Vec<Row> = (off..(off + rows_h).min(n)).map(row).collect();
    let widths: Vec<Constraint> = cfg.specs.iter().map(|s| s.w).collect();
    let head_style = Style::default().fg(pal().dim).add_modifier(Modifier::BOLD);
    let header = Row::new(
        cfg.specs
            .iter()
            .map(|s| cell(s.head.to_string(), right_aligned(s.c), head_style)),
    );
    let mut table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(Style::default().bg(pal().sel_bg).add_modifier(Modifier::BOLD));
    if let (true, Some(cells)) = (show_footer, cfg.footer) {
        table = table.footer(Row::new(cells).style(Style::default().add_modifier(Modifier::BOLD)));
    }
    let mut ts = TableState::default().with_selected(Some(sel - off));
    f.render_stateful_widget(table, area, &mut ts);
}

fn title(text: &str) -> Line<'static> {
    Line::from(vec![
        Span::raw(" "),
        Span::styled(text.to_string(), Style::default().add_modifier(Modifier::BOLD)),
        Span::raw(" "),
    ])
}

fn hint(text: &str) -> Line<'static> {
    Line::from(Span::styled(format!(" {text} "), Style::default().fg(pal().dim))).right_aligned()
}

fn footer_cells(specs: &[Spec], total: &Agg) -> Vec<Cell<'static>> {
    specs
        .iter()
        .map(|s| match s.c {
            C::Label => cell("Total".into(), false, Style::default()),
            c => cell(
                num_text(c, total).unwrap_or_default(),
                right_aligned(c),
                num_style(c, false),
            ),
        })
        .collect()
}

// -------------------------------------------------------------------- daily

fn daily_specs(w: u16) -> Vec<Spec> {
    let label = |n| with_w(spec(C::Label), Constraint::Length(n));
    match w {
        80.. => vec![
            label(9),
            spec(C::In),
            spec(C::Out),
            spec(C::Cw),
            spec(C::Cr),
            spec(C::Total),
            spec(C::Cost),
            spec(C::Msgs),
            spec(C::Share),
        ],
        60..=79 => vec![
            label(9),
            spec(C::In),
            spec(C::Out),
            spec(C::Cw),
            spec(C::Cr),
            spec(C::Total),
            spec(C::Cost),
        ],
        44..=59 => vec![
            label(5),
            spec(C::In),
            spec(C::Out),
            spec(C::Cache),
            spec(C::Total),
            spec(C::Cost),
        ],
        28..=43 => vec![label(5), spec(C::Total), spec(C::Cost)],
        _ => vec![label(5), spec(C::Cost)],
    }
}

pub fn daily(f: &mut Frame, area: Rect, app: &mut App, tier: Tier) {
    if tier == Tier::Wide && area.height >= 14 {
        let side_w = (area.width * 3 / 10).clamp(34, 56);
        let [main, side] = Layout::horizontal([Constraint::Min(60), Constraint::Length(side_w)]).areas(area);
        daily_main(f, main, app);
        sidebar(f, side, app);
    } else {
        daily_main(f, area, app);
    }
}

fn daily_main(f: &mut Frame, area: Rect, app: &mut App) {
    let h = area.height;
    let chart_h = match h {
        22.. => h * 2 / 5,
        16..=21 => 9,
        12..=15 => 7,
        _ => 0,
    };
    if chart_h >= 6 && h - chart_h >= 5 {
        let (c, t) = split_top(area, chart_h);
        daily_chart(f, c, app);
        daily_table(f, t, app);
    } else {
        daily_table(f, area, app);
    }
}

/// Colour key for the top models, as many as fit in `room` cells.
fn legend(app: &App, room: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0usize;
    for (pos, m) in app.summary.model_rank.iter().enumerate().take(8) {
        let label = &app.ds.models[*m as usize].short;
        let need = label.chars().count() + 3;
        if used + need > room {
            break;
        }
        used += need;
        out.push(Span::styled("■ ", Style::default().fg(series(pos))));
        out.push(Span::styled(format!("{label}  "), Style::default().fg(pal().dim)));
    }
    out
}

fn daily_chart(f: &mut Frame, area: Rect, app: &App) {
    let metric = app.metric;
    let rank = &app.summary.model_rank;
    let buckets: Vec<Bucket> = app
        .summary
        .days
        .iter()
        .map(|d| {
            let mut segs: Vec<(usize, f64)> = d
                .models
                .iter()
                .map(|(m, a)| (rank.iter().position(|r| r == m).unwrap_or(0), value(metric, a)))
                .collect();
            segs.sort_by_key(|s| s.0);
            Bucket {
                label: fmt_md(d.day),
                segs,
            }
        })
        .collect();

    let name = match metric {
        Metric::Cost => "Est. cost per day",
        Metric::Tokens => "Tokens per day",
    };
    let legend = legend(app, (area.width as usize).saturating_sub(name.len() + 8));
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal().border))
        .title(title(name));
    if !legend.is_empty() {
        block = block.title(Line::from(legend).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    let axis = |v: f64| match metric {
        Metric::Cost => fmt::usd(v),
        Metric::Tokens => fmt::tok(v as u64),
    };
    chart::render(f.buffer_mut(), inner, &buckets, &axis, &pal().series);
}

fn daily_table(f: &mut Frame, area: Rect, app: &mut App) {
    let specs = daily_specs(area.width);
    let metric = app.metric;
    let today = app.summary.today;
    let days = &app.summary.days;
    let max = days.iter().map(|d| value(metric, &d.agg)).fold(0.0, f64::max);
    let long_label = specs[0].w == Constraint::Length(9);
    let n = days.len();
    let row = |i: usize| {
        let d = &days[n - 1 - i];
        let wd = weekday(d.day as i64);
        let text = if long_label {
            format!("{} {}", WEEKDAYS[wd as usize], fmt_md(d.day))
        } else {
            fmt_md(d.day)
        };
        let weekend = wd == 0 || wd == 6;
        let mut st = Style::default();
        if d.day == today {
            st = st.fg(pal().accent).add_modifier(Modifier::BOLD);
        } else if weekend {
            st = st.fg(pal().dim);
        }
        let empty = d.agg.msgs == 0;
        let frac = if max > 0.0 { value(metric, &d.agg) / max } else { 0.0 };
        Row::new(agg_cells(
            &specs,
            &d.agg,
            Line::from(Span::styled(
                text,
                if empty { Style::default().fg(pal().dim) } else { st },
            )),
            frac,
            empty,
        ))
    };
    let cfg = TableCfg {
        title: title(&format!("Daily · {} days", n)),
        bottom: Some(hint("t range · c metric")),
        specs: &specs,
        footer: Some(footer_cells(&specs, &app.summary.total)),
    };
    render_table(f, area, cfg, n, &mut app.tables[View::Daily.idx()], &row);
}

// ------------------------------------------------------------------ sidebar

struct BarItem {
    label: String,
    frac: f64,
    value: String,
    color: Color,
}

fn bar_list(f: &mut Frame, area: Rect, name: &str, items: &[BarItem]) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal().border))
        .title(title(name));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let nw = items.iter().map(|i| i.label.chars().count()).max().unwrap_or(0).min(14);
    let vw = items.iter().map(|i| i.value.chars().count()).max().unwrap_or(0);
    let pct_w = 4usize;
    let fixed = nw + 1 + 1 + pct_w + 1 + vw;
    let bw = (inner.width as usize).saturating_sub(fixed);
    let lines: Vec<Line> = items
        .iter()
        .take(inner.height as usize)
        .map(|it| {
            let mut spans = vec![Span::raw(format!("{:<nw$} ", fmt::trunc(&it.label, nw)))];
            if bw >= 3 {
                spans.push(Span::styled(fmt::hbar(it.frac, bw), Style::default().fg(it.color)));
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                format!("{:>pct_w$} ", fmt::pct(it.frac)),
                Style::default().fg(pal().dim),
            ));
            spans.push(Span::raw(format!("{:>vw$}", it.value)));
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

fn breakdown_items(app: &App, items: &[(u16, Agg)], projects: bool, limit: usize) -> Vec<BarItem> {
    let metric = app.metric;
    let sum: f64 = items.iter().map(|(_, a)| value(metric, a)).sum();
    items
        .iter()
        .take(limit)
        .enumerate()
        .map(|(pos, (id, a))| {
            let v = value(metric, a);
            BarItem {
                label: if projects {
                    app.ds.projects[*id as usize].clone()
                } else {
                    app.ds.models[*id as usize].short.clone()
                },
                frac: if sum > 0.0 { v / sum } else { 0.0 },
                value: match metric {
                    Metric::Cost => fmt::usd(a.cost),
                    Metric::Tokens => fmt::tok(a.total()),
                },
                color: if projects {
                    pal().accent
                } else {
                    series(app.summary.model_rank.iter().position(|m| m == id).unwrap_or(pos))
                },
            }
        })
        .collect()
}

fn sidebar(f: &mut Frame, area: Rect, app: &App) {
    let t = &app.summary.total;
    let total = t.total().max(1) as f64;
    let mix = vec![
        BarItem {
            label: "Input".into(),
            frac: t.i as f64 / total,
            value: fmt::tok(t.i),
            color: series(0),
        },
        BarItem {
            label: "Output".into(),
            frac: t.o as f64 / total,
            value: fmt::tok(t.o),
            color: series(3),
        },
        BarItem {
            label: "Cache write".into(),
            frac: t.cw as f64 / total,
            value: fmt::tok(t.cw),
            color: series(2),
        },
        BarItem {
            label: "Cache read".into(),
            frac: t.cr as f64 / total,
            value: fmt::tok(t.cr),
            color: series(1),
        },
    ];
    let mix_h = 6u16;
    let (top, mix_area) = if area.height > mix_h + 4 {
        let (a, b) = split_top(area, area.height - mix_h);
        (a, Some(b))
    } else {
        (area, None)
    };
    let lists_h = top.height;
    if lists_h >= 14 {
        let (m, p) = split_top(top, lists_h / 2);
        bar_list(
            f,
            m,
            "Models",
            &breakdown_items(app, &app.models_sorted, false, m.height.saturating_sub(2) as usize),
        );
        bar_list(
            f,
            p,
            "Projects",
            &breakdown_items(app, &app.projects_sorted, true, p.height.saturating_sub(2) as usize),
        );
    } else {
        bar_list(
            f,
            top,
            "Models",
            &breakdown_items(app, &app.models_sorted, false, top.height.saturating_sub(2) as usize),
        );
    }
    if let Some(r) = mix_area {
        bar_list(f, r, "Token mix", &mix);
    }
}

// ---------------------------------------------------------------- breakdown

pub fn breakdown(f: &mut Frame, area: Rect, app: &mut App, tier: Tier) {
    let projects = app.view == View::Projects;
    let w = area.width;
    let name_w = |n| with_w(spec(C::Label), Constraint::Min(n));
    let specs: Vec<Spec> = match w {
        96.. => vec![
            name_w(14),
            spec(C::Msgs),
            spec(C::In),
            spec(C::Out),
            spec(C::Cw),
            spec(C::Cr),
            spec(C::Total),
            spec(C::Cost),
            with_w(spec(C::Share), Constraint::Length(14)),
        ],
        76..=95 => vec![
            name_w(12),
            spec(C::In),
            spec(C::Out),
            spec(C::Cw),
            spec(C::Cr),
            spec(C::Total),
            spec(C::Cost),
        ],
        52..=75 => vec![name_w(10), spec(C::Out), spec(C::Cache), spec(C::Total), spec(C::Cost)],
        32..=51 => vec![name_w(8), spec(C::Total), spec(C::Cost)],
        _ => vec![name_w(6), spec(C::Cost)],
    };
    let metric = app.metric;
    let items: &Vec<(u16, Agg)> = if projects {
        &app.projects_sorted
    } else {
        &app.models_sorted
    };
    let sum: f64 = items.iter().map(|(_, a)| value(metric, a)).sum();
    let ds = app.ds.clone();
    let rank = &app.summary.model_rank;
    let active = if projects { app.project_filter } else { app.model_filter };
    let row = |i: usize| {
        let (id, a) = &items[i];
        let is_active = active == Some(*id);
        let name = if projects {
            ds.projects[*id as usize].clone()
        } else {
            ds.models[*id as usize].short.clone()
        };
        let mut spans = Vec::new();
        if !projects {
            let pos = rank.iter().position(|m| m == id).unwrap_or(0);
            spans.push(Span::styled("■ ", Style::default().fg(series(pos))));
        }
        let name_style = if is_active {
            Style::default().fg(pal().caution).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        spans.push(Span::styled(name, name_style));
        if is_active {
            spans.push(Span::styled(" ◂", Style::default().fg(pal().caution)));
        }
        let frac = if sum > 0.0 { value(metric, a) / sum } else { 0.0 };
        Row::new(agg_cells(&specs, a, Line::from(spans), frac, false))
    };
    let (view, label) = if projects {
        (View::Projects, "Projects")
    } else {
        (View::Models, "Models")
    };
    let n = items.len();
    // Tall pane: shrink the table to its content and show the trend below it.
    let content_h = n as u16 + 4;
    let table_area = if tier != Tier::Narrow && area.height >= content_h + 9 {
        let (t, c) = split_top(area, content_h);
        daily_chart(f, c, app);
        t
    } else {
        area
    };
    let items: &Vec<(u16, Agg)> = if projects {
        &app.projects_sorted
    } else {
        &app.models_sorted
    };
    let cfg = TableCfg {
        title: title(&format!("{label} · {}", items.len())),
        bottom: Some(hint("⏎ filter · esc clear")),
        specs: &specs,
        footer: Some(footer_cells(&specs, &app.summary.total)),
    };
    render_table(f, table_area, cfg, n, &mut app.tables[view.idx()], &row);
}

// ----------------------------------------------------------------- sessions

pub fn sessions(f: &mut Frame, area: Rect, app: &mut App) {
    let specs: Vec<Spec> = match area.width {
        100.. => vec![
            spec(C::When),
            spec(C::Project),
            spec(C::Model),
            spec(C::Msgs),
            spec(C::Out),
            spec(C::Total),
            spec(C::Cost),
            spec(C::Dur),
        ],
        74..=99 => vec![
            spec(C::When),
            spec(C::Project),
            spec(C::Model),
            spec(C::Total),
            spec(C::Cost),
        ],
        44..=73 => vec![spec(C::When), spec(C::Project), spec(C::Cost)],
        _ => vec![spec(C::When), spec(C::Cost)],
    };
    let ds = app.ds.clone();
    let sessions = &app.summary.sessions;
    let order = &app.sess_order;
    let row = |i: usize| {
        let s = &sessions[order[i]];
        let cells = specs
            .iter()
            .map(|sp| match sp.c {
                C::When => cell(fmt_local_dt(s.last), false, Style::default()),
                C::Project => cell(ds.projects[s.project as usize].clone(), false, Style::default()),
                C::Model => cell(
                    ds.models[s.top_model as usize].short.clone(),
                    false,
                    Style::default().fg(pal().dim),
                ),
                C::Dur => cell(fmt::dur(s.last - s.first), true, Style::default().fg(pal().dim)),
                c => cell(
                    num_text(c, &s.agg).unwrap_or_default(),
                    right_aligned(c),
                    num_style(c, false),
                ),
            })
            .collect::<Vec<_>>();
        Row::new(cells)
    };
    let cfg = TableCfg {
        title: title(&format!("Sessions · {} · sort: {}", order.len(), app.sess_sort.label())),
        bottom: Some(hint("s sort")),
        specs: &specs,
        footer: None,
    };
    render_table(f, area, cfg, order.len(), &mut app.tables[View::Sessions.idx()], &row);
}

// --------------------------------------------------------------------- live

/// Per-minute rate buckets for the Live charts, stacked by model.
pub(super) fn live_buckets(app: &App, pick: &dyn Fn(&Agg) -> u64) -> Vec<Bucket> {
    let lv = &app.live;
    let rank = &app.summary.model_rank;
    let per_min = 60.0 / lv.step.max(1) as f64;
    lv.buckets
        .iter()
        .map(|b| {
            let mut segs: Vec<(usize, f64)> = b
                .models
                .iter()
                .map(|(m, a)| {
                    (
                        rank.iter().position(|r| r == m).unwrap_or(*m as usize),
                        pick(a) as f64 * per_min,
                    )
                })
                .collect();
            segs.sort_by_key(|s| s.0);
            Bucket {
                label: fmt_local_hm(b.start),
                segs,
            }
        })
        .collect()
}

pub fn live(f: &mut Frame, area: Rect, app: &App) {
    let (strip, body) = split_top(area, (area.height >= 6) as u16);
    if strip.height > 0 {
        live_strip(f, strip, app);
    }
    if body.height >= 16 {
        let (a, b) = split_top(body, body.height * 3 / 5);
        live_chart(f, a, app, "Tokens / min", &|a| a.total());
        live_chart(f, b, app, "Output tokens / min", &|a| a.o);
    } else {
        live_chart(f, body, app, "Tokens / min", &|a| a.total());
    }
}

fn live_chart(f: &mut Frame, area: Rect, app: &App, name: &str, pick: &dyn Fn(&Agg) -> u64) {
    let buckets = live_buckets(app, pick);
    let heading = format!("{name} · last {}", app.live.window.label());
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal().border))
        .title(title(&heading));
    let key = legend(app, (area.width as usize).saturating_sub(heading.chars().count() + 8));
    if !key.is_empty() {
        block = block.title(Line::from(key).right_aligned());
    }
    let inner = block.inner(area);
    f.render_widget(block, area);
    chart::render(f.buffer_mut(), inner, &buckets, &|v| fmt::tok(v as u64), &pal().series);
}

/// One line of headline numbers: activity light, current rates, $/hr, recency.
fn live_strip(f: &mut Frame, area: Rect, app: &App) {
    let lv = &app.live;
    let active = lv.last_msg_ago.is_some_and(|s| s < 90);
    let (dot, dot_style) = if active {
        ("● live", Style::default().fg(pal().cost).add_modifier(Modifier::BOLD))
    } else {
        ("○ idle", Style::default().fg(pal().dim))
    };
    let per_min = |n: u64| fmt::tok(n / 5);
    let sep = || Span::styled(" │ ", Style::default().fg(pal().border));
    let label = |t: &str| Span::styled(format!("{t} "), Style::default().fg(pal().dim));
    let bold = |t: String| Span::styled(t, Style::default().add_modifier(Modifier::BOLD));
    let ago = match lv.last_msg_ago {
        Some(s) => format!("{} ago", fmt::dur(s)),
        None => "none in window".to_string(),
    };
    let spans = vec![
        Span::raw(" "),
        Span::styled(dot, dot_style),
        sep(),
        label("tokens/min"),
        bold(per_min(lv.recent.total())),
        sep(),
        label("output/min"),
        bold(per_min(lv.recent.o)),
        sep(),
        label("≈ $/hr"),
        bold(fmt::usd(lv.quarter.cost * 4.0)),
        sep(),
        label("last msg"),
        bold(ago),
        Span::styled("  (rates: last 5 min)", Style::default().fg(pal().dim)),
    ];
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

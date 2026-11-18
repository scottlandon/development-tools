//! Render the whole UI into an in-memory terminal at many sizes. The point is
//! the responsive behaviour: no panics at any size, and the right pieces
//! appear / disappear as the pane grows.

use super::*;
use crate::agg::Range;
use crate::app::{App, View};
use crate::data::{Dataset, ModelInfo, Row};
use crate::pricing;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use std::sync::Arc;

const TODAY: i32 = 20_400;

fn dataset(days: i32) -> Dataset {
    let names = [
        "claude-sonnet-5-5",
        "claude-opus-5-5",
        "claude-haiku-5-5",
        "mystery-model",
    ];
    let models: Vec<ModelInfo> = names
        .iter()
        .map(|n| ModelInfo {
            short: crate::data::short_model(n),
            price: pricing::lookup(n),
        })
        .collect();
    let mut rows = Vec::new();
    for d in 0..days {
        for k in 0..6u32 {
            let model = ((d as u32 + k) % 4) as u16;
            let (i, o, cr, cw5) = (
                200 + k * 10,
                1500 + d as u32 * 37 + k * 400,
                400_000 + k * 90_000,
                20_000 + k * 1000,
            );
            let cost = models[model as usize]
                .price
                .map(|p| p.cost(i as u64, o as u64, cr as u64, cw5 as u64, 0))
                .unwrap_or(0.0);
            rows.push(Row {
                ts: (TODAY - d) as i64 * 86_400 + 40_000 + k as i64 * 600,
                day: TODAY - d,
                model,
                project: (k % 3) as u16,
                session: (d as u32 * 6 + k) % 17,
                i,
                o,
                cr,
                cw5,
                cw1: 0,
                cost,
                priced: models[model as usize].price.is_some(),
            });
        }
    }
    rows.sort_by_key(|r| r.ts);
    Dataset {
        rows,
        models,
        projects: vec![
            "api-server".into(),
            "development-tools".into(),
            "a-very-long-project-name-indeed".into(),
        ],
        sessions: 17,
        files: 42,
    }
}

fn app_with(days: i32, range: Range) -> App {
    let mut a = App::new(range);
    a.ds = Arc::new(dataset(days));
    a.have_data = true;
    a.scanning = false;
    a.today = TODAY;
    a.roots = vec!["/x".into()];
    a.recompute();
    a
}

fn render(app: &mut App, w: u16, h: u16) -> String {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    let buf = term.backend().buffer().clone();
    let mut s = String::new();
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            s.push_str(buf[(x, y)].symbol());
        }
        s.push('\n');
    }
    s
}

#[test]
fn never_panics_at_any_size_or_view() {
    for range in [Range::Today, Range::Week, Range::Month, Range::All] {
        let mut app = app_with(45, range);
        for view in View::ALL {
            app.view = view;
            for &(w, h) in &[
                (1, 1),
                (2, 1),
                (10, 3),
                (20, 5),
                (35, 8),
                (36, 9),
                (40, 10),
                (60, 12),
                (79, 24),
                (80, 24),
                (100, 30),
                (129, 40),
                (130, 40),
                (200, 60),
                (300, 12),
                (250, 7),
                (36, 80),
                (500, 100),
            ] {
                let _ = render(&mut app, w, h);
            }
        }
    }
}

#[test]
fn never_panics_with_filters_help_and_scrolled_tables() {
    let mut app = app_with(300, Range::All);
    app.model_filter = Some(1);
    app.project_filter = Some(2);
    app.recompute();
    app.show_help = true;
    for view in View::ALL {
        app.view = view;
        let t = &mut app.tables[view.idx()];
        t.select(Some(120));
        for &(w, h) in &[(30, 12), (80, 20), (140, 50)] {
            let _ = render(&mut app, w, h);
        }
    }
}

#[test]
fn empty_and_loading_states() {
    let mut app = App::new(Range::Week);
    assert!(render(&mut app, 60, 10).contains("scanning"));
    app.have_data = true;
    app.roots = vec!["/nowhere".into()];
    assert!(render(&mut app, 60, 10).contains("No Claude Code usage found"));
    // Dataset with rows, but all outside the range.
    let mut app = app_with(3, Range::Today);
    app.today += 100;
    app.recompute();
    let _ = render(&mut app, 80, 24);
}

#[test]
fn wide_pane_shows_sidebar_chart_and_cards() {
    let mut app = app_with(30, Range::Month);
    let s = render(&mut app, 160, 40);
    for want in [
        "Est. cost per day",
        "Token mix",
        "Models",
        "Projects",
        "Cache hit",
        "Daily",
        "Msgs",
        "Share",
    ] {
        assert!(s.contains(want), "wide layout missing {want:?}\n{s}");
    }
}

#[test]
fn medium_pane_has_chart_but_no_sidebar() {
    let mut app = app_with(30, Range::Month);
    let s = render(&mut app, 100, 30);
    assert!(s.contains("Est. cost per day"));
    assert!(s.contains("CacheR"));
    assert!(!s.contains("Token mix"));
}

#[test]
fn narrow_pane_drops_columns() {
    let mut app = app_with(30, Range::Month);
    let s = render(&mut app, 50, 30);
    assert!(s.contains("Total") && s.contains("Cost"));
    assert!(!s.contains("CacheR") && !s.contains("Msgs"));
}

#[test]
fn short_pane_drops_chart_then_chrome() {
    let mut app = app_with(30, Range::Month);
    let s = render(&mut app, 100, 11);
    assert!(!s.contains("Est. cost per day"), "no room for a chart at 11 rows");
    assert!(s.contains("Daily"));
    let s = render(&mut app, 100, 7);
    assert!(!s.contains("1 Daily"), "compact mode has no tab bar");
    assert!(!s.contains("cc-usage"), "compact header is just numbers");
    assert!(s.contains('$'));
}

#[test]
fn compact_one_liner_and_today_models() {
    let mut app = app_with(5, Range::Today);
    let s = render(&mut app, 30, 5);
    assert!(s.contains("1d"), "{s}");
    assert!(
        s.contains("sonnet") || s.contains("opus") || s.contains("haiku"),
        "today shows models when days run out\n{s}"
    );
    let one = render(&mut app, 12, 1);
    assert!(one.contains('$') || one.contains('K') || one.contains('M'), "{one}");
}

#[test]
fn unpriced_models_are_flagged_not_hidden() {
    let mut app = app_with(10, Range::Week);
    app.view = View::Models;
    let s = render(&mut app, 120, 30);
    assert!(s.contains("mystery-model"));
    assert!(s.contains("n/a"));
}

#[test]
fn filter_chip_shows_in_header() {
    let mut app = app_with(10, Range::Week);
    app.model_filter = Some(0);
    app.recompute();
    assert!(render(&mut app, 100, 30).contains("model:sonnet-5-5"));
}

#[test]
fn resize_sequence_is_stable() {
    // Simulate dragging a pane edge: render every width in a sweep.
    let mut app = app_with(60, Range::Quarter);
    for w in 1..=200u16 {
        let _ = render(&mut app, w, 24);
    }
    for h in 1..=70u16 {
        let _ = render(&mut app, 120, h);
    }
}

/// A Live-tab app whose data is "now": a message every 20 s for the last few minutes.
fn live_app() -> App {
    let mut a = app_with(2, Range::Week);
    let now = chrono::Utc::now().timestamp();
    let mut ds = dataset(2);
    let n = ds.rows.len();
    for (i, r) in ds.rows.iter_mut().enumerate() {
        r.ts = now - (n - i) as i64 * 20;
    }
    a.ds = Arc::new(ds);
    a.view = View::Live;
    a.recompute();
    a
}

#[test]
fn live_tab_shows_two_charts_and_rates() {
    let mut app = live_app();
    let s = render(&mut app, 120, 34);
    for want in [
        "Tokens / min",
        "Output tokens / min",
        "● live",
        "tokens/min",
        "output/min",
        "$/hr",
        "last msg",
        "1 hour",
    ] {
        assert!(s.contains(want), "live view missing {want:?}\n{s}");
    }
    assert!(!s.contains("Cache hit"), "range stat cards are hidden on the Live tab");
    // Real bars were drawn (a message arrived within the last hour).
    assert!(s.contains('█') || s.contains('▇') || s.contains('▄'), "{s}");
}

#[test]
fn live_tab_drops_second_chart_when_short() {
    let mut app = live_app();
    // Cards are hidden on this tab, so 20 rows still fit both charts…
    let s = render(&mut app, 120, 20);
    assert!(s.contains("Tokens / min") && s.contains("Output tokens / min"), "{s}");
    // …but at 17 rows (14 for the body) only the main chart remains.
    let s = render(&mut app, 120, 17);
    assert!(s.contains("Tokens / min"));
    assert!(!s.contains("Output tokens / min"), "no room for two charts\n{s}");
}

#[test]
fn live_tab_window_changes_with_t() {
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = live_app();
    app.on_key(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::NONE));
    let s = render(&mut app, 120, 34);
    assert!(s.contains("6 hours"), "{s}");
}

#[test]
fn live_tab_idle_and_empty_states() {
    // Data exists but nothing in the last hour: honest "idle", no fake bars.
    let mut app = app_with(5, Range::Week);
    app.view = View::Live;
    app.recompute_live();
    let s = render(&mut app, 120, 30);
    assert!(s.contains("○ idle"), "{s}");
    assert!(s.contains("none in window"), "{s}");
    assert!(s.contains("no usage in this range"), "{s}");
}

#[test]
fn live_tab_compact_variants() {
    let mut app = live_app();
    let s = render(&mut app, 40, 7);
    assert!(s.contains("live 1h") && s.contains("/min"), "{s}");
    // Not enough height for a chart: a one-line sparkline instead.
    let s = render(&mut app, 30, 2);
    assert!(s.contains("/min"), "{s}");
    assert!(s.chars().any(|c| "▁▂▃▄▅▆▇█".contains(c)), "sparkline expected\n{s}");
    let _ = render(&mut app, 12, 1);
}

#[test]
fn live_tab_survives_resize_sweep_and_filters() {
    let mut app = live_app();
    app.model_filter = Some(1);
    app.recompute();
    for w in 1..=180u16 {
        let _ = render(&mut app, w, 30);
    }
    for h in 1..=60u16 {
        let _ = render(&mut app, 100, h);
    }
}

// ----------------------------------------------------------------- themes

use ratatui::buffer::Buffer;
use ratatui::style::Color;
use theme::ThemeId;

fn render_buf(app: &mut App, w: u16, h: u16) -> Buffer {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(|f| draw(f, app)).unwrap();
    term.backend().buffer().clone()
}

fn themed(id: ThemeId, truecolor: bool) -> App {
    let mut a = app_with(30, Range::Month);
    a.theme = id;
    a.truecolor = truecolor;
    a
}

#[test]
fn every_theme_renders_every_view_at_every_size_class() {
    for id in ThemeId::ALL {
        for truecolor in [true, false] {
            let mut app = themed(id, truecolor);
            for view in View::ALL {
                app.view = view;
                for &(w, h) in &[(1, 1), (20, 4), (40, 8), (60, 20), (100, 30), (160, 45)] {
                    let _ = render(&mut app, w, h);
                }
            }
            app.show_help = true;
            let _ = render(&mut app, 100, 30);
        }
    }
}

#[test]
fn mainframe_paints_pure_black_edge_to_edge() {
    let mut app = themed(ThemeId::Mainframe, true);
    let buf = render_buf(&mut app, 120, 34);
    let blacks = buf.content.iter().filter(|c| c.bg == Color::Rgb(0, 0, 0)).count();
    let others: Vec<_> = buf
        .content
        .iter()
        .filter(|c| c.bg != Color::Rgb(0, 0, 0))
        .map(|c| c.bg)
        .collect();
    // Everything is black except the selected row and the accent / filter chips.
    assert!(blacks > buf.content.len() * 9 / 10, "{blacks} of {}", buf.content.len());
    assert!(
        others
            .iter()
            .all(|c| matches!(c, Color::Rgb(0x14, 0x38, 0x26) | Color::Rgb(0x33, 0xFF, 0x77))),
        "unexpected backgrounds: {others:?}"
    );
    // The readout is white-ish, not the terminal default.
    assert!(buf.content.iter().any(|c| c.fg == Color::Rgb(0xE6, 0xED, 0xE8)));
}

#[test]
fn themed_help_overlay_keeps_the_ground() {
    let mut app = themed(ThemeId::Catppuccin, true);
    app.show_help = true;
    let buf = render_buf(&mut app, 100, 30);
    let base = Color::Rgb(0x1E, 0x1E, 0x2E);
    // Centre of the popup (rows are blank padding there) must still be the theme's ground, not Reset.
    let cell = &buf[(50, 15)];
    assert_eq!(cell.bg, base, "{cell:?}");
}

#[test]
fn indexed_mode_never_emits_rgb() {
    for id in ThemeId::ALL {
        let mut app = themed(id, false);
        let buf = render_buf(&mut app, 120, 34);
        assert!(
            buf.content
                .iter()
                .all(|c| !matches!(c.fg, Color::Rgb(..)) && !matches!(c.bg, Color::Rgb(..))),
            "{id:?} leaked 24-bit colour in 256-colour mode"
        );
    }
}

#[test]
fn chart_bars_use_the_themes_series_colours() {
    let mut app = themed(ThemeId::Mainframe, true);
    let buf = render_buf(&mut app, 120, 34);
    let p = theme::resolve(ThemeId::Mainframe, true);
    let seen: std::collections::HashSet<_> = buf
        .content
        .iter()
        .filter(|c| "▁▂▃▄▅▆▇█".contains(c.symbol()))
        .map(|c| c.fg)
        .collect();
    assert!(
        seen.contains(&p.series[0]),
        "top model uses the first series colour: {seen:?}"
    );
    assert!(seen.len() >= 2, "stacked by model: {seen:?}");
    assert!(
        seen.iter().all(|c| p.series.contains(c)),
        "only theme colours in bars: {seen:?}"
    );
}

#[test]
fn switching_theme_changes_the_next_frame() {
    let mut app = themed(ThemeId::Mainframe, true);
    let before = render_buf(&mut app, 100, 30);
    app.theme = ThemeId::Amber;
    let after = render_buf(&mut app, 100, 30);
    // (99, 29) is empty footer space: pure ground.
    assert_eq!(before[(99, 29)].bg, Color::Rgb(0, 0, 0));
    assert_eq!(after[(99, 29)].bg, Color::Rgb(0x15, 0x0F, 0x08));
    // Text content is identical; only styling changed.
    let text = |b: &Buffer| b.content.iter().map(|c| c.symbol().to_string()).collect::<String>();
    assert_eq!(text(&before), text(&after));
}

#[test]
fn catppuccin_paints_mocha_base_with_mauve_accents() {
    let mut app = themed(ThemeId::Catppuccin, true);
    let buf = render_buf(&mut app, 120, 34);
    let base = Color::Rgb(0x1E, 0x1E, 0x2E);
    let on_base = buf.content.iter().filter(|c| c.bg == base).count();
    assert!(
        on_base > buf.content.len() * 9 / 10,
        "{on_base} of {}",
        buf.content.len()
    );
    let others: std::collections::HashSet<_> = buf.content.iter().filter(|c| c.bg != base).map(|c| c.bg).collect();
    let allowed = [Color::Rgb(0x31, 0x32, 0x44), Color::Rgb(0xCB, 0xA6, 0xF7)]; // surface0 row, mauve chip
    assert!(others.iter().all(|c| allowed.contains(c)), "{others:?}");
    assert!(
        buf.content.iter().any(|c| c.fg == Color::Rgb(0xCD, 0xD6, 0xF4)),
        "text colour used"
    );
}

// ------------------------------------------------------------ golden frames
//
// Whole-frame text snapshots in tests/golden/. They catch layout regressions the targeted assertions
// above don't think to check. After an intended UI change, review the diff and regenerate with:
//   UPDATE_GOLDEN=1 cargo test -p cc-usage golden
// Views that print local clock times (Sessions, Live) are left out so the files don't depend on the timezone.

fn golden(name: &str, frame: &str) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(format!("{name}.txt"));
    let trimmed: String = frame.lines().map(|l| format!("{}\n", l.trim_end())).collect();
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &trimmed).unwrap();
        return;
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing {}; create it with UPDATE_GOLDEN=1", path.display()));
    assert!(
        trimmed == want,
        "frame differs from {}\n--- got ---\n{trimmed}\n--- want ---\n{want}",
        path.display()
    );
}

fn golden_app() -> App {
    app_with(45, Range::Month)
}

#[test]
fn golden_daily_at_each_width_tier() {
    for (w, h) in [(60, 20), (100, 30), (140, 40)] {
        golden(&format!("daily_{w}x{h}"), &render(&mut golden_app(), w, h));
    }
}

#[test]
fn golden_models_projects_and_tokens_metric() {
    let mut a = golden_app();
    a.view = View::Models;
    golden("models_100x30", &render(&mut a, 100, 30));
    a.view = View::Projects;
    golden("projects_100x30", &render(&mut a, 100, 30));
    let mut a = golden_app();
    a.metric = crate::app::Metric::Tokens;
    a.recompute();
    golden("daily_tokens_100x30", &render(&mut a, 100, 30));
}

#[test]
fn golden_filtered_compact_and_help() {
    let mut a = golden_app();
    a.model_filter = Some(0);
    a.recompute();
    golden("daily_filtered_100x30", &render(&mut a, 100, 30));
    golden("compact_30x8", &render(&mut golden_app(), 30, 8));
    let mut a = golden_app();
    a.show_help = true;
    golden("help_100x30", &render(&mut a, 100, 30));
}

#[test]
fn unparsed_transcripts_get_their_own_message() {
    let mut app = App::new(Range::Week);
    app.have_data = true;
    app.ds = Arc::new(Dataset {
        files: 3,
        ..Default::default()
    });
    app.roots = vec!["/somewhere".into()];
    let s = render(&mut app, 100, 10);
    assert!(s.contains("Found 3 transcript files but no usage records"), "{s}");
    assert!(s.contains("log format has changed"), "{s}");
    assert!(s.contains("/somewhere"), "{s}");
    assert!(!s.contains("No Claude Code usage found"), "{s}");
}

#[test]
fn stat_strip_never_shows_a_half_cut_item() {
    // The full strip, at a width where everything fits, gives the only legitimate prefixes.
    let strip_line = |w: u16| {
        let mut app = app_with(45, Range::Month);
        render(&mut app, w, 20)
            .lines()
            .find(|l| l.starts_with(" Tokens"))
            .map(|l| l.trim_end().to_string())
    };
    let full = strip_line(200).expect("strip at 200 cols");
    let items: Vec<&str> = full.split(" │ ").collect();
    assert_eq!(items.len(), 4, "{full}");
    let boundaries: Vec<String> = (1..=items.len()).map(|n| items[..n].join(" │ ")).collect();
    for w in 36..=129 {
        let line = strip_line(w).unwrap_or_else(|| panic!("no strip at width {w}"));
        assert!(boundaries.contains(&line), "width {w} shows a partial item: {line:?}");
    }
}

#[test]
fn zero_sized_panes_draw_nothing_and_do_not_panic() {
    let mut app = app_with(10, Range::Week);
    for (w, h) in [(0, 0), (0, 10), (10, 0)] {
        assert!(render(&mut app, w, h).trim().is_empty());
    }
}

#[test]
fn header_shows_scanning_while_a_scan_is_in_flight() {
    let mut app = app_with(10, Range::Week);
    assert!(!render(&mut app, 100, 24).contains("scanning"));
    app.scanning = true;
    assert!(render(&mut app, 100, 24).contains("scanning"));
}

#[test]
fn cost_card_says_n_a_when_nothing_in_view_is_priced() {
    let mut app = app_with(10, Range::Week);
    app.model_filter = Some(3); // "mystery-model" has no price
    app.recompute();
    let s = render(&mut app, 140, 40);
    assert!(s.contains("n/a"), "{s}");
}

#[test]
fn footer_shows_the_latest_notice() {
    let mut app = app_with(10, Range::Week);
    app.on_key(ratatui::crossterm::event::KeyEvent::new(
        ratatui::crossterm::event::KeyCode::Char('a'),
        ratatui::crossterm::event::KeyModifiers::NONE,
    ));
    let s = render(&mut app, 100, 24);
    assert!(s.contains("theme: Amber"), "{s}");
    assert!(
        !s.contains("q quit"),
        "the notice replaces the key hints until the next key"
    );
}

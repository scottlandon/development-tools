//! UI state and input handling. Rendering lives in `ui/`.

use crate::agg::{self, Agg, Filter, Live, LiveWindow, Range, SessionAgg, Summary};
use crate::data::Dataset;
use crate::timeutil;
use crate::ui::theme::ThemeId;
use crate::worker::Msg;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::widgets::TableState;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Daily,
    Models,
    Projects,
    Sessions,
    Live,
}

impl View {
    pub const ALL: [View; 5] = [View::Daily, View::Models, View::Projects, View::Sessions, View::Live];
    pub fn idx(self) -> usize {
        match self {
            View::Daily => 0,
            View::Models => 1,
            View::Projects => 2,
            View::Sessions => 3,
            View::Live => 4,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            View::Daily => "Daily",
            View::Models => "Models",
            View::Projects => "Projects",
            View::Sessions => "Sessions",
            View::Live => "Live",
        }
    }
    pub fn short(self) -> &'static str {
        match self {
            View::Daily => "Day",
            View::Models => "Mdl",
            View::Projects => "Prj",
            View::Sessions => "Ses",
            View::Live => "Liv",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    Cost,
    Tokens,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SessSort {
    Recent,
    Cost,
    Tokens,
}

impl SessSort {
    pub fn label(self) -> &'static str {
        match self {
            SessSort::Recent => "recent",
            SessSort::Cost => "cost",
            SessSort::Tokens => "tokens",
        }
    }
}

#[expect(
    clippy::struct_excessive_bools,
    reason = "independent UI state flags; a state enum would not simplify them"
)]
pub struct App {
    pub ds: Arc<Dataset>,
    pub have_data: bool,
    pub scanning: bool,
    pub view: View,
    pub range: Range,
    pub metric: Metric,
    pub model_filter: Option<u16>,
    pub project_filter: Option<u16>,
    pub sess_sort: SessSort,
    pub summary: Summary,
    pub models_sorted: Vec<(u16, Agg)>,
    pub projects_sorted: Vec<(u16, Agg)>,
    pub sess_order: Vec<usize>,
    pub theme: ThemeId,
    /// Whether to emit 24-bit colour (else the nearest xterm-256 index).
    pub truecolor: bool,
    pub tables: [TableState; 5],
    pub live_window: LiveWindow,
    pub live: Live,
    pub today: i32,
    pub show_help: bool,
    pub quit: bool,
    pub dirty: bool,
    pub rescan_requested: bool,
    pub notice: Option<String>,
    /// Transcript directories being scanned (shown when nothing is found).
    pub roots: Vec<String>,
}

impl App {
    pub fn new(range: Range) -> App {
        let mut tables: [TableState; 5] = Default::default();
        for t in &mut tables {
            t.select(Some(0));
        }
        App {
            ds: Arc::new(Dataset::default()),
            have_data: false,
            scanning: true,
            view: View::Daily,
            range,
            metric: Metric::Cost,
            model_filter: None,
            project_filter: None,
            sess_sort: SessSort::Recent,
            summary: Summary::default(),
            models_sorted: Vec::new(),
            projects_sorted: Vec::new(),
            sess_order: Vec::new(),
            theme: ThemeId::default(),
            truecolor: true,
            tables,
            live_window: LiveWindow::default(),
            live: Live::default(),
            today: timeutil::today(),
            show_help: false,
            quit: false,
            dirty: true,
            rescan_requested: false,
            notice: None,
            roots: Vec::new(),
        }
    }

    pub fn filter(&self) -> Filter {
        Filter {
            range_days: self.range.days(),
            model: self.model_filter,
            project: self.project_filter,
        }
    }

    pub fn recompute(&mut self) {
        self.summary = agg::compute(&self.ds, self.filter(), self.today);
        let key = |a: &Agg, m: Metric| match m {
            Metric::Cost => a.cost,
            Metric::Tokens => a.total() as f64,
        };
        let metric = self.metric;
        let sort = |v: &Vec<(u16, Agg)>| {
            let mut v = v.clone();
            v.sort_by(|a, b| {
                key(&b.1, metric)
                    .partial_cmp(&key(&a.1, metric))
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            v
        };
        self.models_sorted = sort(&self.summary.by_model);
        self.projects_sorted = sort(&self.summary.by_project);
        self.resort_sessions();
        // Keep selections in range (Daily is newest-first, so row 0 stays "today").
        for v in View::ALL {
            let n = self.row_count(v);
            let t = &mut self.tables[v.idx()];
            let sel = t.selected().unwrap_or(0).min(n.saturating_sub(1));
            t.select(if n == 0 { None } else { Some(sel) });
        }
        self.recompute_live();
        self.dirty = true;
    }

    /// Cheap (binary search + a few hundred rows); safe to call every second.
    pub fn recompute_live(&mut self) {
        let f = Filter {
            range_days: None,
            model: self.model_filter,
            project: self.project_filter,
        };
        self.live = agg::compute_live(&self.ds, f, self.live_window, chrono::Utc::now().timestamp());
        self.dirty = true;
    }

    fn resort_sessions(&mut self) {
        let s: &[SessionAgg] = &self.summary.sessions;
        let mut ord: Vec<usize> = (0..s.len()).collect();
        match self.sess_sort {
            SessSort::Recent => {} // already newest-first
            SessSort::Cost => ord.sort_by(|&a, &b| {
                s[b].agg
                    .cost
                    .partial_cmp(&s[a].agg.cost)
                    .unwrap_or(std::cmp::Ordering::Equal)
            }),
            SessSort::Tokens => ord.sort_by(|&a, &b| s[b].agg.total().cmp(&s[a].agg.total())),
        }
        self.sess_order = ord;
    }

    pub fn row_count(&self, v: View) -> usize {
        match v {
            View::Daily => self.summary.days.len(),
            View::Models => self.models_sorted.len(),
            View::Projects => self.projects_sorted.len(),
            View::Sessions => self.sess_order.len(),
            View::Live => 0,
        }
    }

    pub fn on_msg(&mut self, m: Msg) {
        match m {
            Msg::Data(ds) => {
                self.ds = ds;
                self.have_data = true;
                self.today = timeutil::today();
                // Drop filters pointing at ids that no longer exist.
                if self.model_filter.is_some_and(|m| m as usize >= self.ds.models.len()) {
                    self.model_filter = None;
                }
                if self
                    .project_filter
                    .is_some_and(|p| p as usize >= self.ds.projects.len())
                {
                    self.project_filter = None;
                }
                self.recompute();
            }
            Msg::Scanning(s) => {
                // Only the first load is worth a spinner; background rescans (which
                // the Live view triggers every second) would just flicker.
                let s = s && !self.have_data;
                if self.scanning != s {
                    self.scanning = s;
                    self.dirty = true;
                }
            }
        }
    }

    /// Called once a second: roll the "today" boundary at midnight, and keep
    /// the Live view moving (the window slides even when nothing new arrives).
    pub fn tick(&mut self) {
        let t = timeutil::today();
        if t != self.today {
            self.today = t;
            self.recompute();
        }
        if self.view == View::Live && self.have_data {
            self.recompute_live();
            self.rescan_requested = true;
        }
    }

    fn cycle<T: Copy + PartialEq>(cur: Option<T>, opts: &[T]) -> Option<T> {
        match cur {
            None => opts.first().copied(),
            Some(c) => match opts.iter().position(|o| *o == c) {
                Some(i) if i + 1 < opts.len() => Some(opts[i + 1]),
                _ => None,
            },
        }
    }

    fn select_by(&mut self, delta: isize) {
        let n = self.row_count(self.view);
        if n == 0 {
            return;
        }
        let t = &mut self.tables[self.view.idx()];
        let cur = t.selected().unwrap_or(0).cast_signed();
        t.select(Some((cur + delta).clamp(0, n.cast_signed() - 1).cast_unsigned()));
        self.dirty = true;
    }

    pub fn on_key(&mut self, k: KeyEvent) {
        if k.kind == KeyEventKind::Release {
            return;
        }
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.show_help {
            self.show_help = false;
            self.dirty = true;
            if !matches!(k.code, KeyCode::Char('q')) {
                return;
            }
        }
        self.notice = None;
        match k.code {
            KeyCode::Char('c') if ctrl => self.quit = true,
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => {
                if self.model_filter.is_some() || self.project_filter.is_some() {
                    self.model_filter = None;
                    self.project_filter = None;
                    self.recompute();
                }
            }
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = true,
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.go_view(1),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.go_view(-1),
            KeyCode::Char(c @ '1'..='5') => {
                self.view = View::ALL[c as usize - '1' as usize];
            }
            KeyCode::Down | KeyCode::Char('j') => self.select_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.select_by(-1),
            KeyCode::PageDown => self.select_by(10),
            KeyCode::PageUp => self.select_by(-10),
            KeyCode::Char('d') if ctrl => self.select_by(10),
            KeyCode::Char('u') if ctrl => self.select_by(-10),
            KeyCode::Home | KeyCode::Char('g') => self.select_by(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.select_by(isize::MAX / 2),
            KeyCode::Char('t') if self.view == View::Live => {
                self.live_window = self.live_window.next();
                self.recompute_live();
            }
            KeyCode::Char('T') if self.view == View::Live => {
                self.live_window = self.live_window.prev();
                self.recompute_live();
            }
            KeyCode::Char('t') => {
                self.range = self.range.next();
                self.recompute();
            }
            KeyCode::Char('T') => {
                for _ in 0..Range::ALL.len() - 1 {
                    self.range = self.range.next();
                }
                self.recompute();
            }
            KeyCode::Char('a') => self.set_theme(self.theme.next()),
            KeyCode::Char('A') => self.set_theme(self.theme.prev()),
            KeyCode::Char('c') => {
                self.metric = if self.metric == Metric::Cost {
                    Metric::Tokens
                } else {
                    Metric::Cost
                };
                self.recompute();
            }
            KeyCode::Char('m') => {
                let opts: Vec<u16> = self.summary.by_model.iter().map(|(m, _)| *m).collect();
                self.model_filter = Self::cycle(self.model_filter, &opts);
                self.recompute();
            }
            KeyCode::Char('p') => {
                let opts: Vec<u16> = self.summary.by_project.iter().map(|(p, _)| *p).collect();
                self.project_filter = Self::cycle(self.project_filter, &opts);
                self.recompute();
            }
            KeyCode::Char('s') => {
                self.sess_sort = match self.sess_sort {
                    SessSort::Recent => SessSort::Cost,
                    SessSort::Cost => SessSort::Tokens,
                    SessSort::Tokens => SessSort::Recent,
                };
                self.resort_sessions();
                self.tables[View::Sessions.idx()].select(Some(0));
                self.dirty = true;
            }
            KeyCode::Char('r') => {
                self.rescan_requested = true;
                self.notice = Some("rescanning…".into());
            }
            KeyCode::Enter => self.apply_selection_as_filter(),
            _ => return,
        }
        if self.view == View::Live {
            self.recompute_live(); // never show a stale window when switching in
        }
        self.dirty = true;
    }

    fn set_theme(&mut self, t: ThemeId) {
        self.theme = t;
        self.notice = Some(format!("theme: {}", t.label()));
    }

    fn go_view(&mut self, d: isize) {
        let n = View::ALL.len().cast_signed();
        self.view = View::ALL[(self.view.idx().cast_signed() + d).rem_euclid(n).cast_unsigned()];
    }

    fn apply_selection_as_filter(&mut self) {
        let sel = self.tables[self.view.idx()].selected();
        match (self.view, sel) {
            (View::Models, Some(i)) if i < self.models_sorted.len() => {
                let m = self.models_sorted[i].0;
                self.model_filter = if self.model_filter == Some(m) { None } else { Some(m) };
                self.recompute();
            }
            (View::Projects, Some(i)) if i < self.projects_sorted.len() => {
                let p = self.projects_sorted[i].0;
                self.project_filter = if self.project_filter == Some(p) { None } else { Some(p) };
                self.recompute();
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    #[test]
    fn view_idx_matches_all_order() {
        for (n, v) in View::ALL.into_iter().enumerate() {
            assert_eq!(v.idx(), n, "{v:?}: idx must be its position in View::ALL");
            assert_eq!(View::ALL[v.idx()], v);
        }
    }

    #[test]
    fn view_navigation_wraps() {
        let mut a = App::new(Range::Week);
        a.on_key(key(KeyCode::Char('h')));
        assert_eq!(a.view, View::Live, "Live is the last tab, so h from Daily wraps to it");
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.view, View::Daily);
        a.on_key(key(KeyCode::Char('3')));
        assert_eq!(a.view, View::Projects);
        a.on_key(key(KeyCode::Char('5')));
        assert_eq!(a.view, View::Live);
    }

    #[test]
    fn range_cycles_and_quit() {
        let mut a = App::new(Range::Today);
        a.on_key(key(KeyCode::Char('t')));
        assert_eq!(a.range, Range::Week);
        a.on_key(key(KeyCode::Char('T')));
        assert_eq!(a.range, Range::Today);
        a.on_key(key(KeyCode::Char('q')));
        assert!(a.quit);
    }

    #[test]
    fn t_cycles_live_window_only_on_live_tab() {
        let mut a = App::new(Range::Week);
        a.on_key(key(KeyCode::Char('5')));
        a.on_key(key(KeyCode::Char('t')));
        assert_eq!(a.live_window, LiveWindow::H6);
        assert_eq!(a.range, Range::Week, "range untouched on the Live tab");
        a.on_key(key(KeyCode::Char('T')));
        a.on_key(key(KeyCode::Char('T')));
        assert_eq!(a.live_window, LiveWindow::M15);
        a.on_key(key(KeyCode::Char('1')));
        a.on_key(key(KeyCode::Char('t')));
        assert_eq!(a.range, Range::Month);
        assert_eq!(a.live_window, LiveWindow::M15);
    }

    #[test]
    fn live_tab_asks_for_rescans_each_tick() {
        let mut a = App::new(Range::Week);
        a.have_data = true;
        a.tick();
        assert!(!a.rescan_requested, "other tabs don't poll");
        a.on_key(key(KeyCode::Char('5')));
        a.tick();
        assert!(a.rescan_requested);
    }

    #[test]
    fn a_cycles_themes_both_ways_and_announces_it() {
        let mut a = App::new(Range::Week);
        assert_eq!(a.theme, ThemeId::Mainframe, "Mainframe is the default");
        a.on_key(key(KeyCode::Char('a')));
        assert_eq!(a.theme, ThemeId::Amber);
        assert_eq!(a.notice.as_deref(), Some("theme: Amber"));
        a.on_key(key(KeyCode::Char('A')));
        a.on_key(key(KeyCode::Char('A')));
        assert_eq!(a.theme, ThemeId::Catppuccin, "wraps backwards");
    }

    #[test]
    fn cycle_wraps_to_none() {
        assert_eq!(App::cycle(None, &[3u16, 5]), Some(3));
        assert_eq!(App::cycle(Some(3u16), &[3, 5]), Some(5));
        assert_eq!(App::cycle(Some(5u16), &[3, 5]), None);
        assert_eq!(App::cycle(None, &[] as &[u16]), None);
    }

    // ----- behaviour with data loaded

    fn loaded(range: Range) -> App {
        let mut a = App::new(range);
        a.on_msg(Msg::Data(Arc::new(crate::demo::dataset(
            chrono::Utc::now().timestamp(),
        ))));
        a
    }

    fn press(a: &mut App, c: char) {
        a.on_key(key(KeyCode::Char(c)));
    }

    fn press_ctrl(a: &mut App, c: char) {
        a.on_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
    }

    #[test]
    fn data_fills_the_summary_and_the_spinner_only_shows_for_the_first_load() {
        let mut a = App::new(Range::Week);
        assert!(a.scanning && !a.have_data, "starts out waiting for the first scan");
        a.on_msg(Msg::Data(Arc::new(crate::demo::dataset(
            chrono::Utc::now().timestamp(),
        ))));
        assert!(a.have_data);
        assert!(a.summary.total.msgs > 0 && a.summary.total.cost > 0.0);
        assert!(!a.models_sorted.is_empty() && !a.projects_sorted.is_empty() && !a.sess_order.is_empty());
        a.on_msg(Msg::Scanning(false));
        assert!(!a.scanning);
        a.on_msg(Msg::Scanning(true));
        assert!(!a.scanning, "background rescans must not bring the spinner back");
    }

    #[test]
    fn a_new_snapshot_drops_filters_that_point_at_vanished_ids() {
        let mut a = loaded(Range::All);
        a.model_filter = Some(999);
        a.project_filter = Some(999);
        a.on_msg(Msg::Data(Arc::new(crate::demo::dataset(
            chrono::Utc::now().timestamp(),
        ))));
        assert_eq!((a.model_filter, a.project_filter), (None, None));
        a.model_filter = Some(0);
        a.on_msg(Msg::Data(Arc::new(crate::demo::dataset(
            chrono::Utc::now().timestamp(),
        ))));
        assert_eq!(a.model_filter, Some(0), "a valid filter survives a rescan");
    }

    #[test]
    fn m_walks_through_every_model_then_clears() {
        let mut a = loaded(Range::All);
        let all_msgs = a.summary.total.msgs;
        let models: Vec<u16> = a.models_sorted.iter().map(|m| m.0).collect();
        assert!(models.len() >= 3, "demo data uses several models");
        let msgs_by_model: std::collections::HashMap<u16, u64> =
            a.models_sorted.iter().map(|(m, agg)| (*m, agg.msgs)).collect();

        let mut visited = Vec::new();
        for _ in 0..models.len() {
            press(&mut a, 'm');
            let m = a.model_filter.expect("another model to pick");
            visited.push(m);
            assert_eq!(
                a.summary.total.msgs, msgs_by_model[&m],
                "totals narrow to exactly that model"
            );
            assert_eq!(
                a.summary.by_model.len(),
                models.len(),
                "the sidebar still lists every model"
            );
        }
        visited.sort_unstable();
        let mut want = models.clone();
        want.sort_unstable();
        assert_eq!(visited, want, "every model is reachable");
        press(&mut a, 'm');
        assert_eq!(a.model_filter, None, "one more press clears the filter");
        assert_eq!(a.summary.total.msgs, all_msgs);
    }

    #[test]
    fn p_walks_through_every_project_then_clears() {
        let mut a = loaded(Range::All);
        let all_msgs = a.summary.total.msgs;
        let projects = a.projects_sorted.len();
        assert!(projects >= 3);
        let mut visited = Vec::new();
        for _ in 0..projects {
            press(&mut a, 'p');
            visited.push(a.project_filter.expect("another project to pick"));
            assert!(a.summary.total.msgs < all_msgs);
        }
        visited.sort_unstable();
        visited.dedup();
        assert_eq!(visited.len(), projects, "every project is reachable");
        press(&mut a, 'p');
        assert_eq!(a.project_filter, None);
    }

    #[test]
    fn esc_clears_both_filters() {
        let mut a = loaded(Range::All);
        press(&mut a, 'm');
        press(&mut a, 'p');
        assert!(a.model_filter.is_some() && a.project_filter.is_some());
        a.on_key(key(KeyCode::Esc));
        assert_eq!((a.model_filter, a.project_filter), (None, None));
    }

    #[test]
    fn c_toggles_the_metric_and_resorts_by_it() {
        let mut a = loaded(Range::All);
        assert_eq!(a.metric, Metric::Cost);
        let by_cost: Vec<f64> = a.models_sorted.iter().map(|m| m.1.cost).collect();
        assert!(by_cost.windows(2).all(|w| w[0] >= w[1]), "sorted by cost, descending");
        press(&mut a, 'c');
        assert_eq!(a.metric, Metric::Tokens);
        let by_tokens: Vec<u64> = a.models_sorted.iter().map(|m| m.1.total()).collect();
        assert!(
            by_tokens.windows(2).all(|w| w[0] >= w[1]),
            "sorted by tokens, descending"
        );
        press(&mut a, 'c');
        assert_eq!(a.metric, Metric::Cost);
    }

    #[test]
    fn s_cycles_the_session_sort_and_resets_the_selection() {
        let mut a = loaded(Range::All);
        a.view = View::Sessions;
        let recent: Vec<i64> = a.sess_order.iter().map(|&i| a.summary.sessions[i].last).collect();
        assert!(recent.windows(2).all(|w| w[0] >= w[1]), "newest first");
        a.tables[View::Sessions.idx()].select(Some(5));

        press(&mut a, 's');
        assert_eq!(a.sess_sort, SessSort::Cost);
        let costs: Vec<f64> = a.sess_order.iter().map(|&i| a.summary.sessions[i].agg.cost).collect();
        assert!(costs.windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(a.tables[View::Sessions.idx()].selected(), Some(0));

        press(&mut a, 's');
        assert_eq!(a.sess_sort, SessSort::Tokens);
        let toks: Vec<u64> = a
            .sess_order
            .iter()
            .map(|&i| a.summary.sessions[i].agg.total())
            .collect();
        assert!(toks.windows(2).all(|w| w[0] >= w[1]));

        press(&mut a, 's');
        assert_eq!(a.sess_sort, SessSort::Recent);
    }

    #[test]
    fn selection_keys_move_and_clamp() {
        let mut a = loaded(Range::Month);
        let n = a.row_count(View::Daily);
        assert!(n > 12);
        let sel = |a: &App| a.tables[View::Daily.idx()].selected().unwrap();
        assert_eq!(sel(&a), 0);
        press(&mut a, 'j');
        a.on_key(key(KeyCode::Down));
        assert_eq!(sel(&a), 2);
        press(&mut a, 'k');
        a.on_key(key(KeyCode::Up));
        a.on_key(key(KeyCode::Up));
        assert_eq!(sel(&a), 0, "can't go above the first row");
        a.on_key(key(KeyCode::PageDown));
        assert_eq!(sel(&a), 10);
        a.on_key(key(KeyCode::PageUp));
        assert_eq!(sel(&a), 0);
        press_ctrl(&mut a, 'd');
        assert_eq!(sel(&a), 10);
        press_ctrl(&mut a, 'u');
        assert_eq!(sel(&a), 0);
        press(&mut a, 'G');
        assert_eq!(sel(&a), n - 1, "G jumps to the last row");
        a.on_key(key(KeyCode::PageDown));
        assert_eq!(sel(&a), n - 1, "can't go past the last row");
        press(&mut a, 'g');
        assert_eq!(sel(&a), 0);
        a.on_key(key(KeyCode::End));
        assert_eq!(sel(&a), n - 1);
        a.on_key(key(KeyCode::Home));
        assert_eq!(sel(&a), 0);
    }

    #[test]
    fn enter_filters_by_the_selected_model_or_project_and_toggles_off() {
        let mut a = loaded(Range::All);
        a.view = View::Models;
        a.tables[View::Models.idx()].select(Some(1));
        let second = a.models_sorted[1].0;
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.model_filter, Some(second));
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.model_filter, None, "Enter on the active filter removes it");

        a.view = View::Projects;
        a.tables[View::Projects.idx()].select(Some(0));
        let first = a.projects_sorted[0].0;
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.project_filter, Some(first));
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.project_filter, None);

        a.view = View::Daily;
        a.on_key(key(KeyCode::Enter));
        assert_eq!(
            (a.model_filter, a.project_filter),
            (None, None),
            "Enter does nothing on Daily"
        );
    }

    #[test]
    fn help_overlay_swallows_one_key_then_closes() {
        let mut a = loaded(Range::Week);
        press(&mut a, '?');
        assert!(a.show_help);
        press(&mut a, 'c'); // would toggle the metric if it weren't swallowed
        assert!(!a.show_help);
        assert_eq!(a.metric, Metric::Cost, "the key that closes help is not also acted on");
        press(&mut a, '?');
        press(&mut a, 'q');
        assert!(a.quit, "q still quits from the help overlay");
    }

    #[test]
    fn quit_keys_and_ignored_input() {
        let mut a = loaded(Range::Week);
        press_ctrl(&mut a, 'c');
        assert!(a.quit);

        let mut a = loaded(Range::Week);
        let mut release = key(KeyCode::Char('q'));
        release.kind = KeyEventKind::Release;
        a.on_key(release);
        assert!(!a.quit, "key releases are ignored");

        a.dirty = false;
        press(&mut a, 'z');
        assert!(!a.dirty, "an unbound key doesn't force a redraw");
        press(&mut a, 'j');
        assert!(a.dirty);
    }

    #[test]
    fn r_requests_a_rescan_and_says_so() {
        let mut a = loaded(Range::Week);
        press(&mut a, 'r');
        assert!(a.rescan_requested);
        assert_eq!(a.notice.as_deref(), Some("rescanning…"));
        press(&mut a, 'j');
        assert_eq!(a.notice, None, "the notice clears on the next key");
    }

    #[test]
    fn t_walks_all_ranges_and_capital_t_walks_back() {
        let mut a = loaded(Range::Today);
        let mut seen = vec![a.range];
        for _ in 0..Range::ALL.len() - 1 {
            press(&mut a, 't');
            seen.push(a.range);
        }
        assert_eq!(seen, Range::ALL, "t visits every range in order");
        press(&mut a, 't');
        assert_eq!(a.range, Range::Today, "and wraps");
        press(&mut a, 'T');
        assert_eq!(a.range, Range::All);
        // A longer range includes more messages.
        let all = a.summary.total.msgs;
        press(&mut a, 'T');
        press(&mut a, 'T');
        press(&mut a, 'T');
        assert!(a.summary.total.msgs < all);
    }

    #[test]
    fn tick_rolls_the_day_over_at_midnight() {
        let mut a = loaded(Range::Today);
        let today = a.today;
        a.today = today - 1; // pretend the app has been open since yesterday
        a.dirty = false;
        a.tick();
        assert_eq!(a.today, today);
        assert!(a.dirty, "the new day is redrawn");
    }

    #[test]
    fn plain_d_and_u_do_not_page() {
        let mut a = loaded(Range::Month);
        press(&mut a, 'd');
        press(&mut a, 'u');
        assert_eq!(a.tables[View::Daily.idx()].selected(), Some(0));
        press(&mut a, 'G');
        let last = a.tables[View::Daily.idx()].selected();
        press(&mut a, 'u');
        press(&mut a, 'd');
        assert_eq!(a.tables[View::Daily.idx()].selected(), last);
    }

    #[test]
    fn escape_clears_a_single_filter() {
        let mut a = loaded(Range::Month);
        press(&mut a, 'm');
        assert!(a.model_filter.is_some());
        a.on_key(key(KeyCode::Esc));
        assert!(a.model_filter.is_none(), "Esc clears a model filter on its own");
        press(&mut a, 'p');
        assert!(a.project_filter.is_some());
        a.on_key(key(KeyCode::Esc));
        assert!(a.project_filter.is_none(), "and a project filter on its own");
    }

    #[test]
    fn enter_ignores_a_selection_past_the_end() {
        let mut a = loaded(Range::Month);
        for v in [View::Models, View::Projects] {
            a.view = v;
            a.tables[v.idx()].select(Some(10_000));
            a.on_key(key(KeyCode::Enter));
        }
        assert!(a.model_filter.is_none() && a.project_filter.is_none());
    }

    #[test]
    fn tab_and_sort_names_are_short_and_distinct() {
        let views = [View::Daily, View::Models, View::Projects, View::Sessions, View::Live];
        assert_eq!(views.map(View::short), ["Day", "Mdl", "Prj", "Ses", "Liv"]);
        assert_eq!(
            views.map(View::label),
            ["Daily", "Models", "Projects", "Sessions", "Live"]
        );
        assert_eq!(
            [SessSort::Recent, SessSort::Cost, SessSort::Tokens].map(SessSort::label),
            ["recent", "cost", "tokens"]
        );
    }
}

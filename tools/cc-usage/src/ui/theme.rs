//! Colour themes.
//!
//! Mainframe (the default), Amber and Catppuccin all paint their own ground: "pure black" or "mocha
//! base" is the whole point of them. Mainframe is pure black with an electric-green accent, Amber a
//! warm phosphor tube, and Catppuccin the community Mocha flavour (https://catppuccin.com/palette).
//!
//! Themes are written as hex. At draw time they resolve to 24-bit colour where the terminal supports
//! it, otherwise to the nearest xterm-256 index, so a theme degrades instead of breaking.
//!
//! The resolved palette lives in a thread-local that `ui::draw` refreshes every frame. Rendering code
//! asks `pal()` rather than threading a palette through every function, and because it is
//! thread-local the parallel test runner can't make one test see another's theme.

use ratatui::style::{Color, Style};
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ThemeId {
    #[default]
    Mainframe,
    Amber,
    Catppuccin,
}

impl ThemeId {
    pub const ALL: [ThemeId; 3] = [ThemeId::Mainframe, ThemeId::Amber, ThemeId::Catppuccin];

    /// Stable identifier used by `--theme` and the saved-choice file.
    pub fn id(self) -> &'static str {
        match self {
            ThemeId::Mainframe => "mainframe",
            ThemeId::Amber => "amber",
            ThemeId::Catppuccin => "catppuccin",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Mainframe => "Mainframe",
            ThemeId::Amber => "Amber",
            ThemeId::Catppuccin => "Catppuccin",
        }
    }

    pub fn parse(s: &str) -> Option<ThemeId> {
        let s = s.trim().to_ascii_lowercase();
        ThemeId::ALL.into_iter().find(|t| t.id() == s)
    }

    fn shift(self, by: usize) -> ThemeId {
        let i = ThemeId::ALL.iter().position(|t| *t == self).unwrap_or(0);
        ThemeId::ALL[(i + by) % ThemeId::ALL.len()]
    }
    pub fn next(self) -> ThemeId {
        self.shift(1)
    }
    pub fn prev(self) -> ThemeId {
        self.shift(ThemeId::ALL.len() - 1)
    }

    pub fn list() -> String {
        ThemeId::ALL.iter().map(|t| t.id()).collect::<Vec<_>>().join(", ")
    }
}

type Rgb = (u8, u8, u8);

const fn hex(h: u32) -> Rgb {
    ((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

/// A theme as authored.
struct Def {
    bg: Rgb,
    text: Rgb,
    /// Secondary text: labels, hints, zero rows.
    dim: Rgb,
    border: Rgb,
    accent: Rgb,
    /// Text drawn on an accent- or caution-filled chip.
    on_accent: Rgb,
    /// Colour for cost figures. Cost is money out, so it takes the palette's "money out" colour
    /// (magenta on Mainframe, coral on Amber, red on Catppuccin).
    cost: Rgb,
    /// Filter chips and the "this row is the active filter" marker.
    caution: Rgb,
    /// Background of the selected table row.
    sel_bg: Rgb,
    /// Chart / legend series, in rank order.
    series: [Rgb; 8],
}

fn def(id: ThemeId) -> Def {
    match id {
        // Mainframe: pure black, white readout, electric green, the ANSI brights.
        // Cost is magenta, to stand apart from the green accent.
        ThemeId::Mainframe => Def {
            bg: hex(0x000000),
            text: hex(0xE6EDE8),
            dim: hex(0x728679),
            border: hex(0x1F5A3C),
            accent: hex(0x33FF77),
            on_accent: hex(0x001A0A),
            cost: hex(0xFF4FD8),
            caution: hex(0xFF8C42),
            sel_bg: hex(0x143826),
            series: [
                hex(0x33FF77),
                hex(0xF2F2F2),
                hex(0x8A8A8A),
                hex(0xFF5C5C),
                hex(0xFF8C42),
                hex(0xFF4FD8),
                hex(0x3FE0FF),
                hex(0xFFE14D),
            ],
        },
        // Amber: the P3 amber tube. Warm black ground, text in the phosphor's own tone.
        ThemeId::Amber => Def {
            bg: hex(0x150F08),
            text: hex(0xFFD08A),
            dim: hex(0xA07B42),
            border: hex(0x57432A),
            accent: hex(0xFFB000),
            on_accent: hex(0x1A1204),
            cost: hex(0xFF5F56),
            caution: hex(0xFFE066),
            sel_bg: hex(0x30251A),
            series: [
                hex(0xFFB000),
                hex(0xFFF4D6),
                hex(0xFF5F56),
                hex(0x8CE08C),
                hex(0xFF8AC8),
                hex(0xC9A0FF),
                hex(0x7FD3FF),
                hex(0x9C8B76),
            ],
        },
        // Catppuccin Mocha: mauve accent on the "base" ground. Secondary text is overlay2 rather than the
        // dimmer overlay1, which is 4.4:1 on base and would fail the 4.5 floor the tests hold themes to.
        ThemeId::Catppuccin => Def {
            bg: hex(0x1E1E2E),
            text: hex(0xCDD6F4),
            dim: hex(0x9399B2),
            border: hex(0x585B70),
            accent: hex(0xCBA6F7),
            on_accent: hex(0x11111B),
            cost: hex(0xF38BA8),
            caution: hex(0xF9E2AF),
            sel_bg: hex(0x313244),
            series: [
                hex(0xCBA6F7), // mauve
                hex(0xA6E3A1), // green
                hex(0xFAB387), // peach
                hex(0x89B4FA), // blue
                hex(0xF5C2E7), // pink
                hex(0xF9E2AF), // yellow
                hex(0x94E2D5), // teal
                hex(0xF38BA8), // red
            ],
        },
    }
}

/// A theme resolved to terminal colours.
#[derive(Clone, Copy, Debug)]
pub struct Pal {
    pub bg: Color,
    pub text: Color,
    pub dim: Color,
    pub border: Color,
    pub accent: Color,
    pub on_accent: Color,
    pub cost: Color,
    pub caution: Color,
    pub sel_bg: Color,
    pub series: [Color; 8],
}

pub fn resolve(id: ThemeId, truecolor: bool) -> Pal {
    let d = def(id);
    let c = |rgb: Rgb| to_color(rgb, truecolor);
    Pal {
        bg: c(d.bg),
        text: c(d.text),
        dim: c(d.dim),
        border: c(d.border),
        accent: c(d.accent),
        on_accent: c(d.on_accent),
        cost: c(d.cost),
        caution: c(d.caution),
        sel_bg: c(d.sel_bg),
        series: d.series.map(c),
    }
}

fn to_color(rgb: Rgb, truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    } else {
        Color::Indexed(nearest_xterm(rgb))
    }
}

/// Nearest entry of the xterm-256 colour cube / grey ramp (the 16 ANSI slots are skipped: users
/// remap those, so they aren't a reliable target).
pub fn nearest_xterm((r, g, b): Rgb) -> u8 {
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
    let level = |v: u8| -> usize { (0..6).min_by_key(|&i| (LEVELS[i] as i32 - v as i32).abs()).unwrap_or(0) };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;

    let avg = (r as i32 + g as i32 + b as i32) / 3;
    let gi = (((avg - 8) as f32 / 10.0).round() as i32).clamp(0, 23);
    let gv = (8 + 10 * gi) as u8;
    let gray = (gv, gv, gv);

    let dist = |a: Rgb| {
        let d = |x: u8, y: u8| (x as i32 - y as i32).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    if dist(gray) < dist(cube) {
        (232 + gi) as u8
    } else {
        cube_idx as u8
    }
}

/// `COLORTERM=truecolor|24bit` is the de-facto signal (herdr, tmux with Tc, iTerm, Ghostty, WezTerm…).
pub fn detect_truecolor() -> bool {
    is_truecolor(std::env::var("COLORTERM").ok().as_deref())
}

fn is_truecolor(colorterm: Option<&str>) -> bool {
    colorterm.is_some_and(|v| {
        let v = v.to_ascii_lowercase();
        v.contains("truecolor") || v.contains("24bit")
    })
}

thread_local! {
    static CURRENT: Cell<Pal> = Cell::new(resolve(ThemeId::default(), true));
}

pub fn set_current(p: Pal) {
    CURRENT.with(|c| c.set(p));
}

pub fn pal() -> Pal {
    CURRENT.with(|c| c.get())
}

pub fn series(i: usize) -> Color {
    let p = pal();
    p.series[i % p.series.len()]
}

/// Base style for the whole screen: the theme's ground and text colour.
pub fn fill_style() -> Style {
    let p = pal();
    Style::default().bg(p.bg).fg(p.text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum((r, g, b): Rgb) -> f64 {
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)
    }

    /// WCAG contrast ratio.
    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn parse_and_cycle() {
        assert_eq!(ThemeId::parse("Mainframe"), Some(ThemeId::Mainframe));
        assert_eq!(ThemeId::parse(" CATPPUCCIN\n"), Some(ThemeId::Catppuccin));
        assert_eq!(ThemeId::parse("nope"), None);
        assert_eq!(ThemeId::Catppuccin.next(), ThemeId::Mainframe);
        assert_eq!(ThemeId::Mainframe.prev(), ThemeId::Catppuccin);
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::parse(t.id()), Some(t), "ids round-trip");
        }
    }

    #[test]
    fn mainframe_is_the_default() {
        assert_eq!(ThemeId::default(), ThemeId::Mainframe);
        assert_eq!(ThemeId::ALL[0], ThemeId::Mainframe, "first in the cycle too");
    }

    #[test]
    fn xterm_mapping() {
        assert_eq!(nearest_xterm(hex(0x5fafff)), 75);
        assert_eq!(nearest_xterm(hex(0x000000)), 16);
        assert_eq!(nearest_xterm(hex(0xffffff)), 231);
        assert_eq!(nearest_xterm(hex(0x808080)), 244);
        assert_eq!(nearest_xterm(hex(0x585858)), 240);
        assert_eq!(nearest_xterm(hex(0x3a3a3a)), 237);
        // Mainframe green lands on a bright green, not a grey.
        let i = nearest_xterm(hex(0x33FF77));
        assert!((16..232).contains(&(i as u16)), "{i}");
    }

    #[test]
    fn mainframe_is_black_and_green() {
        let p = resolve(ThemeId::Mainframe, true);
        assert_eq!(p.bg, Color::Rgb(0, 0, 0), "pure black");
        assert_eq!(p.accent, Color::Rgb(0x33, 0xFF, 0x77), "electric green");
        assert_eq!(p.text, Color::Rgb(0xE6, 0xED, 0xE8));
        assert_eq!(p.series[0], p.accent, "chart ramp leads with the accent");
    }

    #[test]
    fn catppuccin_is_mocha() {
        let p = resolve(ThemeId::Catppuccin, true);
        assert_eq!(p.bg, Color::Rgb(0x1E, 0x1E, 0x2E), "base");
        assert_eq!(p.text, Color::Rgb(0xCD, 0xD6, 0xF4), "text");
        assert_eq!(p.accent, Color::Rgb(0xCB, 0xA6, 0xF7), "mauve");
        assert_eq!(p.sel_bg, Color::Rgb(0x31, 0x32, 0x44), "surface0");
        assert_eq!(p.series[0], p.accent);
    }

    #[test]
    fn truecolor_flag_selects_rgb_or_indexed() {
        assert!(matches!(resolve(ThemeId::Amber, true).accent, Color::Rgb(..)));
        assert!(matches!(resolve(ThemeId::Amber, false).accent, Color::Indexed(_)));
        assert!(matches!(resolve(ThemeId::Amber, false).bg, Color::Indexed(_)));
    }

    #[test]
    fn every_theme_is_legible() {
        for id in ThemeId::ALL {
            let d = def(id);
            let bg = d.bg;
            let name = id.id();
            assert!(contrast(d.text, bg) >= 7.0, "{name}: body text");
            assert!(contrast(d.dim, bg) >= 4.5, "{name}: dim text");
            assert!(contrast(d.accent, bg) >= 4.5, "{name}: accent");
            assert!(contrast(d.cost, bg) >= 4.5, "{name}: cost figures");
            assert!(contrast(d.border, bg) >= 1.8, "{name}: border visible");
            assert!(contrast(d.text, d.sel_bg) >= 4.5, "{name}: text on selected row");
            assert!(contrast(d.dim, d.sel_bg) >= 3.0, "{name}: dim text on selected row");
            assert!(contrast(d.on_accent, d.accent) >= 4.5, "{name}: text on accent chip");
            assert!(contrast(d.on_accent, d.caution) >= 4.5, "{name}: text on filter chip");
            assert_ne!(d.sel_bg, bg, "{name}: selection must show");
            for (i, s) in d.series.iter().enumerate() {
                assert!(contrast(*s, bg) >= 3.0, "{name}: series {i} against the ground");
            }
            // Series must be tellable apart: no repeated colour anywhere in the ramp.
            for i in 0..8 {
                for j in (i + 1)..8 {
                    assert_ne!(d.series[i], d.series[j], "{name}: series {i} == {j}");
                }
            }
        }
    }

    #[test]
    fn current_theme_is_per_thread() {
        set_current(resolve(ThemeId::Catppuccin, true));
        assert_eq!(pal().accent, Color::Rgb(0xCB, 0xA6, 0xF7));
        std::thread::spawn(|| {
            // A fresh thread starts at the default theme, whatever another thread set.
            assert_eq!(pal().bg, Color::Rgb(0, 0, 0));
        })
        .join()
        .unwrap();
    }

    #[test]
    fn truecolor_detection_follows_colorterm() {
        assert!(is_truecolor(Some("truecolor")));
        assert!(is_truecolor(Some("24bit")));
        assert!(is_truecolor(Some("TrueColor")));
        assert!(!is_truecolor(Some("256color")));
        assert!(!is_truecolor(Some("")));
        assert!(!is_truecolor(None));
    }
}

//! Colour themes.
//!
//! Every theme paints its own ground: "pure black" or "mocha base" is the whole point of them.
//! Mainframe (the default) is pure black with an electric-green accent, Amber a warm phosphor tube,
//! and Catppuccin the community Mocha flavour (<https://catppuccin.com/palette>). Daylight (warm paper,
//! teal accent) is the light theme. Mainframe, Amber and Daylight are the "clean" variants of the palettes of the same names in
//! the source palettes, with the accent, text and chart colours carried over.
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
    Daylight,
}

impl ThemeId {
    pub const ALL: [ThemeId; 4] = [
        ThemeId::Mainframe,
        ThemeId::Amber,
        ThemeId::Catppuccin,
        ThemeId::Daylight,
    ];

    /// Stable identifier used by `--theme`.
    pub fn id(self) -> &'static str {
        match self {
            ThemeId::Mainframe => "mainframe",
            ThemeId::Amber => "amber",
            ThemeId::Catppuccin => "catppuccin",
            ThemeId::Daylight => "daylight",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemeId::Mainframe => "Mainframe",
            ThemeId::Amber => "Amber",
            ThemeId::Catppuccin => "Catppuccin",
            ThemeId::Daylight => "Daylight",
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
    let [_, r, g, b] = h.to_be_bytes();
    (r, g, b)
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
    /// Models are coloured by family from these hues.
    model_hues: FamilyHues,
}

/// One hue per model family, taken from the theme's own palette (see `ui::models`).
#[derive(Clone, Copy, Debug)]
pub struct FamilyHues {
    haiku: Rgb,
    sonnet: Rgb,
    opus: Rgb,
    /// Fable and Mythos.
    fable: Rgb,
    /// `series` slots for models of no recognised family, cycled through. Keeps them off the family hues.
    pub other: &'static [usize],
}

fn def(id: ThemeId) -> Def {
    match id {
        // Mainframe: pure black, white readout, electric green, the ANSI brights (the source palette's
        // "clean" Mainframe). Cost is magenta, to stand apart from the green accent.
        ThemeId::Mainframe => Def {
            bg: hex(0x00_00_00),
            text: hex(0xE6_ED_E8),
            dim: hex(0x72_86_79),
            border: hex(0x2B_7A_52),
            accent: hex(0x33_FF_77),
            on_accent: hex(0x00_1A_0A),
            cost: hex(0xFF_4F_D8),
            caution: hex(0xFF_8C_42),
            sel_bg: hex(0x1F_27_23),
            series: [
                hex(0x33_FF_77),
                hex(0xF2_F2_F2),
                hex(0x8A_8A_8A),
                hex(0xFF_5C_5C),
                hex(0xFF_8C_42),
                hex(0xFF_4F_D8),
                hex(0x3F_E0_FF),
                hex(0xFF_E1_4D),
            ],
            // Phosphor: one green per family, pale Haiku to deep Fable, so weight reads as depth. Models
            // of any other family take white, orange and grey, none of them green.
            model_hues: FamilyHues {
                haiku: hex(0xC8_FF_D9),
                sonnet: hex(0x7D_FF_A6),
                opus: hex(0x33_FF_77),
                fable: hex(0x1F_B5_52),
                other: &[1, 4, 2],
            },
        },
        // Amber: the P3 amber tube. Warm black ground, text in the phosphor's own tone.
        ThemeId::Amber => Def {
            bg: hex(0x15_0F_08),
            text: hex(0xFF_D0_8A),
            dim: hex(0xA0_7B_42),
            border: hex(0x57_43_2A),
            accent: hex(0xFF_B0_00),
            on_accent: hex(0x1A_12_04),
            cost: hex(0xFF_5F_56),
            caution: hex(0xFF_E0_66),
            sel_bg: hex(0x30_25_1A),
            series: [
                hex(0xFF_B0_00),
                hex(0xFF_F4_D6),
                hex(0xFF_5F_56),
                hex(0x8C_E0_8C),
                hex(0xFF_8A_C8),
                hex(0xC9_A0_FF),
                hex(0x7F_D3_FF),
                hex(0x9C_8B_76),
            ],
            // Amber to ember: one shade per family, yellow-amber Haiku to burnt-orange Fable, so weight reads
            // as heat. Models of any other family take sky, green, violet and pink, none of them amber.
            model_hues: FamilyHues {
                haiku: hex(0xFF_E0_66),
                sonnet: hex(0xFF_C2_33),
                opus: hex(0xFF_9A_1F),
                fable: hex(0xE0_66_1A),
                other: &[6, 3, 5, 4],
            },
        },
        ThemeId::Daylight => daylight(),
        // Catppuccin Mocha: mauve accent on the "base" ground. Secondary text is overlay2 rather than the
        // dimmer overlay1, which is 4.4:1 on base and would fail the 4.5 floor the tests hold themes to.
        ThemeId::Catppuccin => Def {
            bg: hex(0x1E_1E_2E),
            text: hex(0xCD_D6_F4),
            dim: hex(0x93_99_B2),
            border: hex(0x58_5B_70),
            accent: hex(0xCB_A6_F7),
            on_accent: hex(0x11_11_1B),
            cost: hex(0xF3_8B_A8),
            caution: hex(0xF9_E2_AF),
            sel_bg: hex(0x31_32_44),
            series: [
                hex(0xCB_A6_F7), // mauve
                hex(0xA6_E3_A1), // green
                hex(0xFA_B3_87), // peach
                hex(0x89_B4_FA), // blue
                hex(0xF5_C2_E7), // pink
                hex(0xF9_E2_AF), // yellow
                hex(0x94_E2_D5), // teal
                hex(0xF3_8B_A8), // red
            ],
            // Lavender to mauve: one shade per family, pale Haiku to deep-mauve Fable, so weight reads as
            // depth around the accent. Sonnet and Opus are the official Mocha lavender and mauve, with a
            // lighter and a deeper step derived. Models of any other family take green, peach, teal and
            // yellow, none of them purple.
            model_hues: FamilyHues {
                haiku: hex(0xD9_DE_FE),
                sonnet: hex(0xB4_BE_FE),
                opus: hex(0xCB_A6_F7),
                fable: hex(0xA4_7F_D6),
                other: &[1, 2, 6, 5],
            },
        },
    }
}

/// Daylight: warm paper ground, teal accent. The accent slot takes
/// the source palette's text-weight teal (#0F726B, 5.1:1) rather than the #0D9488 fill, which is 3.3:1;
/// the chart ramp keeps the fill teal in first place. Selection is the accent at 10% over the paper.
fn daylight() -> Def {
    Def {
        bg: hex(0xF3_F0_EA),
        text: hex(0x1F_1E_1B),
        dim: hex(0x6A_65_5B),
        border: hex(0xB8_B2_A5),
        accent: hex(0x0F_72_6B),
        on_accent: hex(0xFF_FF_FF),
        cost: hex(0xCF_24_24),
        caution: hex(0xAB_50_09),
        sel_bg: hex(0xDC_E7_E0),
        series: [
            hex(0x0D_94_88),
            hex(0x25_63_EB),
            hex(0x7C_3A_ED),
            hex(0xB4_53_09),
            hex(0xDB_27_77),
            hex(0x15_80_3D),
            hex(0xEA_58_0C),
            hex(0x64_74_8B),
        ],
        // Ink: slate greys darkening Haiku to Opus, with Fable in the teal accent so the top family stands
        // out from the greys. Models of any other family take blue, violet, pink and orange, none of them
        // grey or teal.
        model_hues: FamilyHues {
            haiku: hex(0x64_74_8B),
            sonnet: hex(0x47_55_69),
            opus: hex(0x33_41_55),
            fable: hex(0x0F_72_6B),
            other: &[1, 2, 4, 6],
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
    pub model_hues: FamilyHues,
    bg_rgb: Rgb,
    truecolor: bool,
}

impl Pal {
    /// The colour of a model of `tier` (0 Haiku, 1 Sonnet, 2 Opus, 3 Fable or Mythos) with `back`
    /// newer versions in its family out of `of` in all. The newest is the family hue itself; each
    /// older one is blended 30% further toward the ground, with the total capped where the colour
    /// would fall under 3:1 against it, so the versions stay legible and distinct. Resolved like every other colour, so 256-colour terminals get
    /// the nearest xterm index.
    pub fn family_shade(&self, tier: u8, back: usize, of: usize) -> Color {
        let h = self.model_hues;
        let hue = match tier {
            0 => h.haiku,
            1 => h.sonnet,
            2 => h.opus,
            _ => h.fable,
        };
        let bg = self.bg_rgb;
        let blend = |pm: i32| {
            let mix = |a: u8, b: u8| {
                let (a, b) = (i32::from(a), i32::from(b));
                u8::try_from(a + ((b - a) * pm + 500).div_euclid(1000)).unwrap_or(u8::MAX)
            };
            (mix(hue.0, bg.0), mix(hue.1, bg.1), mix(hue.2, bg.2))
        };
        // Largest blend (per mille) that still reads against the ground.
        let (mut lo, mut hi) = (0, 1000);
        while lo < hi {
            let mid = (lo + hi + 1) / 2;
            if contrast(blend(mid), bg) >= 3.0 {
                lo = mid;
            } else {
                hi = mid - 1;
            }
        }
        let step = if of > 1 {
            (lo / i32::try_from(of - 1).unwrap_or(i32::MAX)).min(300)
        } else {
            0
        };
        let pm = step.saturating_mul(i32::try_from(back).unwrap_or(i32::MAX));
        to_color(blend(pm), self.truecolor)
    }
}

/// Relative luminance of an sRGB colour.
fn lum((r, g, b): Rgb) -> f64 {
    let ch = |v: u8| {
        let v = f64::from(v) / 255.0;
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
        model_hues: d.model_hues,
        bg_rgb: d.bg,
        truecolor,
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
    let level = |v: u8| -> usize {
        (0..6)
            .min_by_key(|&i| (i32::from(LEVELS[i]) - i32::from(v)).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (level(r), level(g), level(b));
    let cube = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;

    let avg = (i32::from(r) + i32::from(g) + i32::from(b)) / 3;
    #[expect(
        clippy::cast_possible_truncation,
        reason = "avg is 0..=255, so the rounded value is at most 25"
    )]
    let gi = (((avg - 8) as f32 / 10.0).round() as i32).clamp(0, 23);
    let gv = u8::try_from(8 + 10 * gi).unwrap_or(u8::MAX);
    let gray = (gv, gv, gv);

    let dist = |a: Rgb| {
        let d = |x: u8, y: u8| (i32::from(x) - i32::from(y)).pow(2);
        d(a.0, r) + d(a.1, g) + d(a.2, b)
    };
    if dist(gray) < dist(cube) {
        u8::try_from(232 + gi).unwrap_or(u8::MAX)
    } else {
        u8::try_from(cube_idx).unwrap_or(u8::MAX)
    }
}

/// `COLORTERM=truecolor|24bit` is the de-facto signal (herdr, tmux with Tc, iTerm, Ghostty, `WezTerm`…).
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
    CURRENT.with(std::cell::Cell::get)
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

    #[test]
    fn parse_and_cycle() {
        assert_eq!(ThemeId::parse("Mainframe"), Some(ThemeId::Mainframe));
        assert_eq!(ThemeId::parse(" CATPPUCCIN\n"), Some(ThemeId::Catppuccin));
        assert_eq!(ThemeId::parse("nope"), None);
        assert_eq!(
            ThemeId::parse("strawberry-milk"),
            None,
            "a removed theme is unknown, not an error"
        );
        assert_eq!(ThemeId::Daylight.next(), ThemeId::Mainframe);
        assert_eq!(ThemeId::Mainframe.prev(), ThemeId::Daylight);
        assert_eq!(ThemeId::Catppuccin.next(), ThemeId::Daylight);
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::parse(t.id()), Some(t), "ids round-trip");
            assert_eq!(t.next().prev(), t, "next and prev are inverses");
        }
        assert_eq!(ThemeId::list(), "mainframe, amber, catppuccin, daylight");
    }

    #[test]
    fn mainframe_is_the_default() {
        assert_eq!(ThemeId::default(), ThemeId::Mainframe);
        assert_eq!(ThemeId::ALL[0], ThemeId::Mainframe, "first in the cycle too");
    }

    #[test]
    fn xterm_mapping() {
        assert_eq!(nearest_xterm(hex(0x5f_af_ff)), 75);
        assert_eq!(nearest_xterm(hex(0x00_00_00)), 16);
        assert_eq!(nearest_xterm(hex(0xff_ff_ff)), 231);
        assert_eq!(nearest_xterm(hex(0x80_80_80)), 244);
        assert_eq!(nearest_xterm(hex(0x58_58_58)), 240);
        assert_eq!(nearest_xterm(hex(0x3a_3a_3a)), 237);
        // Mainframe green lands on a bright green, not a grey.
        let i = nearest_xterm(hex(0x33_FF_77));
        assert!((16..232).contains(&u16::from(i)), "{i}");
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
    fn light_theme_follows_its_source_palette() {
        let p = resolve(ThemeId::Daylight, true);
        assert_eq!(p.bg, Color::Rgb(0xF3, 0xF0, 0xEA), "warm paper");
        assert_eq!(p.series[0], Color::Rgb(0x0D, 0x94, 0x88), "teal");
        assert!(lum(def(ThemeId::Daylight).bg) > 0.5, "a light ground");
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

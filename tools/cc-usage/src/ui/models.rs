//! Per-model colours. Most themes colour models by usage rank (`theme::series`); a theme with family
//! hues (Mainframe, Amber, Catppuccin) gives each family its own colour instead, the newest version in the
//! dataset the hue itself and older ones progressively dimmer, so a model keeps its colour whatever
//! the filter or time window does to its rank.

use super::theme::{pal, series};
use crate::data::Dataset;
use ratatui::style::Color;

/// Tier per family. Mythos shares Fable's tier, and so its hue.
const FAMILIES: [(&str, u8); 5] = [("haiku", 0), ("sonnet", 1), ("opus", 2), ("fable", 3), ("mythos", 3)];

/// A model's tier and (major, minor) version, read from its full id: `claude-opus-4-1-20250805` is
/// (2, (4, 1)), `claude-3-5-haiku-20241022` is (0, (3, 5)). `None` when the family is unrecognised.
pub fn tier_version(name: &str) -> Option<(u8, (u32, u32))> {
    let lower = name.to_ascii_lowercase();
    let toks: Vec<&str> = lower
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let (at, tier) = toks.iter().enumerate().find_map(|(i, t)| {
        FAMILIES
            .iter()
            .find(|(family, _)| t.contains(family))
            .map(|(_, tier)| (i, *tier))
    })?;
    // The version follows the family (`opus-4-1`) or, in the older scheme, precedes it
    // (`3-5-haiku`). Parts are 1-2 digits, so a trailing date never counts.
    let before = || {
        let lead: Vec<&str> = toks[..at].iter().copied().skip_while(|t| part(t).is_none()).collect();
        version(&lead)
    };
    let found = version(&toks[at + 1..]).or_else(before);
    Some((tier, found.unwrap_or((0, 0))))
}

fn part(t: &str) -> Option<u32> {
    if t.len() <= 2 && t.bytes().all(|b| b.is_ascii_digit()) {
        t.parse().ok()
    } else {
        None
    }
}

fn version(toks: &[&str]) -> Option<(u32, u32)> {
    let mut nums = toks.iter().map_while(|t| part(t));
    let major = nums.next()?;
    Some((major, nums.next().unwrap_or(0)))
}

/// The colour of model `id` (an index into `ds.models`); `rank` is the current usage rank.
pub fn model_color(ds: &Dataset, rank: &[u32], id: u32) -> Color {
    let by_rank = || series(rank.iter().position(|r| *r == id).unwrap_or(id as usize));
    let p = pal();
    let Some(hues) = p.model_hues else {
        return by_rank();
    };
    let Some(me) = ds.models.get(id as usize).map(|m| m.name.as_str()) else {
        return by_rank();
    };
    let key = |name: &str| tier_version(name).map(|tv| (tv, name.to_owned()));
    let names = || ds.models.iter().map(|m| m.name.as_str());
    if let Some(mine @ ((tier, _), _)) = key(me) {
        // Position among every model of the family in the dataset, so it never depends on the filter.
        let family: Vec<_> = names().filter_map(key).filter(|k| k.0.0 == tier).collect();
        let newer = family.iter().filter(|k| **k > mine).count();
        p.family_shade(tier, newer, family.len()).unwrap_or_else(by_rank)
    } else {
        let nth = names().filter(|n| tier_version(n).is_none() && *n < me).count();
        series(hues.other[nth % hues.other.len()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::ModelInfo;
    use crate::ui::theme::{ThemeId, resolve, set_current};

    fn ds(names: &[&str]) -> Dataset {
        Dataset {
            models: names
                .iter()
                .map(|n| ModelInfo {
                    name: (*n).to_string(),
                    short: (*n).to_string(),
                    price: None,
                })
                .collect(),
            ..Dataset::default()
        }
    }

    fn lum(c: Color) -> f64 {
        let Color::Rgb(r, g, b) = c else {
            panic!("not truecolor: {c:?}")
        };
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

    #[test]
    fn tier_and_version_parsing() {
        assert_eq!(tier_version("claude-haiku-5-5"), Some((0, (5, 5))));
        assert_eq!(tier_version("claude-sonnet-4-6"), Some((1, (4, 6))));
        assert_eq!(tier_version("claude-opus-4-1-20250805"), Some((2, (4, 1))));
        assert_eq!(tier_version("claude-opus-4-20250514"), Some((2, (4, 0))));
        assert_eq!(tier_version("claude-fable-5-2"), Some((3, (5, 2))));
        assert_eq!(tier_version("claude-mythos-5-1"), Some((3, (5, 1))));
        assert_eq!(tier_version("claude-3-5-sonnet-20241022"), Some((1, (3, 5))));
        assert_eq!(tier_version("claude-3-haiku-20240307"), Some((0, (3, 0))));
        assert_eq!(tier_version("claude-opus-4-1[1m]"), Some((2, (4, 1))));
        assert_eq!(tier_version("Claude-FABLE"), Some((3, (0, 0))));
        assert_eq!(tier_version("<synthetic>"), None);
        assert_eq!(tier_version("mystery-model"), None);
        assert_eq!(tier_version(""), None);
    }

    const FIVE: [&str; 5] = [
        "claude-fable-5-2",
        "claude-haiku-5-5",
        "claude-fable-5-1",
        "claude-opus-5-5",
        "claude-sonnet-5-5",
    ];

    fn rgb(hex: u32) -> Color {
        let [_, r, g, b] = hex.to_be_bytes();
        Color::Rgb(r, g, b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// A theme's family hues, the dimming of the older Fable, and nothing sharing a colour.
    fn check_families(id: ThemeId, bg: Color, [haiku, sonnet, opus, fable]: [u32; 4]) {
        set_current(resolve(id, true));
        let d = ds(&FIVE);
        let rank: Vec<u32> = (0..5).collect();
        let col = |i| model_color(&d, &rank, i);
        assert_eq!(col(1), rgb(haiku));
        assert_eq!(col(4), rgb(sonnet));
        assert_eq!(col(3), rgb(opus));
        assert_eq!(col(0), rgb(fable), "newest fable is the family hue");
        let old = col(2);
        assert!(lum(old) < lum(rgb(fable)), "older fable is dimmer: {old:?}");
        assert!(contrast(old, bg) >= 3.0, "{old:?} reads on the ground");
        let (Color::Rgb(r, g, b), Color::Rgb(fr, fg, fb)) = (old, rgb(fable)) else {
            panic!("not truecolor")
        };
        assert!(
            (r.cmp(&g), r.cmp(&b), g.cmp(&b)) == (fr.cmp(&fg), fr.cmp(&fb), fg.cmp(&fb)),
            "dimmed, not shifted to another hue: {old:?}"
        );
        let all: std::collections::HashSet<_> = (0..5).map(col).collect();
        assert_eq!(all.len(), 5, "every model its own colour");
    }

    #[test]
    fn mainframe_colours_models_by_family() {
        let fam = [0xC8_FF_D9, 0x7D_FF_A6, 0x33_FF_77, 0x1F_B5_52];
        check_families(ThemeId::Mainframe, Color::Rgb(0, 0, 0), fam);
        // All green, so depth is the cue: strictly darker from Haiku up to Fable.
        assert!(fam.windows(2).all(|w| lum(rgb(w[0])) > lum(rgb(w[1]))), "{fam:x?}");
        let d = ds(&FIVE);
        let rank: Vec<u32> = (0..5).collect();
        let old = model_color(&d, &rank, 2);
        assert!(contrast(old, Color::Rgb(0, 0, 0)) >= 3.0, "{old:?}");
        for i in [0, 1, 3, 4] {
            assert_ne!(old, model_color(&d, &rank, i), "older fable vs model {i}");
        }
    }

    #[test]
    fn amber_colours_models_by_family() {
        let fam = [0xFF_E0_66, 0xFF_C2_33, 0xFF_9A_1F, 0xE0_66_1A];
        let bg = Color::Rgb(0x15, 0x0F, 0x08);
        check_families(ThemeId::Amber, bg, fam);
        // All amber to ember, so heat is the cue: strictly darker from Haiku up to Fable.
        assert!(fam.windows(2).all(|w| lum(rgb(w[0])) > lum(rgb(w[1]))), "{fam:x?}");
        let d = ds(&FIVE);
        let rank: Vec<u32> = (0..5).collect();
        let old = model_color(&d, &rank, 2);
        assert!(contrast(old, bg) >= 3.0, "{old:?}");
        for i in [0, 1, 3, 4] {
            assert_ne!(old, model_color(&d, &rank, i), "older fable vs model {i}");
        }
    }

    #[test]
    fn catppuccin_colours_models_by_family() {
        let fam = [0xD9_DE_FE, 0xB4_BE_FE, 0xCB_A6_F7, 0xA4_7F_D6];
        let bg = Color::Rgb(0x1E, 0x1E, 0x2E);
        check_families(ThemeId::Catppuccin, bg, fam);
        // All lavender to mauve, so depth is the cue: strictly darker from Haiku up to Fable.
        assert!(fam.windows(2).all(|w| lum(rgb(w[0])) > lum(rgb(w[1]))), "{fam:x?}");
        let d = ds(&FIVE);
        let rank: Vec<u32> = (0..5).collect();
        let old = model_color(&d, &rank, 2);
        assert!(contrast(old, bg) >= 3.0, "{old:?}");
        for i in [0, 1, 3, 4] {
            assert_ne!(old, model_color(&d, &rank, i), "older fable vs model {i}");
        }
    }

    #[test]
    fn dimming_steps_back_one_version_at_a_time() {
        set_current(resolve(ThemeId::Mainframe, true));
        let d = ds(&["claude-opus-4", "claude-opus-4-1", "claude-opus-4-5", "claude-opus-5-1"]);
        let rank: Vec<u32> = (0..4).collect();
        let cols: Vec<Color> = (0..4).rev().map(|i| model_color(&d, &rank, i)).collect();
        assert_eq!(cols[0], rgb(0x33_FF_77));
        for w in cols.windows(2) {
            assert!(lum(w[0]) > lum(w[1]), "each older version dimmer: {w:?}");
            assert!(contrast(w[1], Color::Rgb(0, 0, 0)) >= 3.0, "{w:?}");
        }
    }

    #[test]
    fn mythos_shares_fables_hue_but_not_its_shade() {
        set_current(resolve(ThemeId::Mainframe, true));
        let alone = ds(&["claude-mythos-5-2"]);
        assert_eq!(model_color(&alone, &[0], 0), rgb(0x1F_B5_52));
        let both = ds(&["claude-mythos-5-2", "claude-fable-5-2"]);
        let (m, f) = (model_color(&both, &[0, 1], 0), model_color(&both, &[0, 1], 1));
        assert_ne!(m, f);
        assert_eq!(m, rgb(0x1F_B5_52), "mythos sorts after fable on a version tie");
        assert!(lum(f) < lum(m));
    }

    #[test]
    fn colour_does_not_depend_on_rank() {
        set_current(resolve(ThemeId::Mainframe, true));
        let d = ds(&["claude-haiku-5-5", "claude-opus-5-5", "mystery-model"]);
        for id in 0..3 {
            let a = model_color(&d, &[0, 1, 2], id);
            assert_eq!(a, model_color(&d, &[2, 1, 0], id));
            assert_eq!(a, model_color(&d, &[1], id), "filtered out of the rank too");
        }
    }

    #[test]
    fn unknown_families_take_the_other_slots() {
        let themes: [(_, &[usize]); 3] = [
            (ThemeId::Mainframe, &[1, 4, 2]),
            (ThemeId::Amber, &[6, 3, 5, 4]),
            (ThemeId::Catppuccin, &[1, 2, 6, 5]),
        ];
        for (id, other) in themes {
            let p = resolve(id, true);
            set_current(p);
            let d = ds(&[
                "claude-haiku-5-5",
                "claude-sonnet-5-5",
                "claude-opus-5-5",
                "claude-fable-5-1",
                "mystery-a",
                "mystery-b",
                "mystery-c",
                "mystery-d",
                "mystery-e",
            ]);
            let fam: Vec<Color> = (0..4).map(|i| model_color(&d, &[], i)).collect();
            for n in 0..5_u32 {
                let c = model_color(&d, &[], 4 + n);
                assert_eq!(c, p.series[other[n as usize % other.len()]]);
                assert!(!fam.contains(&c), "{id:?}: never a family colour");
            }
        }
    }

    #[test]
    fn daylight_colours_by_rank() {
        let p = resolve(ThemeId::Daylight, true);
        set_current(p);
        let d = ds(&["claude-haiku-5-5", "claude-opus-5-5"]);
        assert_eq!(model_color(&d, &[1, 0], 1), p.series[0]);
        assert_eq!(model_color(&d, &[1, 0], 0), p.series[1]);
    }

    #[test]
    fn families_resolve_to_xterm_indices_without_truecolor() {
        for id in [ThemeId::Mainframe, ThemeId::Amber, ThemeId::Catppuccin] {
            set_current(resolve(id, false));
            let d = ds(&["claude-haiku-5-5", "claude-fable-5-1", "claude-fable-5-2", "mystery"]);
            for i in 0..4 {
                assert!(matches!(model_color(&d, &[0, 1], i), Color::Indexed(_)));
            }
        }
    }
}

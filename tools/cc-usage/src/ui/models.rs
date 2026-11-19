//! Per-model colours. Most themes colour models by usage rank (`theme::series`); a theme with a
//! model ramp (Mainframe) colours them by tier instead, light Haiku to the darkest Fable, so a model
//! keeps its colour whatever the filter or time window does to its rank.

use super::theme::{pal, series};
use crate::data::Dataset;
use ratatui::style::Color;

/// Tier per family, lightest to darkest. Mythos shares Fable's tier.
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
    let Some(ramp) = p.model_ramp else {
        return by_rank();
    };
    let Some(me) = ds.models.get(id as usize).map(|m| m.name.as_str()) else {
        return by_rank();
    };
    let key = |name: &str| tier_version(name).map(|tv| (tv, name.to_owned()));
    let names = || ds.models.iter().map(|m| m.name.as_str());
    if let Some(mine) = key(me) {
        // Position among every tiered model in the dataset, so it never depends on the filter.
        let tiered: Vec<_> = names().filter_map(key).collect();
        let below = tiered.iter().filter(|k| **k < mine).count();
        p.ramp_step(below, tiered.len()).unwrap_or_else(by_rank)
    } else {
        let nth = names().filter(|n| tier_version(n).is_none() && *n < me).count();
        series(ramp.other[nth % ramp.other.len()])
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

    #[test]
    fn mainframe_models_run_light_to_red_by_tier() {
        set_current(resolve(ThemeId::Mainframe, true));
        let d = ds(&[
            "claude-fable-5-2",
            "claude-haiku-5-5",
            "claude-fable-5-1",
            "claude-opus-5-5",
            "claude-sonnet-5-5",
        ]);
        let rank: Vec<u32> = (0..5).collect();
        // Ascending tier / version: haiku, sonnet, opus, fable-5-1, fable-5-2.
        let cols: Vec<Color> = [1, 4, 3, 2, 0].iter().map(|&i| model_color(&d, &rank, i)).collect();
        for w in cols.windows(2) {
            assert!(lum(w[0]) > lum(w[1]), "strictly darker: {w:?}");
        }
        assert_eq!(cols[4], Color::Rgb(0xFF, 0x5C, 0x5C), "newest fable is the series red");
        let black = lum(Color::Rgb(0, 0, 0));
        for c in &cols {
            assert!((lum(*c) + 0.05) / (black + 0.05) >= 3.0, "{c:?} reads on black");
        }
        let distinct: std::collections::HashSet<_> = cols.iter().collect();
        assert_eq!(distinct.len(), cols.len());
    }

    #[test]
    fn a_lone_model_is_the_dark_end() {
        set_current(resolve(ThemeId::Mainframe, true));
        let d = ds(&["claude-haiku-5-5"]);
        assert_eq!(model_color(&d, &[0], 0), Color::Rgb(0xFF, 0x5C, 0x5C));
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
    fn untiered_models_stay_off_the_ramp() {
        let p = resolve(ThemeId::Mainframe, true);
        set_current(p);
        let d = ds(&["claude-opus-5-5", "mystery-a", "mystery-b"]);
        let (a, b) = (model_color(&d, &[], 1), model_color(&d, &[], 2));
        assert_eq!(a, p.series[0]);
        assert_ne!(a, b);
        assert!(![a, b].contains(&p.series[3]), "never the red");
    }

    #[test]
    fn other_themes_colour_by_rank() {
        let p = resolve(ThemeId::Amber, true);
        set_current(p);
        let d = ds(&["claude-haiku-5-5", "claude-opus-5-5"]);
        assert_eq!(model_color(&d, &[1, 0], 1), p.series[0]);
        assert_eq!(model_color(&d, &[1, 0], 0), p.series[1]);
    }

    #[test]
    fn ramp_resolves_to_xterm_indices_without_truecolor() {
        set_current(resolve(ThemeId::Mainframe, false));
        let d = ds(&["claude-haiku-5-5", "claude-fable-5-2"]);
        assert!(matches!(model_color(&d, &[0, 1], 0), Color::Indexed(_)));
        assert!(matches!(model_color(&d, &[0, 1], 1), Color::Indexed(_)));
    }
}

//! Stacked vertical bar chart drawn straight into the buffer with eighth
//! blocks. Adapts bar width to the room and rebins when there are more days
//! than columns.

use super::fmt;
use super::theme::pal;
use crate::cast;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

pub struct Bucket {
    /// Axis label for the start of this bucket (kept to 5 cells: `MM-DD` / `HH:MM`).
    pub label: String,
    /// (colour index, value), bottom to top.
    pub segs: Vec<(usize, f64)>,
}

impl Bucket {
    fn total(&self) -> f64 {
        self.segs.iter().map(|s| s.1).sum()
    }
}

const LOWER: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// Merge consecutive buckets so at most `max` remain.
pub fn rebin(buckets: &[Bucket], max: usize) -> Vec<Bucket> {
    if max == 0 || buckets.len() <= max {
        return buckets
            .iter()
            .map(|b| Bucket {
                label: b.label.clone(),
                segs: b.segs.clone(),
            })
            .collect();
    }
    let k = buckets.len().div_ceil(max);
    buckets
        .chunks(k)
        .map(|chunk| {
            let mut segs: Vec<(usize, f64)> = Vec::new();
            for b in chunk {
                for &(c, v) in &b.segs {
                    match segs.iter_mut().find(|s| s.0 == c) {
                        Some(s) => s.1 += v,
                        None => segs.push((c, v)),
                    }
                }
            }
            segs.sort_by_key(|s| s.0);
            Bucket {
                label: chunk[0].label.clone(),
                segs,
            }
        })
        .collect()
}

/// Where each bar goes. Separated out so it can be unit-tested.
#[derive(Debug, PartialEq)]
pub struct Geometry {
    /// (x offset within the plot, bar width) per bucket after rebinning.
    pub slots: Vec<(usize, usize)>,
    /// Smallest distance between neighbouring bar starts (for label spacing).
    pub min_slot: usize,
}

/// Bars are spread over the whole plot width (capped at 10 cells per slot so
/// a handful of days doesn't turn into giant blocks), with a 1-cell gap when
/// there's room, otherwise packed 1-wide, rebinned if there are too many.
pub fn geometry(n: usize, plot_w: usize) -> Geometry {
    let n = n.max(1);
    if plot_w >= 2 * n {
        let span = plot_w.min(n * 10);
        let slots: Vec<(usize, usize)> = (0..n)
            .map(|i| {
                let (x, next) = (i * span / n, (i + 1) * span / n);
                (x, ((next - x) - 1).max(1))
            })
            .collect();
        Geometry {
            min_slot: span / n,
            slots,
        }
    } else {
        let cols = n.min(plot_w.max(1));
        Geometry {
            slots: (0..cols).map(|i| (i, 1)).collect(),
            min_slot: 1,
        }
    }
}

pub fn render(buf: &mut Buffer, area: Rect, buckets: &[Bucket], axis: &dyn Fn(f64) -> String, palette: &[Color]) {
    if area.height < 3 || area.width < 8 {
        return;
    }
    let ymax = buckets.iter().map(Bucket::total).fold(0.0, f64::max);
    if buckets.is_empty() || ymax <= 0.0 {
        let msg = "no usage in this range";
        let x = area.x + area.width.saturating_sub(cast::to_u16(msg.len())) / 2;
        buf.set_string(x, area.y + area.height / 2, msg, Style::default().fg(pal().dim));
        return;
    }

    let want_labels = area.height >= 4;
    let bars_h = (area.height as usize) - usize::from(want_labels);
    let top = axis(ymax);
    let mid = axis(ymax / 2.0);
    let axis_w = (top.chars().count().max(mid.chars().count()).max(1) + 1).min(9);
    if area.width as usize <= axis_w + 2 {
        return;
    }
    let plot_w = area.width as usize - axis_w;

    let g = geometry(buckets.len(), plot_w);
    let bins = rebin(buckets, g.slots.len());

    // Axis labels (right-aligned against the plot).
    let axis_style = Style::default().fg(pal().dim);
    let put_axis = |buf: &mut Buffer, row: usize, text: &str| {
        let t = fmt::trunc(text, axis_w - 1);
        let x = area.x + cast::to_u16(axis_w - 1 - t.chars().count());
        buf.set_string(x, area.y + cast::to_u16(row), &t, axis_style);
    };
    put_axis(buf, 0, &top);
    if bars_h >= 6 {
        put_axis(buf, bars_h / 2, &mid);
    }
    put_axis(buf, bars_h - 1, "0");

    let total_units = (bars_h * 8) as f64;
    for (bi, b) in bins.iter().enumerate() {
        let (sx, bar_w) = g.slots[bi];
        let x0 = area.x + cast::to_u16(axis_w + sx);
        let total = b.total();
        if total <= 0.0 {
            // A faint baseline tick so empty days are visible as "nothing".
            buf.set_string(x0, area.y + cast::to_u16(bars_h - 1), "·", axis_style);
            continue;
        }
        let units = cast::f64_to_usize((total / ymax * total_units).round()).clamp(1, bars_h * 8);
        // Cumulative segment boundaries in eighth-cell units.
        let mut bounds = Vec::with_capacity(b.segs.len());
        let mut cum = 0.0;
        for &(c, v) in &b.segs {
            cum += v;
            bounds.push((cast::f64_to_usize((cum / total * units as f64).round()), c));
        }
        for row in 0..bars_h {
            let filled = units.saturating_sub(row * 8).min(8);
            if filled == 0 {
                break;
            }
            let top_unit = row * 8 + filled - 1;
            let colour_ix = bounds
                .iter()
                .find(|(bound, _)| *bound > top_unit)
                .or(bounds.last())
                .map_or(0, |x| x.1);
            let style = Style::default().fg(palette[colour_ix % palette.len()]);
            let ch = LOWER[filled - 1];
            let y = area.y + cast::to_u16(bars_h - 1 - row);
            for dx in 0..bar_w {
                buf[(x0 + cast::to_u16(dx), y)].set_char(ch).set_style(style);
            }
        }
    }

    if want_labels {
        let y = area.y + cast::to_u16(bars_h);
        let step = 6usize.div_ceil(g.min_slot).max(1);
        let last_x = area.x + area.width;
        for (bi, b) in bins.iter().enumerate().step_by(step) {
            let x0 = area.x + cast::to_u16(axis_w + g.slots[bi].0);
            let label = &b.label;
            if x0 + cast::to_u16(label.len()) > last_x {
                break;
            }
            buf.set_string(
                x0,
                y,
                label,
                Style::default().fg(pal().dim).add_modifier(Modifier::empty()),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(day: i32, v: f64) -> Bucket {
        Bucket {
            label: crate::timeutil::fmt_md(day),
            segs: vec![(0, v)],
        }
    }

    #[test]
    fn geometry_scales_with_room() {
        // Few days: capped at 10 cells per slot (9 bar + 1 gap).
        let g = geometry(7, 100);
        assert_eq!(g.slots.len(), 7);
        assert_eq!(g.slots[0], (0, 9));
        assert_eq!(g.slots[1].0, 10);
        // Many days: spread over the full width, widths differ by at most 1, gap kept.
        let g = geometry(30, 100);
        assert_eq!(g.slots.len(), 30);
        assert!(g.slots.iter().all(|&(_, w)| w == 2 || w == 3), "{:?}", g.slots);
        let (lx, lw) = *g.slots.last().unwrap();
        assert!(lx + lw <= 100 && lx + lw >= 98, "uses the full width: {}", lx + lw);
        for w in g.slots.windows(2) {
            assert!(w[0].0 + w[0].1 < w[1].0, "bars never touch: {w:?}");
        }
        // Tight: packed 1-wide, no gap; too many: rebinned down to the width.
        assert_eq!(geometry(30, 45).slots.len(), 30);
        assert!(geometry(30, 45).slots.iter().enumerate().all(|(i, &s)| s == (i, 1)));
        assert_eq!(geometry(200, 80).slots.len(), 80);
    }

    #[test]
    fn rebinning_preserves_totals() {
        let src: Vec<_> = (0..10).map(|i| b(i, 1.0)).collect();
        let out = rebin(&src, 4);
        assert_eq!(out.len(), 4); // chunks of 3,3,3,1
        let sum: f64 = out.iter().map(super::Bucket::total).sum();
        assert!((sum - 10.0).abs() < 1e-9);
        assert_eq!(out[1].label, crate::timeutil::fmt_md(3));
    }

    #[test]
    fn renders_tallest_bar_full_height() {
        let area = Rect::new(0, 0, 30, 8);
        let mut buf = Buffer::empty(area);
        let data = vec![b(20_000, 10.0), b(20_001, 5.0)];
        render(&mut buf, area, &data, &|v| format!("{v:.0}"), &[Color::Cyan]);
        let axis_w: u16 = 3; // "10" + 1
        // Tallest bar: every plot row (7 of them) is a full block.
        for y in 0..7 {
            assert_eq!(buf[(axis_w, y)].symbol(), "█", "row {y}");
        }
        // The half-height bar (second slot, 10 cells in) leaves the top row empty.
        let second = axis_w + 10;
        assert_eq!(buf[(second, 0)].symbol(), " ");
        assert_eq!(buf[(second, 6)].symbol(), "█");
        assert_eq!(buf[(0, 0)].symbol(), "1"); // axis label "10"
    }

    #[test]
    fn tiny_areas_do_not_panic() {
        for (w, h) in [(0, 0), (1, 1), (7, 3), (8, 2), (9, 3), (20, 4)] {
            let area = Rect::new(0, 0, w, h);
            let mut buf = Buffer::empty(area);
            let data: Vec<_> = (0..50).map(|i| b(20_000 + i, f64::from(i))).collect();
            render(
                &mut buf,
                area,
                &data,
                &|v| format!("{v:.0}"),
                &[Color::Red, Color::Blue],
            );
        }
    }
}

//! Number/text formatting that adapts to the room available.

use unicode_width_lite::truncate_to;

/// 3 significant digits with K/M/B suffix: 842, 1.23K, 12.3K, 123K, 1.50M …
pub fn tok(n: u64) -> String {
    let (v, s) = match n {
        0..=999 => return n.to_string(),
        1_000..=999_999 => (n as f64 / 1e3, "K"),
        1_000_000..=999_999_999 => (n as f64 / 1e6, "M"),
        _ => (n as f64 / 1e9, "B"),
    };
    if v < 10.0 {
        format!("{v:.2}{s}")
    } else if v < 100.0 {
        format!("{v:.1}{s}")
    } else {
        format!("{v:.0}{s}")
    }
}

/// Full integer with thousands separators.
pub fn tok_full(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn usd(c: f64) -> String {
    if c >= 10_000.0 {
        format!("${:.1}K", c / 1000.0)
    } else if c >= 1000.0 {
        format!("${}", tok_full(c.round() as u64))
    } else {
        format!("${c:.2}")
    }
}

pub fn pct(f: f64) -> String {
    format!("{:.0}%", f * 100.0)
}

pub fn dur(secs: i64) -> String {
    let s = secs.max(0);
    if s >= 48 * 3600 {
        format!("{}d{:02}h", s / 86_400, (s % 86_400) / 3600)
    } else if s >= 3600 {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m", s / 60)
    } else {
        format!("{s}s")
    }
}

pub fn trunc(s: &str, w: usize) -> String {
    truncate_to(s, w)
}

/// Horizontal bar of `w` cells filled to `frac` (0..=1) using eighth blocks.
pub fn hbar(frac: f64, w: usize) -> String {
    const PART: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let eighths = (frac.clamp(0.0, 1.0) * w as f64 * 8.0).round() as usize;
    let (full, rem) = (eighths / 8, eighths % 8);
    let mut s = "█".repeat(full.min(w));
    if full < w {
        if rem > 0 {
            s.push(PART[rem]);
        }
        let used = full + (rem > 0) as usize;
        s.push_str(&" ".repeat(w.saturating_sub(used)));
    }
    s
}

/// Minimal width handling: treats every `char` as one cell, which is right
/// for everything this tool prints except East-Asian project names. Those
/// merely get truncated a touch early rather than overflowing, since ratatui
/// clips at the real cell width.
mod unicode_width_lite {
    pub fn truncate_to(s: &str, w: usize) -> String {
        if s.chars().count() <= w {
            return s.to_string();
        }
        if w == 0 {
            return String::new();
        }
        let mut out: String = s.chars().take(w - 1).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_tokens() {
        assert_eq!(tok(0), "0");
        assert_eq!(tok(842), "842");
        assert_eq!(tok(1234), "1.23K");
        assert_eq!(tok(12_345), "12.3K");
        assert_eq!(tok(123_456), "123K");
        assert_eq!(tok(1_500_000), "1.50M");
        assert_eq!(tok(2_345_000_000), "2.35B");
    }

    #[test]
    fn grouping() {
        assert_eq!(tok_full(0), "0");
        assert_eq!(tok_full(999), "999");
        assert_eq!(tok_full(1_000), "1,000");
        assert_eq!(tok_full(1_234_567), "1,234,567");
    }

    #[test]
    fn money() {
        assert_eq!(usd(0.0), "$0.00");
        assert_eq!(usd(12.345), "$12.35");
        assert_eq!(usd(1234.4), "$1,234");
        assert_eq!(usd(12_500.0), "$12.5K");
    }

    #[test]
    fn truncation() {
        assert_eq!(trunc("hello", 10), "hello");
        assert_eq!(trunc("hello world", 6), "hello…");
        assert_eq!(trunc("abc", 0), "");
    }

    #[test]
    fn bars_have_exact_width() {
        for f in [0.0, 0.03, 0.5, 0.999, 1.0] {
            assert_eq!(hbar(f, 10).chars().count(), 10, "{f}");
        }
        assert_eq!(hbar(1.0, 4), "████");
        assert_eq!(hbar(0.0, 4), "    ");
    }

    #[test]
    fn durations() {
        assert_eq!(dur(5), "5s");
        assert_eq!(dur(125), "2m");
        assert_eq!(dur(3 * 3600 + 7 * 60), "3h07m");
        assert_eq!(dur(101 * 3600 + 51 * 60), "4d05h");
    }

    #[test]
    fn token_counts_switch_unit_and_precision_at_the_boundaries() {
        assert_eq!(tok(0), "0");
        assert_eq!(tok(999), "999");
        assert_eq!(tok(1_000), "1.00K");
        assert_eq!(tok(9_990), "9.99K");
        assert_eq!(tok(10_000), "10.0K", "10 and up: one decimal");
        assert_eq!(tok(99_900), "99.9K");
        assert_eq!(tok(100_000), "100K", "100 and up: none");
        assert_eq!(tok(1_000_000), "1.00M");
        assert_eq!(tok(12_345_678), "12.3M");
        assert_eq!(tok(1_000_000_000), "1.00B");
    }
}

//! Checked and saturating numeric conversions, so call sites don't need scattered `as` casts
//! (and lint allows). Each helper documents the behaviour for out-of-range input.

/// `usize` to `u16`, saturating at `u16::MAX`. For terminal-cell arithmetic, where every value
/// derives from a `u16` dimension or a short string.
pub fn to_u16(n: usize) -> u16 {
    u16::try_from(n).unwrap_or(u16::MAX)
}

/// `usize` to `i64`, saturating at `i64::MAX`.
pub fn to_i64(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A `Vec` index as one of the dataset's `u16` ids (project and model indices). Wraps, as a bare
/// `as u16` would, above 65535.
#[expect(
    clippy::cast_possible_truncation,
    reason = "TODO(review): wraps silently with more than 65536 distinct projects or models; the u16 ids are never range-checked"
)]
pub fn wrapping_u16_id(ix: usize) -> u16 {
    ix as u16
}

// Rust's float-to-integer `as` is saturating (NaN becomes 0), which is exactly the clamping
// these helpers want; they exist to name that intent in one place.

/// `f64` to `u64`: truncates toward zero, saturates at both ends, NaN becomes 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float-to-int `as` saturates; truncation toward zero is intended"
)]
pub fn f64_to_u64(v: f64) -> u64 {
    v as u64
}

/// `f64` to `usize`: truncates toward zero, saturates at both ends, NaN becomes 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float-to-int `as` saturates; truncation toward zero is intended"
)]
pub fn f64_to_usize(v: f64) -> usize {
    v as usize
}

/// `f64` to `u32`: truncates toward zero, saturates at both ends, NaN becomes 0.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "float-to-int `as` saturates; truncation toward zero is intended"
)]
pub fn f64_to_u32(v: f64) -> u32 {
    v as u32
}

/// `f64` to `i64`: truncates toward zero, saturates at both ends, NaN becomes 0.
#[expect(
    clippy::cast_possible_truncation,
    reason = "float-to-int `as` saturates; truncation toward zero is intended"
)]
pub fn f64_to_i64(v: f64) -> i64 {
    v as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_helpers_saturate() {
        assert_eq!(to_u16(7), 7);
        assert_eq!(to_u16(70_000), u16::MAX);
        assert_eq!(to_i64(7), 7);
    }

    #[test]
    fn float_helpers_truncate_and_saturate() {
        assert_eq!(f64_to_u64(2.9), 2);
        assert_eq!(f64_to_u64(-1.0), 0);
        assert_eq!(f64_to_u64(f64::NAN), 0);
        assert_eq!(f64_to_usize(3.5), 3);
        assert_eq!(f64_to_u32(1e12), u32::MAX);
        assert_eq!(f64_to_i64(-2.9), -2);
    }
}

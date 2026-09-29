//! Stateless randomness for positional generators: a value is a pure function
//! of `(salt, seq, stream)`, so any row can be computed without drawing the
//! rows before it.

/// Fold a column name into a generator seed (FNV-1a, stable across builds —
/// unlike `std`'s hasher, whose algorithm is unspecified).
pub(crate) fn column_salt(seed: u64, column: &str) -> u64 {
    let mut h = 0xcbf2_9ce4_8422_2325_u64 ^ seed;
    for b in column.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// SplitMix64 over the combined inputs.
pub(crate) fn mix(salt: u64, seq: u64, stream: u64) -> u64 {
    let mut z = salt
        .wrapping_add(seq.wrapping_mul(0x9e37_79b9_7f4a_7c15))
        .wrapping_add(stream.wrapping_mul(0xd1b5_4a32_d192_ed03));
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// [`mix`] mapped to `[0, 1)`.
pub(crate) fn unit(salt: u64, seq: u64, stream: u64) -> f64 {
    (mix(salt, seq, stream) >> 11) as f64 / (1u64 << 53) as f64
}

/// Streams keep the positional generators decorrelated when they share a salt.
pub(crate) const STREAM_WALK: u64 = 1;
pub(crate) const STREAM_DATE: u64 = 2;
pub(crate) const STREAM_TREE: u64 = 3;
pub(crate) const STREAM_FAN_OUT: u64 = 4;
pub(crate) const STREAM_TREE_END: u64 = 5;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_is_in_range_and_stable() {
        for seq in 0..1000 {
            let u = unit(42, seq, STREAM_WALK);
            assert!((0.0..1.0).contains(&u));
            assert_eq!(u, unit(42, seq, STREAM_WALK));
        }
        assert_ne!(column_salt(1, "a"), column_salt(1, "b"));
    }
}

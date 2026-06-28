//! Parsing Kubernetes [resource quantities] into plain numbers.
//!
//! K8s expresses CPU and memory as suffixed strings — `"250m"`, `"8"`,
//! `"16331752Ki"`, `"4523122n"`. Charts and comparisons need numbers, so
//! the projector parses them here: CPU into **millicores** (integer),
//! memory (and other binary quantities) into **bytes** (integer).
//!
//! [resource quantities]: https://kubernetes.io/docs/reference/kubernetes-api/common-definitions/quantity/

/// Parse a CPU quantity into millicores.
///
/// - plain cores: `"8"` → `8000`, `"0.5"` → `500`
/// - milli: `"250m"` → `250`
/// - micro / nano (metrics-server usage): `"4523122n"` → `4` (nanocores → m)
pub fn parse_cpu_millicores(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let (num, factor_to_milli): (&str, f64) = if let Some(n) = s.strip_suffix('n') {
        (n, 1.0 / 1_000_000.0) // nanocores → millicores
    } else if let Some(n) = s.strip_suffix('u') {
        (n, 1.0 / 1_000.0) // microcores → millicores
    } else if let Some(n) = s.strip_suffix('m') {
        (n, 1.0) // already millicores
    } else {
        (s, 1_000.0) // whole cores → millicores
    };
    let value: f64 = num.trim().parse().ok()?;
    Some((value * factor_to_milli).round() as i64)
}

/// Parse a memory / storage quantity into bytes.
///
/// Handles binary suffixes (`Ki`, `Mi`, `Gi`, `Ti`, `Pi`, `Ei`), decimal
/// SI suffixes (`k`/`K`, `M`, `G`, `T`, `P`, `E`), and bare byte counts.
pub fn parse_memory_bytes(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }

    const BINARY: &[(&str, i128)] = &[
        ("Ki", 1 << 10),
        ("Mi", 1 << 20),
        ("Gi", 1 << 30),
        ("Ti", 1i128 << 40),
        ("Pi", 1i128 << 50),
        ("Ei", 1i128 << 60),
    ];
    for (suffix, mult) in BINARY {
        if let Some(num) = s.strip_suffix(suffix) {
            return scale(num, *mult);
        }
    }

    const DECIMAL: &[(&str, i128)] = &[
        ("E", 1_000_000_000_000_000_000),
        ("P", 1_000_000_000_000_000),
        ("T", 1_000_000_000_000),
        ("G", 1_000_000_000),
        ("M", 1_000_000),
        ("k", 1_000),
        ("K", 1_000),
    ];
    for (suffix, mult) in DECIMAL {
        if let Some(num) = s.strip_suffix(suffix) {
            return scale(num, *mult);
        }
    }

    // Bare byte count (possibly fractional, e.g. exponent form "1e9").
    let value: f64 = s.parse().ok()?;
    Some(value.round() as i64)
}

fn scale(num: &str, mult: i128) -> Option<i64> {
    let value: f64 = num.trim().parse().ok()?;
    let bytes = (value * mult as f64).round() as i128;
    Some(bytes.clamp(i64::MIN as i128, i64::MAX as i128) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_cores_to_millicores() {
        assert_eq!(parse_cpu_millicores("8"), Some(8000));
        assert_eq!(parse_cpu_millicores("0.5"), Some(500));
        assert_eq!(parse_cpu_millicores("250m"), Some(250));
        assert_eq!(parse_cpu_millicores("4523122n"), Some(5)); // 4.523122 m, rounds to 5
        assert_eq!(parse_cpu_millicores(""), None);
        assert_eq!(parse_cpu_millicores("garbage"), None);
    }

    #[test]
    fn memory_suffixes_to_bytes() {
        assert_eq!(parse_memory_bytes("16331752Ki"), Some(16_723_714_048));
        assert_eq!(parse_memory_bytes("1Gi"), Some(1_073_741_824));
        assert_eq!(parse_memory_bytes("1Mi"), Some(1_048_576));
        assert_eq!(parse_memory_bytes("1M"), Some(1_000_000));
        assert_eq!(parse_memory_bytes("128974848"), Some(128_974_848));
        assert_eq!(parse_memory_bytes("nope"), None);
    }
}

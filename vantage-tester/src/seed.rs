/// The seed a root scenario's random source starts from: 64-bit FNV-1a over the file's
/// project-relative path, a zero byte, and the scenario name. The path is normalised first
/// (`\` read as `/`, `.` and empty segments dropped, `..` resolved), so `./tests//a.yaml`,
/// `tests/x/../a.yaml` and `tests/a.yaml` seed alike.
///
/// It depends on nothing else, so a scenario draws the same ids and random values on every
/// machine and every run. A what-if inherits the state, seed included, of where it branches.
///
/// ```
/// use vantage_tester::seed;
///
/// assert_eq!(seed("tests/scenarios/funding.yaml", "funded"), 0x95d6_2f9f_f16f_f5dd);
/// assert_eq!(seed("./tests\\scenarios/funding.yaml", "funded"), 0x95d6_2f9f_f16f_f5dd);
/// assert_eq!(seed("tests/x/../scenarios/funding.yaml", "funded"), 0x95d6_2f9f_f16f_f5dd);
/// assert_ne!(seed("tests/scenarios/funding.yaml", "funded-by-dave"), 0x95d6_2f9f_f16f_f5dd);
/// ```
pub fn seed(path: &str, scenario: &str) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let path = normalise(path);
    let bytes = path
        .bytes()
        .chain(std::iter::once(0))
        .chain(scenario.bytes());
    bytes.fold(OFFSET, |hash, b| (hash ^ u64::from(b)).wrapping_mul(PRIME))
}

/// A `..` drops the segment before it; one with nothing before it to drop is kept.
pub(crate) fn normalise(path: &str) -> String {
    let path = path.replace('\\', "/");
    let mut kept: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." if kept.last().is_some_and(|s| *s != "..") => {
                kept.pop();
            }
            _ => kept.push(segment),
        }
    }
    kept.join("/")
}

//! Built-in aircraft types and carriers.

/// An aircraft type with the performance figures the sim flies it by.
#[derive(Debug)]
pub struct Aircraft {
    /// ICAO type designator (`A320`, `B789`, …).
    pub code: &'static str,
    pub seats: u32,
    /// Cruise speed, knots.
    pub cruise_kts: f64,
    /// Cruise altitude, feet.
    pub cruise_ft: f64,
    /// Longest route it is given, nautical miles.
    pub range_nm: f64,
    /// Shortest route it is given, nautical miles (keeps widebodies off hops).
    pub min_nm: f64,
}

const fn ac(
    code: &'static str,
    seats: u32,
    cruise_kts: f64,
    cruise_ft: f64,
    range_nm: f64,
    min_nm: f64,
) -> Aircraft {
    Aircraft {
        code,
        seats,
        cruise_kts,
        cruise_ft,
        range_nm,
        min_nm,
    }
}

/// Routes shorter than this go to narrowbodies only.
const NARROWBODY_NM: f64 = 2500.0;

pub static AIRCRAFT: &[Aircraft] = &[
    ac("E190", 100, 447.0, 37_000.0, 2_400.0, 0.0),
    ac("A320", 180, 450.0, 37_000.0, 3_300.0, 0.0),
    ac("B738", 189, 453.0, 37_000.0, 3_000.0, 0.0),
    ac("A21N", 220, 450.0, 37_000.0, 4_000.0, 0.0),
    ac("B789", 290, 488.0, 41_000.0, 7_600.0, 1_500.0),
    ac("A359", 325, 488.0, 41_000.0, 9_700.0, 1_500.0),
    ac("B77W", 396, 490.0, 39_000.0, 7_300.0, 2_000.0),
    ac("A388", 525, 490.0, 39_000.0, 8_000.0, 3_000.0),
];

/// Types that can fly a route of `nm`: narrowbodies below
/// [`NARROWBODY_NM`], anything with the range above it.
pub fn types_for(nm: f64) -> Vec<&'static Aircraft> {
    AIRCRAFT
        .iter()
        .filter(|a| a.range_nm >= nm && a.min_nm <= nm)
        .filter(|a| nm >= NARROWBODY_NM || a.min_nm == 0.0)
        .collect()
}

/// An airline: IATA code, hub airport and registration template (`#` digit,
/// `?` letter).
#[derive(Debug)]
pub struct Carrier {
    pub code: &'static str,
    pub hub: &'static str,
    pub registration: &'static str,
}

const fn cr(code: &'static str, hub: &'static str, registration: &'static str) -> Carrier {
    Carrier {
        code,
        hub,
        registration,
    }
}

pub static CARRIERS: &[Carrier] = &[
    cr("BA", "LHR", "G-????"),
    cr("AF", "CDG", "F-????"),
    cr("LH", "FRA", "D-A???"),
    cr("KL", "AMS", "PH-???"),
    cr("IB", "MAD", "EC-???"),
    cr("VY", "BCN", "EC-???"),
    cr("AZ", "FCO", "EI-???"),
    cr("LX", "ZRH", "HB-???"),
    cr("SK", "CPH", "OY-???"),
    cr("EI", "DUB", "EI-???"),
    cr("TK", "IST", "TC-???"),
    cr("MS", "CAI", "SU-???"),
    cr("SA", "JNB", "ZS-???"),
    cr("EK", "DXB", "A6-???"),
    cr("QR", "DOH", "A7-???"),
    cr("AI", "DEL", "VT-???"),
    cr("SQ", "SIN", "9V-S??"),
    cr("CX", "HKG", "B-H??"),
    cr("CA", "PEK", "B-####"),
    cr("MU", "PVG", "B-####"),
    cr("KE", "ICN", "HL####"),
    cr("JL", "NRT", "JA###?"),
    cr("QF", "SYD", "VH-???"),
    cr("B6", "JFK", "N###??"),
    cr("UA", "ORD", "N###UA"),
    cr("DL", "ATL", "N###DN"),
    cr("AA", "DFW", "N###AN"),
    cr("AC", "YYZ", "C-????"),
    cr("AS", "SEA", "N###AS"),
    cr("AM", "MEX", "XA-???"),
    cr("LA", "GRU", "PR-???"),
];

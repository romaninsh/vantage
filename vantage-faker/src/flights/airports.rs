//! Built-in airports and great-circle geometry on a spherical Earth.

/// A major airport: IATA code, city and position in degrees.
#[derive(Debug)]
pub struct Airport {
    pub iata: &'static str,
    pub city: &'static str,
    pub lat: f64,
    pub lon: f64,
}

const fn ap(iata: &'static str, city: &'static str, lat: f64, lon: f64) -> Airport {
    Airport {
        iata,
        city,
        lat,
        lon,
    }
}

/// The airports flights are drawn between.
pub static AIRPORTS: &[Airport] = &[
    ap("LHR", "London", 51.4700, -0.4543),
    ap("CDG", "Paris", 49.0097, 2.5479),
    ap("FRA", "Frankfurt", 50.0379, 8.5622),
    ap("AMS", "Amsterdam", 52.3105, 4.7683),
    ap("MAD", "Madrid", 40.4983, -3.5676),
    ap("BCN", "Barcelona", 41.2974, 2.0833),
    ap("FCO", "Rome", 41.8003, 12.2389),
    ap("ZRH", "Zurich", 47.4582, 8.5555),
    ap("CPH", "Copenhagen", 55.6180, 12.6508),
    ap("DUB", "Dublin", 53.4264, -6.2499),
    ap("IST", "Istanbul", 41.2753, 28.7519),
    ap("CAI", "Cairo", 30.1219, 31.4056),
    ap("JNB", "Johannesburg", -26.1367, 28.2411),
    ap("DXB", "Dubai", 25.2532, 55.3657),
    ap("DOH", "Doha", 25.2731, 51.6081),
    ap("DEL", "Delhi", 28.5562, 77.1000),
    ap("BOM", "Mumbai", 19.0896, 72.8656),
    ap("SIN", "Singapore", 1.3644, 103.9915),
    ap("HKG", "Hong Kong", 22.3080, 113.9185),
    ap("PEK", "Beijing", 40.0799, 116.6031),
    ap("PVG", "Shanghai", 31.1443, 121.8083),
    ap("ICN", "Seoul", 37.4602, 126.4407),
    ap("NRT", "Tokyo", 35.7720, 140.3929),
    ap("SYD", "Sydney", -33.9399, 151.1753),
    ap("JFK", "New York", 40.6413, -73.7781),
    ap("ORD", "Chicago", 41.9742, -87.9073),
    ap("ATL", "Atlanta", 33.6407, -84.4277),
    ap("DFW", "Dallas", 32.8998, -97.0403),
    ap("MIA", "Miami", 25.7959, -80.2870),
    ap("YYZ", "Toronto", 43.6777, -79.6248),
    ap("SEA", "Seattle", 47.4502, -122.3088),
    ap("SFO", "San Francisco", 37.6213, -122.3790),
    ap("LAX", "Los Angeles", 33.9416, -118.4085),
    ap("MEX", "Mexico City", 19.4361, -99.0719),
    ap("GRU", "Sao Paulo", -23.4356, -46.4731),
    ap("EZE", "Buenos Aires", -34.8222, -58.5358),
    ap("SCL", "Santiago", -33.3930, -70.7858),
    ap("BOG", "Bogota", 4.7016, -74.1469),
    ap("BOS", "Boston", 42.3656, -71.0096),
    ap("IAD", "Washington", 38.9531, -77.4565),
    ap("YVR", "Vancouver", 49.1967, -123.1815),
    ap("HNL", "Honolulu", 21.3245, -157.9251),
    ap("MEL", "Melbourne", -37.6690, 144.8410),
    ap("PER", "Perth", -31.9385, 115.9672),
    ap("AKL", "Auckland", -37.0082, 174.7850),
    ap("BKK", "Bangkok", 13.6900, 100.7501),
    ap("KUL", "Kuala Lumpur", 2.7456, 101.7072),
    ap("CGK", "Jakarta", -6.1256, 106.6559),
    ap("MNL", "Manila", 14.5086, 121.0194),
    ap("TPE", "Taipei", 25.0797, 121.2342),
    ap("NBO", "Nairobi", -1.3192, 36.9278),
    ap("ADD", "Addis Ababa", 8.9779, 38.7993),
    ap("CPT", "Cape Town", -33.9715, 18.6021),
    ap("LOS", "Lagos", 6.5774, 3.3212),
];

/// Look an airport up by IATA code.
pub fn airport(iata: &str) -> Option<&'static Airport> {
    AIRPORTS.iter().find(|a| a.iata == iata)
}

/// Mean Earth radius in nautical miles.
const EARTH_NM: f64 = 3440.065;

fn rad(a: &Airport) -> (f64, f64) {
    (a.lat.to_radians(), a.lon.to_radians())
}

/// Central angle between two airports, in radians (haversine).
fn central_angle(a: &Airport, b: &Airport) -> f64 {
    let ((la1, lo1), (la2, lo2)) = (rad(a), rad(b));
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * h.sqrt().min(1.0).asin()
}

/// Great-circle distance in nautical miles.
pub fn distance_nm(a: &Airport, b: &Airport) -> f64 {
    central_angle(a, b) * EARTH_NM
}

/// Initial great-circle bearing from `(lat1, lon1)` to `(lat2, lon2)`, all in
/// degrees; result in `[0, 360)`.
pub fn bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (la1, la2) = (lat1.to_radians(), lat2.to_radians());
    let dlon = (lon2 - lon1).to_radians();
    let y = dlon.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// The point a fraction `f` (0..=1) of the way from `a` to `b` along the
/// great circle, as `(lat, lon)` in degrees.
pub fn interpolate(a: &Airport, b: &Airport, f: f64) -> (f64, f64) {
    let d = central_angle(a, b);
    if d < 1e-9 {
        return (a.lat, a.lon);
    }
    let ((la1, lo1), (la2, lo2)) = (rad(a), rad(b));
    let (wa, wb) = (((1.0 - f) * d).sin() / d.sin(), (f * d).sin() / d.sin());
    let x = wa * la1.cos() * lo1.cos() + wb * la2.cos() * lo2.cos();
    let y = wa * la1.cos() * lo1.sin() + wb * la2.cos() * lo2.sin();
    let z = wa * la1.sin() + wb * la2.sin();
    let lat = z.atan2((x * x + y * y).sqrt()).to_degrees();
    let lon = y.atan2(x).to_degrees();
    (lat, lon)
}

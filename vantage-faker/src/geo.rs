//! Great-circle geometry on a spherical Earth. Positions are `(lat, lon)` in
//! degrees; shared by the flight effect and the sim scripts' geo verbs.

/// Mean Earth radius in kilometres.
pub const EARTH_KM: f64 = 6371.0088;
/// Mean Earth radius in nautical miles.
pub const EARTH_NM: f64 = 3440.065;

/// Central angle between two points, in radians (haversine).
pub fn central_angle(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (la1, lo1, la2, lo2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let h = ((la2 - la1) / 2.0).sin().powi(2)
        + la1.cos() * la2.cos() * ((lo2 - lo1) / 2.0).sin().powi(2);
    2.0 * h.sqrt().min(1.0).asin()
}

/// Great-circle distance in kilometres.
pub fn distance_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    central_angle(lat1, lon1, lat2, lon2) * EARTH_KM
}

/// Initial great-circle bearing from the first point to the second, in
/// `[0, 360)` degrees.
pub fn bearing(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let (la1, la2) = (lat1.to_radians(), lat2.to_radians());
    let dlon = (lon2 - lon1).to_radians();
    let y = dlon.sin() * la2.cos();
    let x = la1.cos() * la2.sin() - la1.sin() * la2.cos() * dlon.cos();
    y.atan2(x).to_degrees().rem_euclid(360.0)
}

/// The point a fraction `f` (0..=1) of the way from the first point to the
/// second along the great circle, as `(lat, lon)` in degrees.
pub fn interpolate(lat1: f64, lon1: f64, lat2: f64, lon2: f64, f: f64) -> (f64, f64) {
    let d = central_angle(lat1, lon1, lat2, lon2);
    if d < 1e-9 {
        return (lat1, lon1);
    }
    let (la1, lo1, la2, lo2) = (
        lat1.to_radians(),
        lon1.to_radians(),
        lat2.to_radians(),
        lon2.to_radians(),
    );
    let (wa, wb) = (((1.0 - f) * d).sin() / d.sin(), (f * d).sin() / d.sin());
    let x = wa * la1.cos() * lo1.cos() + wb * la2.cos() * lo2.cos();
    let y = wa * la1.cos() * lo1.sin() + wb * la2.cos() * lo2.sin();
    let z = wa * la1.sin() + wb * la2.sin();
    let lat = z.atan2((x * x + y * y).sqrt()).to_degrees();
    let lon = y.atan2(x).to_degrees();
    (lat, lon)
}

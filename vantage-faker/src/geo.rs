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
///
/// Identical points give the first point. Antipodal points are joined by
/// every meridian-like great circle; this one goes through a point 90° from
/// the first, northward where it can.
pub fn interpolate(lat1: f64, lon1: f64, lat2: f64, lon2: f64, f: f64) -> (f64, f64) {
    let d = central_angle(lat1, lon1, lat2, lon2);
    if d < 1e-9 {
        return (lat1, lon1);
    }
    let a = unit(lat1, lon1);
    let b = unit(lat2, lon2);
    let p = if d.sin() < 1e-9 {
        // Antipodes: rotate from `a` towards a perpendicular unit vector.
        let (s, c) = (f * d).sin_cos();
        let q = perpendicular(a);
        [0, 1, 2].map(|i| c * a[i] + s * q[i])
    } else {
        let (wa, wb) = (((1.0 - f) * d).sin() / d.sin(), (f * d).sin() / d.sin());
        [0, 1, 2].map(|i| wa * a[i] + wb * b[i])
    };
    let lat = p[2].atan2((p[0] * p[0] + p[1] * p[1]).sqrt()).to_degrees();
    let lon = p[1].atan2(p[0]).to_degrees();
    (lat, lon)
}

fn unit(lat: f64, lon: f64) -> [f64; 3] {
    let (la, lo) = (lat.to_radians(), lon.to_radians());
    [la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin()]
}

/// A unit vector at right angles to `a`: the direction of north at `a`, or
/// any equatorial direction at a pole.
fn perpendicular(a: [f64; 3]) -> [f64; 3] {
    let horizontal = (a[0] * a[0] + a[1] * a[1]).sqrt();
    if horizontal < 1e-9 {
        return [1.0, 0.0, 0.0];
    }
    // North at `a`: minus the horizontal direction scaled by sin(lat), plus
    // cos(lat) upward.
    let (sin_lat, cos_lat) = (a[2], horizontal);
    [
        -sin_lat * a[0] / horizontal,
        -sin_lat * a[1] / horizontal,
        cos_lat,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolate_handles_identical_and_antipodal_points() {
        assert_eq!(interpolate(10.0, 20.0, 10.0, 20.0, 0.5), (10.0, 20.0));
        for (lat, lon) in [(0.0, 0.0), (45.0, 10.0), (90.0, 0.0), (-30.0, 170.0)] {
            let (alat, alon) = (-lat, if lon > 0.0 { lon - 180.0 } else { lon + 180.0 });
            for f in [0.0, 0.25, 0.5, 1.0] {
                let (y, x) = interpolate(lat, lon, alat, alon, f);
                assert!(y.is_finite() && x.is_finite(), "{lat},{lon} f={f}");
                let from_start = distance_km(lat, lon, y, x);
                let expected = f * std::f64::consts::PI * EARTH_KM;
                assert!((from_start - expected).abs() < 1.0, "{lat},{lon} f={f}");
            }
        }
        let (y, x) = interpolate(0.0, 0.0, 0.0, 90.0, 0.5);
        assert!(y.abs() < 1e-9 && (x - 45.0).abs() < 1e-9);
    }
}

//! Small tools of the map: the coordinates of a point to copy, distances and areas measured
//! by clicking, and labels from an attribute.

use og_engine::io::Backend;

/// What is being measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Measure {
    Distance,
    Area,
}

/// Points clicked out on the map, in the system the map is drawn in.
#[derive(Clone, Debug, PartialEq)]
pub struct Measuring {
    pub what: Measure,
    pub points: Vec<[f64; 2]>,
}

/// The points in degrees, longitude first, from the system the map is drawn in.
fn to_degrees(points: &[[f64; 2]], crs: Option<u32>, backend: &dyn Backend) -> Option<Vec<[f64; 2]>> {
    let crs = crs?;
    if crs == 4326 {
        return Some(points.to_vec());
    }
    let (mut xs, mut ys): (Vec<f64>, Vec<f64>) = points.iter().map(|p| (p[0], p[1])).unzip();
    backend.transform(crs, 4326, &mut xs, &mut ys).ok()?;
    Some(xs.into_iter().zip(ys).map(|(x, y)| [x, y]).collect())
}

/// A length in metres or kilometres, or in the units of the data when it has no system.
fn length_text(m: f64, metres: bool) -> String {
    if !metres {
        return format!("{} units", og_engine::model::fmt_real((m * 100.0).round() / 100.0));
    }
    if m >= 1000.0 {
        format!("{} km", og_engine::model::fmt_real((m / 10.0).round() / 100.0))
    } else {
        format!("{} m", og_engine::model::fmt_real((m * 10.0).round() / 10.0))
    }
}

fn area_text(m2: f64, metres: bool) -> String {
    if !metres {
        return format!("{} square units", og_engine::model::fmt_real((m2 * 100.0).round() / 100.0));
    }
    if m2 >= 1_000_000.0 {
        format!("{} km²", og_engine::model::fmt_real((m2 / 10_000.0).round() / 100.0))
    } else if m2 >= 10_000.0 {
        format!("{} ha", og_engine::model::fmt_real((m2 / 100.0).round() / 100.0))
    } else {
        format!("{} m²", og_engine::model::fmt_real(m2.round()))
    }
}

/// What has been measured, as a line of text: on the ellipsoid when the map has a
/// coordinate system, so that it is right also in Web Mercator, and else in the plane.
pub fn result(m: &Measuring, crs: Option<u32>, backend: &dyn Backend) -> String {
    use geo::{Area, Distance, Euclidean, Geodesic, Length};
    let pts = &m.points;
    if pts.len() < 2 {
        return match m.what {
            Measure::Distance => "Click the points of the distance on the map".into(),
            Measure::Area => "Click the corners of the area on the map".into(),
        };
    }
    let deg = to_degrees(pts, crs, backend);
    let metres = deg.is_some();
    let line = |p: &[[f64; 2]]| geo::LineString::from(p.iter().map(|c| (c[0], c[1])).collect::<Vec<_>>());
    match m.what {
        Measure::Distance => {
            let total = match &deg {
                Some(d) => Geodesic.length(&line(d)),
                None => Euclidean.length(&line(pts)),
            };
            let last = match &deg {
                Some(d) => {
                    Geodesic.distance(geo::Point::from((d[d.len() - 2][0], d[d.len() - 2][1])), geo::Point::from((d[d.len() - 1][0], d[d.len() - 1][1])))
                }
                None => Euclidean.distance(
                    geo::Point::from((pts[pts.len() - 2][0], pts[pts.len() - 2][1])),
                    geo::Point::from((pts[pts.len() - 1][0], pts[pts.len() - 1][1])),
                ),
            };
            if pts.len() == 2 {
                format!("Distance {}", length_text(total, metres))
            } else {
                format!("Distance {} · the last part {}", length_text(total, metres), length_text(last, metres))
            }
        }
        Measure::Area => {
            if pts.len() < 3 {
                return "Click one more corner".into();
            }
            let ring = |p: &[[f64; 2]]| {
                let mut c: Vec<(f64, f64)> = p.iter().map(|c| (c[0], c[1])).collect();
                c.push(c[0]);
                geo::Polygon::new(geo::LineString::from(c), vec![])
            };
            let (area, perimeter) = match &deg {
                Some(d) => {
                    use geo::orient::{Direction, Orient};
                    use geo::GeodesicArea;
                    // Counter-clockwise, or the area is that of the rest of the Earth.
                    let p = ring(d).orient(Direction::Default);
                    (p.geodesic_area_unsigned(), p.geodesic_perimeter())
                }
                None => {
                    let p = ring(pts);
                    (p.unsigned_area(), Euclidean.length(p.exterior()))
                }
            };
            format!("Area {} · perimeter {}", area_text(area, metres), length_text(perimeter, metres))
        }
    }
}

/// A point as text to copy: in the system of the data, and in degrees as latitude,
/// longitude, as web maps take them. The point is in the system the map is drawn in.
pub fn coordinates(at: [f64; 2], shown: Option<u32>, data: Option<u32>, backend: &dyn Backend) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let round = |v: f64, d: i32| {
        let k = 10f64.powi(d);
        og_engine::model::fmt_real((v * k).round() / k)
    };
    if let Some(data) = data.or(shown) {
        let p = match shown {
            Some(s) if s != data => {
                let (mut xs, mut ys) = (vec![at[0]], vec![at[1]]);
                backend.transform(s, data, &mut xs, &mut ys).ok().map(|_| [xs[0], ys[0]])
            }
            _ => Some(at),
        };
        if let Some(p) = p.filter(|_| data != 4326) {
            let text = format!("{}, {}", round(p[0], 2).replace('\u{a0}', ""), round(p[1], 2).replace('\u{a0}', ""));
            out.push((format!("{} (EPSG:{})", text, data), text));
        }
    }
    if let Some(d) = to_degrees(&[at], shown, backend) {
        let text = format!("{}, {}", round(d[0][1], 6), round(d[0][0], 6));
        out.push((format!("{} (latitude, longitude)", text), text));
    }
    if out.is_empty() {
        let text = format!("{}, {}", round(at[0], 3), round(at[1], 3));
        out.push((text.clone(), text));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distances_and_areas_without_a_system_are_in_the_plane() {
        struct NoBackend;
        let b = og_gdal::GdalBackend::new();
        let _ = NoBackend;
        let m = Measuring { what: Measure::Distance, points: vec![[0.0, 0.0], [3.0, 4.0], [3.0, 10.0]] };
        assert_eq!(result(&m, None, &b), "Distance 11 units · the last part 6 units");
        let a = Measuring { what: Measure::Area, points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]] };
        assert_eq!(result(&a, None, &b), "Area 100 square units · perimeter 40 units");
        // In SWEREF 99 TM: a square of a kilometre, measured on the ellipsoid.
        let sq = Measuring { what: Measure::Area, points: vec![[319000.0, 6400000.0], [319000.0, 6401000.0], [320000.0, 6401000.0], [320000.0, 6400000.0]] };
        let text = result(&sq, Some(3006), &b);
        assert!(text.starts_with("Area 1 km²") || text.starts_with("Area 100 ha") || text.starts_with("Area 99"), "{}", text);
        let d = Measuring { what: Measure::Distance, points: vec![[319000.0, 6400000.0], [320000.0, 6400000.0]] };
        assert_eq!(result(&d, Some(3006), &b), "Distance 1 km");
        let c = coordinates([319758.4, 6400326.7], Some(3006), Some(3006), &b);
        assert_eq!(c[0].1, "319758.4, 6400326.7");
        assert!(c[1].0.ends_with("(latitude, longitude)") && c[1].1.starts_with("57.7"), "{:?}", c);
    }
}

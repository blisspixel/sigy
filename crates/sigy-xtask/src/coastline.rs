//! Converts the pinned Natural Earth 1:110m coastline `GeoJSON` into the compact text asset
//! embedded by the terminal globe. Natural Earth data is in the public domain.

use std::{fmt::Write as _, fs, path::Path};

/// SHA-256 of `geojson/ne_110m_coastline.geojson` at natural-earth-vector tag v5.1.2
/// (commit f1890d9f152c896d250a77557a5751a93d494776).
pub const SOURCE_SHA256: &str = "851f581ff5ffb844deed8ae1a9ce22e3c4bb3d74fa342cadb5d8e39b41ae7c3c";

/// One line per `LineString`: `lon,lat` pairs rounded to hundredths of a degree, separated
/// by single spaces. Consecutive duplicate points after rounding are dropped.
pub fn convert(source: &Path, output: &Path) -> Result<(), String> {
    let digest = crate::vendor::hash_file(source)?;
    if digest != SOURCE_SHA256 {
        return Err(format!("unexpected coastline source sha256 {digest}"));
    }
    let bytes = fs::read(source).map_err(|error| error.to_string())?;
    let text = render(&bytes)?;
    fs::write(output, text).map_err(|error| error.to_string())
}

fn render(bytes: &[u8]) -> Result<String, String> {
    let document: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|error| error.to_string())?;
    let features = document["features"]
        .as_array()
        .ok_or("coastline has no features")?;
    let mut out = String::new();
    for feature in features {
        let geometry = &feature["geometry"];
        if geometry["type"] != "LineString" {
            return Err("coastline feature is not a LineString".into());
        }
        let points = geometry["coordinates"]
            .as_array()
            .ok_or("coastline LineString has no coordinates")?;
        let mut line: Vec<String> = Vec::with_capacity(points.len());
        for point in points {
            let lon = point[0].as_f64().ok_or("coastline longitude")?;
            let lat = point[1].as_f64().ok_or("coastline latitude")?;
            if !(-180.0..=180.0).contains(&lon) || !(-90.0..=90.0).contains(&lat) {
                return Err("coastline point out of range".into());
            }
            let rounded = format!("{},{}", hundredths(lon), hundredths(lat));
            if line.last() != Some(&rounded) {
                line.push(rounded);
            }
        }
        if line.len() < 2 {
            continue;
        }
        let parts = line;
        writeln!(out, "{}", parts.join(" ")).map_err(|error| error.to_string())?;
    }
    Ok(out)
}

/// Hundredths of a degree as decimal text, with no negative zero.
fn hundredths(value: f64) -> String {
    let text = format!("{value:.2}");
    if text == "-0.00" {
        "0.00".to_owned()
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounds_drops_repeats_and_keeps_signs() -> Result<(), String> {
        let json = br#"{"features":[{"geometry":{"type":"LineString","coordinates":[[-0.004,1.0],[0.004,1.0],[179.999,-89.5]]}}]}"#;
        assert_eq!(render(json)?, "0.00,1.00 180.00,-89.50\n");
        let negative = br#"{"features":[{"geometry":{"type":"LineString","coordinates":[[-163.712896,-78.595667],[-0.5,0.25]]}}]}"#;
        assert_eq!(render(negative)?, "-163.71,-78.60 -0.50,0.25\n");
        assert!(
            render(br#"{"features":[{"geometry":{"type":"Point","coordinates":[0,0]}}]}"#).is_err()
        );
        assert!(render(br#"{"features":[{"geometry":{"type":"LineString","coordinates":[[200,0],[0,0]]}}]}"#).is_err());
        Ok(())
    }
}

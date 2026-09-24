//! Terminal globe and day/night map drawn from offline geometry.
//!
//! Coastlines are the vendored Natural Earth 1:110m coastline (public domain; see
//! `assets/README.md`). Night is geometric (solar zenith above 90 degrees) at one explicit
//! UTC instant. Station markers use directory coordinates, which describe where a directory
//! says a stream is located, not where its speakers or subjects are.

use std::sync::OnceLock;

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Color,
    symbols::Marker,
    text::Line,
    widgets::{
        Widget,
        canvas::{Canvas, Circle, Context, Line as Segment, Points},
    },
};
use sigy_service::domain::geo::{
    Daylight, LonLat, MapSegment, Orthographic, SolarPosition, UtcInstant, equirectangular,
    equirectangular_segment,
};

use super::state::{Coordinates, Explorer};

const COASTLINE: &str = include_str!("../../assets/ne_110m_coastline.txt");

/// Parsed coastline polylines. Malformed points are skipped rather than drawn.
fn coastline() -> &'static [Vec<LonLat>] {
    static LINES: OnceLock<Vec<Vec<LonLat>>> = OnceLock::new();
    LINES.get_or_init(|| {
        COASTLINE
            .lines()
            .map(|line| {
                line.split(' ')
                    .filter_map(|pair| {
                        let (lon, lat) = pair.split_once(',')?;
                        LonLat::new(lon.parse().ok()?, lat.parse().ok()?).ok()
                    })
                    .collect()
            })
            .filter(|line: &Vec<LonLat>| line.len() >= 2)
            .collect()
    })
}

#[derive(Debug, Clone, Copy)]
struct Palette {
    coast: Color,
    night: Color,
    rim: Color,
    station: Color,
    selected: Color,
}

impl Palette {
    const fn new(color: bool) -> Self {
        if color {
            Self {
                coast: Color::Green,
                night: Color::DarkGray,
                rim: Color::Gray,
                station: Color::Yellow,
                selected: Color::Cyan,
            }
        } else {
            Self {
                coast: Color::Reset,
                night: Color::Reset,
                rim: Color::Reset,
                station: Color::Reset,
                selected: Color::Reset,
            }
        }
    }
}

/// Counts shown in the header so a partial map is never mistaken for the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapCounts {
    pub mapped: usize,
    pub total: usize,
}

#[must_use]
pub fn counts(model: &Explorer) -> MapCounts {
    MapCounts {
        mapped: model
            .rows()
            .iter()
            .filter(|row| row.coordinates.is_some())
            .count(),
        total: model.rows().len(),
    }
}

/// `YYYY-MM-DD HH:MM UTC` for a Unix time in milliseconds, without a time zone database.
#[must_use]
pub fn utc_label(unix_ms: i64) -> String {
    let seconds = unix_ms.div_euclid(1000);
    let days = seconds.div_euclid(86_400);
    let of_day = seconds.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (proleptic Gregorian).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        of_day / 3600,
        of_day % 3600 / 60
    )
}

fn degrees(value: f64, positive: char, negative: char) -> String {
    let hemisphere = if value < 0.0 { negative } else { positive };
    format!("{:.0}{hemisphere}", value.abs())
}

#[must_use]
pub fn header(model: &Explorer) -> Line<'static> {
    let (lon, lat) = model.globe_center();
    let counts = counts(model);
    let view = if model.flat_map() {
        "Flat map".to_owned()
    } else {
        format!(
            "Globe centered {} {}",
            degrees(lon, 'E', 'W'),
            degrees(lat, 'N', 'S')
        )
    };
    Line::from(format!(
        "{view} | geometric night at {} | {} of {} stations on this page have directory coordinates | h/l/j/k rotate, m map, c center",
        utc_label(model.now_ms()),
        counts.mapped,
        counts.total
    ))
}

/// The drawing area. Bounds keep one unit square in data space square on screen,
/// assuming a terminal cell twice as tall as it is wide, as braille dots are 2 by 4.
pub struct GlobeWidget<'a> {
    model: &'a Explorer,
    palette: Palette,
}

impl<'a> GlobeWidget<'a> {
    #[must_use]
    pub const fn new(model: &'a Explorer, color: bool) -> Self {
        Self {
            model,
            palette: Palette::new(color),
        }
    }
}

fn aspect(area: Rect) -> (f64, f64) {
    let dots_wide = f64::from(area.width) * 2.0;
    let dots_high = f64::from(area.height) * 4.0;
    if dots_wide >= dots_high {
        (dots_wide / dots_high, 1.0)
    } else {
        (1.0, dots_high / dots_wide)
    }
}

impl Widget for GlobeWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.width < 4 || area.height < 2 {
            return;
        }
        let instant = UtcInstant::from_unix_seconds(self.model.now_ms().div_euclid(1000)).ok();
        let sun = instant.map(SolarPosition::at);
        let (ax, ay) = aspect(area);
        let flat = self.model.flat_map();
        // The flat map uses x = lon / 180 and y = lat / 90 in [-1, 1]. Its shape on screen
        // must stay two wide by one high, so one x unit spans twice the length of one y unit.
        let (x_bounds, y_bounds) = if flat {
            let kx = (ax / (2.0 * ay)).max(1.0);
            let ky = kx * 2.0 * ay / ax;
            ([-kx, kx], [-ky, ky])
        } else {
            ([-ax, ax], [-ay, ay])
        };
        let (lon, lat) = self.model.globe_center();
        let projection = Orthographic::new(lon, lat).ok();
        let steps = (u32::from(area.width), u32::from(area.height) * 2);
        let palette = self.palette;
        let model = self.model;
        Canvas::default()
            .marker(Marker::Braille)
            .x_bounds(x_bounds)
            .y_bounds(y_bounds)
            .paint(move |ctx| {
                if let Some(sun) = sun {
                    let night = night_points(flat, projection, &sun, x_bounds, y_bounds, steps);
                    ctx.draw(&Points {
                        coords: &night,
                        color: palette.night,
                    });
                }
                ctx.layer();
                if flat {
                    draw_flat_coast(ctx, palette.coast);
                } else if let Some(projection) = projection {
                    ctx.draw(&Circle {
                        x: 0.0,
                        y: 0.0,
                        radius: 1.0,
                        color: palette.rim,
                    });
                    draw_globe_coast(ctx, &projection, palette.coast);
                }
                ctx.layer();
                draw_stations(ctx, model, flat, projection, palette);
            })
            .render(area, buffer);
    }
}

/// Sparse sample points in night, one per cell column and every other dot row.
fn night_points(
    flat: bool,
    projection: Option<Orthographic>,
    sun: &SolarPosition,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    (columns, rows): (u32, u32),
) -> Vec<(f64, f64)> {
    let mut points = Vec::new();
    let width = x_bounds[1] - x_bounds[0];
    let height = y_bounds[1] - y_bounds[0];
    for column in 0..columns {
        for row in 0..rows {
            let x = x_bounds[0] + (f64::from(column) + 0.5) * width / f64::from(columns);
            let y = y_bounds[0] + (f64::from(row) + 0.5) * height / f64::from(rows);
            let place = if flat {
                LonLat::new(x * 180.0, y * 90.0).ok()
            } else {
                projection.and_then(|projection| {
                    projection.inverse(sigy_service::domain::geo::MapPoint { x, y })
                })
            };
            if place.is_some_and(|place| sun.daylight(place) == Daylight::Night) {
                points.push((x, y));
            }
        }
    }
    points
}

fn draw_globe_coast(ctx: &mut Context<'_>, projection: &Orthographic, color: Color) {
    for line in coastline() {
        for pair in line.windows(2) {
            let Ok(Some((start, end))) = projection.clip_arc(pair[0], pair[1]) else {
                continue;
            };
            if let (Some(a), Some(b)) = (projection.project(start), projection.project(end)) {
                ctx.draw(&Segment {
                    x1: a.x,
                    y1: a.y,
                    x2: b.x,
                    y2: b.y,
                    color,
                });
            }
        }
    }
}

fn draw_flat_coast(ctx: &mut Context<'_>, color: Color) {
    for line in coastline() {
        for pair in line.windows(2) {
            let pieces = match equirectangular_segment(pair[0], pair[1]) {
                MapSegment::Whole(whole) => vec![whole],
                MapSegment::Split(first, second) => vec![first, second],
            };
            for [a, b] in pieces {
                ctx.draw(&Segment {
                    x1: a.x,
                    y1: a.y,
                    x2: b.x,
                    y2: b.y,
                    color,
                });
            }
        }
    }
}

fn place(coordinates: Coordinates) -> Option<LonLat> {
    LonLat::new(coordinates.longitude, coordinates.latitude).ok()
}

fn draw_stations(
    ctx: &mut Context<'_>,
    model: &Explorer,
    flat: bool,
    projection: Option<Orthographic>,
    palette: Palette,
) {
    let selected = model.selected().map(|row| row.id.clone());
    for row in model.rows() {
        let Some(point) = row.coordinates.and_then(place) else {
            continue;
        };
        let screen = if flat {
            Some(equirectangular(point))
        } else {
            projection.and_then(|projection| projection.project(point))
        };
        let Some(screen) = screen else {
            continue;
        };
        let chosen = selected.as_deref() == Some(row.id.as_str());
        let (symbol, color) = if chosen {
            ("@", palette.selected)
        } else {
            ("*", palette.station)
        };
        ctx.print(
            screen.x,
            screen.y,
            ratatui::text::Span::styled(symbol, ratatui::style::Style::default().fg(color)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_labels_are_exact_civil_times() {
        assert_eq!(utc_label(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc_label(1_709_164_800_000), "2024-02-29 00:00 UTC");
        assert_eq!(utc_label(1_790_279_058_731), "2026-09-24 19:44 UTC");
        assert_eq!(utc_label(-86_400_000), "1969-12-31 00:00 UTC");
    }

    #[test]
    fn the_vendored_coastline_parses_completely() {
        let lines = coastline();
        assert_eq!(lines.len(), 134);
        assert!(lines.iter().all(|line| line.len() >= 2));
        let points: usize = lines.iter().map(Vec::len).sum();
        assert!(points > 4_000, "{points}");
    }
}

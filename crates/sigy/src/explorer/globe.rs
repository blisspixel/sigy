//! Terminal globe and day/night map drawn from offline geometry.
//!
//! Coastlines are the vendored Natural Earth 1:110m coastline (public domain; see
//! `assets/README.md`). Night is geometric (solar zenith above 90 degrees) at one explicit
//! UTC instant. Station markers use directory coordinates, which describe where a directory
//! says a stream is located, not where its speakers or subjects are.

use std::{collections::BTreeMap, sync::OnceLock};

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

use super::state::{Coordinates, Explorer, StationRow};

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
            .filter(|row| row.coordinates.and_then(place).is_some())
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
pub fn header_lines(model: &Explorer) -> Vec<Line<'static>> {
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
    vec![
        Line::from(format!(
            "{view} | geometric night at {}",
            utc_label(model.now_ms())
        )),
        Line::from(format!(
            "{} of {} stations on this page have directory coordinates",
            counts.mapped, counts.total,
        )),
        Line::from("h/l/j/k rotate, m map, c center | 2-9 count, + 10 or more in one cell"),
    ]
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
        let view = MapView {
            flat,
            projection,
            x_bounds,
            y_bounds,
            columns: area.width,
            rows: area.height,
        };
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
                draw_stations(ctx, model, &view, palette);
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

/// Uses the same terminal-cell coordinates as canvas labels, rather than braille dots.
/// Grouping is presentation only: every station remains in the filtered result list.
#[derive(Debug, Clone, Copy)]
struct MapView {
    flat: bool,
    projection: Option<Orthographic>,
    x_bounds: [f64; 2],
    y_bounds: [f64; 2],
    columns: u16,
    rows: u16,
}

impl MapView {
    fn point(self, row: &StationRow) -> Option<sigy_service::domain::geo::MapPoint> {
        let point = row.coordinates.and_then(place)?;
        if self.flat {
            Some(equirectangular(point))
        } else {
            self.projection
                .and_then(|projection| projection.project(point))
        }
    }

    fn cell(self, point: sigy_service::domain::geo::MapPoint) -> Option<(u16, u16)> {
        if point.x < self.x_bounds[0]
            || point.x > self.x_bounds[1]
            || point.y < self.y_bounds[0]
            || point.y > self.y_bounds[1]
        {
            return None;
        }
        // Match Canvas labels' multiplication-before-division order exactly.
        // Reordering these operations can round a boundary into an adjacent cell.
        let x = (point.x - self.x_bounds[0]) * f64::from(self.columns.saturating_sub(1))
            / (self.x_bounds[1] - self.x_bounds[0]);
        let y = (self.y_bounds[1] - point.y) * f64::from(self.rows.saturating_sub(1))
            / (self.y_bounds[1] - self.y_bounds[0]);
        Some((cell_index(x, self.columns)?, cell_index(y, self.rows)?))
    }
}

/// Floor a bounded cell coordinate without an unchecked float-to-integer cast.
fn cell_index(position: f64, cells: u16) -> Option<u16> {
    if cells == 0 || !position.is_finite() || position < 0.0 || position >= f64::from(cells) {
        return None;
    }
    let (mut low, mut high) = (0, cells - 1);
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        if f64::from(middle) <= position {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    Some(low)
}

#[derive(Debug, Clone, Copy)]
struct StationCluster {
    point: sigy_service::domain::geo::MapPoint,
    count: usize,
    selected: bool,
}

impl StationCluster {
    fn symbol(self) -> String {
        if self.selected {
            "@".into()
        } else if self.count == 1 {
            "*".into()
        } else if self.count < 10 {
            self.count.to_string()
        } else {
            "+".into()
        }
    }
}

fn station_clusters(
    rows: &[StationRow],
    selected: Option<&str>,
    view: MapView,
) -> Vec<StationCluster> {
    let mut cells = BTreeMap::<(u16, u16), StationCluster>::new();
    for row in rows {
        let Some(point) = view.point(row) else {
            continue;
        };
        let Some(cell) = view.cell(point) else {
            continue;
        };
        let chosen = selected == Some(row.id.as_str());
        let cluster = cells.entry(cell).or_insert(StationCluster {
            point,
            count: 0,
            selected: false,
        });
        cluster.count += 1;
        if chosen {
            cluster.selected = true;
            cluster.point = point;
        }
    }
    cells.into_values().collect()
}

fn draw_stations(ctx: &mut Context<'_>, model: &Explorer, view: &MapView, palette: Palette) {
    let selected = model.selected().map(|row| row.id.as_str());
    for cluster in station_clusters(model.rows(), selected, *view) {
        let color = if cluster.selected {
            palette.selected
        } else {
            palette.station
        };
        ctx.print(
            cluster.point.x,
            cluster.point.y,
            ratatui::text::Span::styled(
                cluster.symbol(),
                ratatui::style::Style::default().fg(color),
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::super::state::{DirectoryView, Effect, Health, Key, Link, Modes};
    use super::*;

    fn row(id: &str, longitude: f64, latitude: f64) -> StationRow {
        StationRow {
            id: id.into(),
            name: id.into(),
            favorite: true,
            directory_health: Health::Unknown,
            directory_languages: "unknown".into(),
            observed_ms: 0,
            hls: false,
            coordinates: Some(Coordinates {
                latitude,
                longitude,
            }),
        }
    }

    fn flat_view() -> MapView {
        MapView {
            flat: true,
            projection: None,
            x_bounds: [-1.0, 1.0],
            y_bounds: [-1.0, 1.0],
            columns: 80,
            rows: 24,
        }
    }

    #[test]
    fn crowded_cells_count_stations_and_keep_selection_in_any_order() {
        let rows = vec![row("selected", 0.0, 0.0), row("nearby", 0.01, 0.01)];
        for ordered in [rows.clone(), rows.into_iter().rev().collect()] {
            let clusters = station_clusters(&ordered, Some("selected"), flat_view());
            assert_eq!(clusters.len(), 1);
            assert_eq!(clusters[0].count, 2);
            assert_eq!(clusters[0].symbol(), "@");
            assert!(clusters[0].point.x.abs() < f64::EPSILON);
            let unselected = station_clusters(&ordered, None, flat_view());
            assert_eq!(unselected[0].symbol(), "2");
        }
        let many: Vec<_> = (0..16).map(|id| row(&id.to_string(), 0.0, 0.0)).collect();
        let clusters = station_clusters(&many, None, flat_view());
        assert_eq!(clusters[0].count, 16);
        assert_eq!(clusters[0].symbol(), "+");
    }

    #[test]
    fn unmapped_invalid_and_far_side_rows_never_create_markers() {
        let mut missing = row("missing", 0.0, 0.0);
        missing.coordinates = None;
        let rows = vec![
            row("near", 0.0, 0.0),
            row("far", 180.0, 0.0),
            row("invalid", 0.0, 91.0),
            row("nan", f64::NAN, 0.0),
            missing,
        ];
        let mut view = flat_view();
        view.flat = false;
        view.projection = Orthographic::new(0.0, 0.0).ok();
        let clusters = station_clusters(&rows, Some("far"), view);
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 1);
        assert_eq!(clusters[0].symbol(), "*");
        assert_eq!(station_clusters(&rows, None, flat_view()).len(), 2);
    }

    #[test]
    fn poles_and_antimeridian_keep_their_correct_edge_cells() {
        let rows = vec![row("west", -180.0, 90.0), row("east", 179.999, -90.0)];
        let view = flat_view();
        let clusters = station_clusters(&rows, None, view);
        assert_eq!(clusters.len(), 2);
        assert_eq!(view.cell(clusters[0].point), Some((0, 0)));
        assert_eq!(view.cell(clusters[1].point), Some((78, 23)));
        let wrapped = station_clusters(&[row("wrapped", 180.0, 90.0)], None, view);
        assert_eq!(view.cell(wrapped[0].point), Some((0, 0)));
        assert_eq!(cell_index(0.0, 1), Some(0));
        assert_eq!(cell_index(39.5, 80), Some(39));
        assert_eq!(cell_index(79.000_000_000_000_01, 80), Some(79));
        for position in [-0.01, 80.0, f64::NAN, f64::INFINITY] {
            assert_eq!(cell_index(position, 80), None);
        }
        assert_eq!(cell_index(0.0, 0), None);
    }

    #[test]
    fn cell_boundary_rounding_cannot_overwrite_the_selected_canvas_marker()
    -> Result<(), Box<dyn std::error::Error>> {
        let boundary = -136.708_860_759_493_75;
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: true,
            },
            0,
        );
        model.note_link(Link::LocalCatalog);
        let Effect::Search(query) = model.handle(Key::Char('v')) else {
            return Err("filter did not submit a query".into());
        };
        assert!(model.apply_search(
            query.generation,
            vec![
                row("selected", boundary - 1e-13, 0.0),
                row("boundary", boundary, 0.0),
            ],
            DirectoryView {
                cached_stations: 2,
                maximum_stations: 10_000,
                favorite_stations: 2,
                refresh: None,
            },
            None
        ));
        assert_eq!(model.handle(Key::Char('7')), Effect::None);
        assert_eq!(model.handle(Key::Char('m')), Effect::None);
        let area = Rect::new(0, 0, 80, 3);
        let mut buffer = Buffer::empty(area);
        GlobeWidget::new(&model, false).render(area, &mut buffer);
        assert_eq!(buffer[(34, 1)].symbol(), "@");
        assert_eq!(
            buffer
                .content
                .iter()
                .filter(|cell| cell.symbol() == "@")
                .count(),
            1
        );
        assert!(!buffer.content.iter().any(|cell| cell.symbol() == "*"));
        let view = MapView {
            x_bounds: [-20.0 / 3.0, 20.0 / 3.0],
            y_bounds: [-1.0, 1.0],
            columns: 80,
            rows: 3,
            ..flat_view()
        };
        let adjacent = station_clusters(
            &[
                row("left", boundary, 0.0),
                row("right", boundary + 1e-13, 0.0),
            ],
            None,
            view,
        );
        assert_eq!(
            adjacent.len(),
            2,
            "stations in adjacent cells must not be merged"
        );
        Ok(())
    }

    #[test]
    fn the_map_uses_the_same_filtered_page_and_rejects_stale_search_results() {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: true,
            },
            0,
        );
        model.note_link(Link::LocalCatalog);
        let Effect::Search(query) = model.handle(Key::Char('v')) else {
            panic!("favorites filter must submit one catalog query");
        };
        assert!(query.favorites_only);
        let directory = DirectoryView {
            cached_stations: 100,
            maximum_stations: 10_000,
            favorite_stations: 3,
            refresh: None,
        };
        let mut missing = row("unmapped", 0.0, 0.0);
        missing.coordinates = None;
        assert!(model.apply_search(
            query.generation,
            vec![row("first", 0.0, 0.0), row("second", 0.01, 0.01), missing,],
            directory.clone(),
            None
        ));
        let clusters = station_clusters(
            model.rows(),
            model.selected().map(|r| r.id.as_str()),
            flat_view(),
        );
        assert_eq!(
            counts(&model),
            MapCounts {
                mapped: 2,
                total: 3
            }
        );
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].count, 2);
        assert_eq!(clusters[0].symbol(), "@");
        assert!(!model.apply_search(
            query.generation,
            vec![row("stale", 90.0, 0.0)],
            directory,
            None
        ));
        assert_eq!(model.rows().len(), 3);
        assert_eq!(model.selected().map(|r| r.id.as_str()), Some("first"));
    }

    #[test]
    fn rendered_clusters_keep_selection_and_fit_each_terminal_size()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut model = Explorer::new(
            Modes {
                reduced_motion: true,
                linear: false,
                monochrome: true,
            },
            1_790_279_058_731,
        );
        model.note_link(Link::LocalCatalog);
        let Effect::Search(query) = model.handle(Key::Char('v')) else {
            return Err("filter did not submit a query".into());
        };
        assert!(model.apply_search(
            query.generation,
            vec![
                row("selected", 0.0, 0.0),
                row("same cell", 0.01, 0.01),
                row("other", -90.0, 40.0),
                row("other nearby", -89.99, 40.01),
            ],
            DirectoryView {
                cached_stations: 4,
                maximum_stations: 10_000,
                favorite_stations: 4,
                refresh: None,
            },
            None
        ));
        assert_eq!(model.handle(Key::Char('7')), Effect::None);
        assert_eq!(model.handle(Key::Char('m')), Effect::None);
        let review = std::env::var_os("SIGY_REVIEW_GLOBE").is_some();
        for (width, height) in [(80, 24), (160, 48), (40, 10)] {
            let area = Rect::new(0, 0, width, height);
            let mut buffer = Buffer::empty(area);
            GlobeWidget::new(&model, false).render(area, &mut buffer);
            let text = buffer
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect::<String>();
            assert_eq!(
                text.matches('@').count(),
                1,
                "selected cluster at {width}x{height}"
            );
            assert_eq!(
                text.matches('2').count(),
                1,
                "unselected cluster at {width}x{height}"
            );
            assert!(
                !text.contains('*'),
                "cluster members were drawn individually"
            );
            assert!(buffer.content.iter().all(|cell| cell.fg == Color::Reset));
            if review {
                write_review_frame(&buffer)?;
                write_render_measurement(&model, area)?;
            }
        }
        Ok(())
    }

    fn write_render_measurement(
        model: &Explorer,
        area: Rect,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut samples = Vec::new();
        for _ in 0..20 {
            let mut buffer = Buffer::empty(area);
            let start = std::time::Instant::now();
            GlobeWidget::new(model, false).render(area, &mut buffer);
            samples.push(start.elapsed().as_micros());
            assert!(buffer.content.iter().any(|cell| cell.symbol() == "@"));
        }
        let mut ordered = samples.clone();
        ordered.sort_unstable();
        let report = serde_json::json!({
            "width": area.width, "height": area.height, "samples_us": samples,
            "p95_us": ordered.get(18), "maximum_us": ordered.last(),
            "scope": "warm monochrome flat-map buffer render, allocation and terminal I/O excluded",
        });
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.agents/globe-review");
        std::fs::write(
            directory.join(format!("render-{}x{}.json", area.width, area.height)),
            serde_json::to_vec(&report)?,
        )?;
        Ok(())
    }

    fn write_review_frame(buffer: &Buffer) -> Result<(), Box<dyn std::error::Error>> {
        let directory =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.agents/globe-review");
        std::fs::create_dir_all(&directory)?;
        let rows = buffer
            .content
            .chunks(usize::from(buffer.area.width))
            .map(|row| {
                row.iter()
                    .map(ratatui::buffer::Cell::symbol)
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        let cells = buffer
            .content
            .iter()
            .enumerate()
            .map(|(index, cell)| {
                serde_json::json!({
                    "x": index % usize::from(buffer.area.width),
                    "y": index / usize::from(buffer.area.width),
                    "text": cell.symbol(), "fg": format!("{:?}", cell.fg),
                    "bg": format!("{:?}", cell.bg), "modifiers": format!("{:?}", cell.modifier),
                })
            })
            .collect::<Vec<_>>();
        let name = format!("flat-{}x{}", buffer.area.width, buffer.area.height);
        std::fs::write(directory.join(format!("{name}.txt")), rows.join("\n"))?;
        let report = serde_json::json!({
            "cols": buffer.area.width, "rows": buffer.area.height, "cells": cells,
            "method": "Ratatui buffer; synthetic directory coordinates; monochrome",
        });
        std::fs::write(
            directory.join(format!("{name}.json")),
            serde_json::to_vec(&report)?,
        )?;
        Ok(())
    }

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

# 0038: Compute globe geometry from one explicit instant

Date: 2026-09-24. Status: geometry implemented in `sigy-core::geo` and covered by unit tests. No rendering, coastline data, clustering, or map/list agreement exists yet; [operation 36](../../ROADMAP.md#build-order) stays open.

## Decision

`sigy-core::geo` holds the geometry for the planned globe and day/night map. It adds no dependency, and `sigy-core` stays dependency-free. Angles are `f64` degrees on a sphere. Longitude wraps to `[-180, 180)`; a point latitude outside `[-90, 90]` or a non-finite value is refused.

The globe is the orthographic projection of Snyder, *Map Projections: A Working Manual*, USGS Professional Paper 1395 (1987). A point is visible when `cos c >= 0`, with a `1e-12` rounding allowance at the horizon. A center latitude is clamped and its longitude wrapped, so rotation input needs no separate range check. The inverse returns the center at the disc origin and nothing outside the unit disc. A great-circle arc shorter than 180 degrees is clipped at the horizon by 60 bisection steps on the one sign change of `cos c`; antipodal endpoints are refused. The flat map is equirectangular. A longitude step above 180 degrees is split at the antimeridian with a linearly interpolated crossing latitude.

Solar position uses the NOAA Solar Calculator formulas after Meeus, *Astronomical Algorithms*: apparent declination, equation of time, and the subsolar point. The instant is always a parameter, given as Unix seconds or a Julian day; nothing reads the system clock. UT is used where the formulas expect TT. Day and night are geometric: night when the solar zenith exceeds 90 degrees. Civil, nautical, and astronomical twilight at 96, 102, and 108 degrees are a separate classification. Refraction, the solar radius, elevation, and parallax are not applied, so this terminator is not a sunrise table, a weather view, or a reception forecast.

## Evidence

Independent fixtures on this host: Meeus example 25.a gives declination -7.78507 against -7.78507 degrees, and example 28.a gives an equation of time of 13.7109 against 13.71 minutes, both at 1992-10-13 0h taken as UT. The NREL SPA example (2003-10-17 19:30:30 UTC, latitude 39.742476, longitude -105.1786) gives declination -9.3158 against -9.31434 degrees and zenith 50.1279 against 50.12795 degrees. Tests assert the looser bounds of 0.01 degree, 0.5 minute, and 0.05 degree. Property tests cover the subsolar point and its antipode, solstice poles, twilight bands, projection center and horizon, the far side, inverse round trips within `1e-9` degree, polar centers, center wrapping, horizon clipping, and the antimeridian split.

NOAA states about one minute of sunrise and sunset accuracy within 72 degrees of the equator and the most reliable results for 1800 to 2100. Accuracy outside those limits is unmeasured here.

## Consequences

The planned coastline source is Natural Earth 1:110m, which is public domain, vendored with its provenance and checked by hash when it is added. Terminal cell aspect correction, rendering tiers, marker clustering, and measurement with capture running belong to later increments of operation 36.

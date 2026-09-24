# Bundled assets

## Natural Earth coastline

`ne_110m_coastline.txt` is derived from `geojson/ne_110m_coastline.geojson` in [natural-earth-vector](https://github.com/nvkelso/natural-earth-vector) at tag v5.1.2 (commit `f1890d9f152c896d250a77557a5751a93d494776`). The source file has SHA-256 `851f581ff5ffb844deed8ae1a9ce22e3c4bb3d74fa342cadb5d8e39b41ae7c3c`. [Natural Earth](https://www.naturalearthdata.com/about/terms-of-use/) data is in the public domain; attribution is appreciated and given here.

`cargo run -p sigy-xtask -- coastline PATH_TO_SOURCE_GEOJSON` regenerates the file. It refuses a source with a different hash, writes one line per coastline with `lon,lat` pairs rounded to hundredths of a degree, and drops repeated points after rounding. The result has 134 lines and SHA-256 `00d0e8ce194671fd5ee58c583619a9294360d15af0c8f2cb5b2684ac4decaf6b`.

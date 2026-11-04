# Grids of the geoid

PROJ needs a grid of the geoid to change heights between heights above the ellipsoid, as
GNSS gives them, and heights above sea level. These come with Ographis, from
[PROJ-data](https://github.com/OSGeo/PROJ-data); others are fetched from cdn.proj.org when
*Change height system* is allowed to.

| File | Heights | Source and licence |
| --- | --- | --- |
| `se_lantmateriet_SWEN17_RH2000.tif` | RH 2000, Sweden (EPSG:5613), to SWEREF 99 | Lantmäteriet, SWEN17_RH2000. CC0 |
| `no_kv_HREF2018B_NN2000_EUREF89.tif` | NN2000, Norway (EPSG:5941), to EUREF89 | Kartverket, HREF2018B. CC BY 4.0 |
| `us_nga_egm96_15.tif` | EGM96 (EPSG:5773), the world, to WGS 84 | NGA. Public domain |

`build.cmd -Package` copies them to the folder `proj-grids` beside the program, where the
program looks for them, as it does here when it runs from the source.

# For AI assistants and coding agents

This file is for an assistant that is asked to **write an Ographis model**, or to change the
code of Ographis. People are welcome to read it too.

## Writing a model

A model is a JSON file, `name.ographis`, with nodes and the connections between their ports.
The program draws it on a canvas, and `og run model.ographis` runs it from the command line.

1. **Look the nodes up, do not guess them.** Every node type, its ports and its settings,
   with their keys, kinds, choices and defaults, are in [`docs/nodes.json`](docs/nodes.json),
   or from the program with `og nodes --json`. Type names, port names and setting keys must
   be exactly as written there.
2. **Start from an example** that does something like the task. The table below lists some.
3. **Check the model** with `og check model.ographis` before it is handed over. It says what
   is wrong and what would be right, for example:
   *The node "stops" (JsonExploder) has no output port "Output"; it has Exploded and Other.*
   Correct it and check again until it says OK. The exit code is 0 when the model is fine.
4. Editors that check JSON can use [`docs/model.schema.json`](docs/model.schema.json), or
   `og schema`.

### The shape of a model

```json
{
  "format": "ographis/1",
  "name": "Large towns",
  "variables": [{"name": "min", "value": "10000"}],
  "nodes": [
    {"id": "towns", "type": "Reader", "name": "Towns", "x": 20, "y": 20,
     "params": {"path": "towns.csv", "x": "lon", "y": "lat", "crs": "EPSG:4326"}},
    {"id": "big", "type": "Tester", "name": "Large towns", "x": 280, "y": 20,
     "params": {"test": "population >= $(min)"}},
    {"id": "out", "type": "Writer", "name": "Result", "x": 540, "y": 20,
     "params": {"path": "out/large_towns.gpkg"}}
  ],
  "edges": [
    {"from": ["towns", "Output"], "to": ["big", "Input"]},
    {"from": ["big", "Passed"], "to": ["out", "Input"]}
  ]
}
```

### Rules worth knowing

- **Ports differ by tool.** *Input* (`Reader`) gives `Output`; *Test* (`Tester`) gives
  `Passed` and `Failed`; *Explode JSON list* (`JsonExploder`) gives `Exploded` and `Other`;
  *Reproject* gives `Reprojected`; *Create points from x and y* gives `Points` and `Failed`.
  Read the ports from the reference every time.
- **Settings are strings**, also numbers: `"timeout": "60"`. Lists and tables have shapes
  of their own, given by `shape` in the reference, such as `"sort_keys": [["population",
  "descending"]]`.
- **Expressions** are as in the field calculator of QGIS: attributes by name, or in
  `"double quotes"` when they have dots or spaces (`"leg.startTime"`), text in
  `'single quotes'`, `||` joins text. The functions are under `expressions` in the reference.
- **Paths** are relative to the folder of the model. Outputs usually write to `out/`.
- **Variables** are written `$(name)` in any setting and defined in `variables`. Use them for
  what a user will change: an address, a date, a threshold. Never write a password into a
  model: database and API connections are named, and kept in the settings of the user.
- **Coordinate systems** are written `EPSG:3006` (SWEREF 99 TM) or `EPSG:4326` (degrees).
- **An input port may take several connections**; their streams are put together.
- **x and y** place the nodes on the canvas. Put them about 260 apart from left to right,
  in the order the data flows.
- Give every node a short `name` that says what it does in this model.

### Examples to start from

| Task | Example |
| --- | --- |
| Read a file, reproject it, write another format | `examples/buildtest.ographis` |
| Filter, join by attribute and by location, statistics | `examples/skaraborg.ographis` |
| Call a web API for each feature, take values out of the JSON, make points | `examples/apis/nominatim_addresses.ographis` |
| Explode a JSON list into features, encoded lines, a planned trip | `examples/apis/otp_routes_and_departures.ographis` |
| Read pages of an API and statistics in JSON-stat | `examples/apis/kolada_inhabitants.ographis`, `examples/apis/scb_population.ographis` |
| Draw a map as a picture or a PDF | `examples/map.ographis`, `examples/village.ographis` |
| Charts | `examples/chart.ographis` |
| Terrain, rasters and 3D | `examples/terrain.ographis`, `examples/raster_tools.ographis`, `examples/extrude_tin.ographis`, `examples/house.ographis` |
| Point clouds: ground, terrain models, buildings | `examples/point_cloud.ographis` |
| 3D models for Blender: terrain with a picture, buildings grouped and named | `examples/blender.ographis` |
| A folder of files read as one | `examples/stops.ographis` |

Every example passes `og check`; a test makes sure of it.

### Three tasks, written as models

- *"Make a GeoPackage of the towns with more than 10 000 inhabitants"*: `Reader` →
  `Tester` (`population > 10000`) → `Writer` from the port `Passed`. The model above.
- *"Geocode the addresses in this CSV with Nominatim"*: `Reader` → `HttpCaller` (an address
  built with `||`, `per_second` 1) → `JsonExtractor` (`[0].lat`, `[0].lon`) → `PointCreator`
  (EPSG:4326) → `Writer`. See `examples/apis/nominatim_addresses.ographis`.
- *"How many bus stops are there in each municipality?"*: two `Reader`s →
  `SpatialJoiner` (stops in `Features`, municipalities in `Join`) → `StatisticsCalculator`
  grouped by the municipality → `Writer`. Or simpler with *Count points in polygons*
  (`PointCounter`); look up its ports in the reference.

## Changing the code

See [`CONTRIBUTING.md`](CONTRIBUTING.md) and [`ARCHITECTURE.md`](ARCHITECTURE.md). In short:
`cargo fmt --all`, `cargo clippy --workspace --all-targets` with no warnings, and
`cargo test`. A new node type goes in the registry in `crates/og-engine/src/nodes/mod.rs`,
with a test; then write `docs/nodes.json` and `docs/model.schema.json` again with
`og nodes --json` and `og schema`, which a test checks.

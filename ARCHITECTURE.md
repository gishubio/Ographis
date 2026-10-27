# How Ographis is built

A map of the code for those who want to read it or change it. How to build, test and send
in a change is in `CONTRIBUTING.md`.

## Four crates

| Crate | What it does | Depends on |
| --- | --- | --- |
| `crates/og-engine` | The data model, the expression language, the tools and the running of a model. CSV, txt, JSON, OBJ and glTF are read here. **No GDAL.** | nothing of ours |
| `crates/og-gdal` | Everything that needs GDAL and PROJ: the file formats, web services, PostGIS, rasters, PDF, reprojection. It implements the trait `Backend`. | og-engine |
| `crates/og-cli` | The command line `og`: `og run`, `og info`, `og doctor`, `og nodes`. One file. | og-engine, og-gdal |
| `crates/og-gui` | The program `ographis`, built with egui. Everything that is drawn is in the library, so that the tests can drive it without a window. | og-engine, og-gdal |

`vendor/egui-snarl` is the canvas library with a small patch (selection box, panning,
connections with bend points); `vendor/egui-snarl/OGRAPHIS.md` says what was changed.

The line between the engine and GDAL is deliberate: the engine and its tests build and
run without GDAL, and another backend could replace `og-gdal` without the tools noticing.

## A run, from file to result

```
model.ographis ──► Workspace::from_json          run.rs: nodes, edges, variables, groups
                     │                            (old files are brought up to date here)
                     ▼
                 run_opts(ws, backend, dir, …)    run.rs: the nodes in order, each when
                     │                            what it needs has run
        for each node│
                     ▼
     expand_params   $(variables) put into the settings
     NodeType.run    nodes/*.rs: fn(&P, &Inputs, &mut NodeCtx) -> Result<Outputs>
                     │   P        the settings, with helpers to read them
                     │   Inputs   the tables that came in, by port
                     │   NodeCtx  the backend, the log (notes), stop and pause
                     ▼
                 Outputs: a Table per output port ──► the next nodes, and RunResult
```

- **`Table`** (`model.rs`) is what flows on a connection: fields, features (attributes,
  a `geo` geometry and heights), a coordinate system as an EPSG code, and optionally a 3D
  model (`mesh.rs`), an image or a grid of heights.
- **`RunResult`** holds every node's result: status, the tables on its ports, the files it
  wrote, the time it took, and the log.
- **Reusing what has not changed**: `node_keys` in `run.rs` gives each node a key made of its
  settings, the variables, what comes in and the dates of the files it reads. With
  `RunOptions.cache`, a node whose key is unchanged takes its result from the last run.
- **A fault** in a tool is caught by `crash::guarded`, becomes an error on that node, and is
  written to `crash-log.txt`. The rest of the model runs.

## The tools

Every tool is a `NodeType` (`nodes/mod.rs`): its name in model files, the title the user
sees, its category in the palette, its ports, its settings (`ParamSpec`) and the function
that runs it. The tools are in a file per category:

| File | Tools |
| --- | --- |
| `input.rs`, `output.rs`, `database.rs` | Input, Output, View, zip, PDF with layers, PostGIS |
| `attr_filter.rs`, `attr_values.rs`, `attr_text.rs`, `attr_table.rs` | Tools for attributes |
| `geom_overlay.rs`, `geom_shape.rs`, `geom_make.rs`, `geom_transform.rs`, `geom_inspect.rs` | Tools for geometry |
| `web.rs`, `json.rs` | HTTP request, STAC search and the JSON tools |
| `map.rs`, `chart.rs`, `scene.rs`, `terrain.rs`, `raster.rs` | Maps, charts, 3D pictures, terrain and rasters |
| `network.rs`, `workflow.rs` | Shortest route, Run model |

Each file ends with `TYPES`, the registrations of its tools; `NODE_TYPES` in `mod.rs` puts
them together. A section in a file starts with a line such as
`// ---------- Buffer (Bufferer) ----------`: the title, and the name in model files.

Beside `nodes/`, the files of `og-engine` that read formats of their own are `csv_io.rs`,
`json.rs`, `obj_io.rs`, `gltf_io.rs` (also meshopt and the property tables of 3D Tiles 1.1),
`tiles3d.rs` (tilesets, explicit and implicit, and point cloud tiles) and `pointcloud.rs`
(LAS and LAZ), `ifc_io.rs` (IFC), `citymodel.rs` (CityGML and CityJSON) and `parquet_io.rs`
(GeoParquet). DXF is read through GDAL in `og-gdal/src/dxf.rs`, which tidies what GDAL gives
into attributes. `history.rs` keeps the runs of a model and its lock, for `og run` and the
program alike. `expr.rs` is the expression language that Create attribute, Test, Raster
calculator and others share.

**Names.** The name in model files (`Bufferer`, `Reader`, `Offsetter`) never changes, so
that old models open. When a port is renamed, `Workspace::from_json` in `run.rs` moves old
connections to it, as for the port Polygons of Join by location, now Join. The title (Buffer, Input, Move by x, y and z) is what the user sees and
can change. The README has a table of both.

## The program

The forms are not drawn by hand for each tool: `params.rs` builds the form of a node from
its `ParamSpec`s, a kind of field for each `ParamKind`. A setting that only applies
sometimes is hidden by a rule in `relevant()` in `params.rs`.

| File | What it does |
| --- | --- |
| `app/mod.rs` | `App`, the state of the program: the model, the selection, the latest result |
| `app/run.rs` | Running in a thread of its own, stop, pause, reuse |
| `app/frame.rs` | The panels of the window and the keyboard shortcuts |
| `app/canvas.rs`, `graph.rs` | The canvas, the nodes as they are drawn, connections and group boxes |
| `app/form.rs`, `params.rs` | The panel on the right and the forms |
| `app/data.rs`, `map.rs`, `mapdata.rs`, `view3d.rs`, `stats.rs` | The table, the map, the 3D view and the statistics |
| `app/edit.rs`, `app/wires.rs`, `app/groups.rs`, `app/files.rs` | Editing: nodes, connections, group boxes, files dropped |
| `app/top_bar.rs`, `app/palette.rs`, `app/navigator.rs`, `app/dialogs.rs` | Menus, the list of tools, Data sets, the windows that open |
| `sources.rs` | Looking up layers, sheets and files in the background |
| `recovery.rs`, `recent.rs`, `settings.rs` | Kept changes, the latest models, the settings window |

## Tests

| Where | What | Needs |
| --- | --- | --- |
| unit tests in `og-engine/src` | expressions, geometry, formats | nothing |
| `og-engine/tests/flows.rs` | whole models through the engine | nothing |
| `og-gdal/tests/roundtrip.rs`, `apis.rs` | formats, rasters, PDF, web services against local servers | GDAL |
| `og-cli/tests/cli.rs` | the command line | GDAL |
| `og-gui/tests/ui/` | the program without a window: clicks, drags, keys, screenshots in `target/screenshots` | GDAL and a graphics driver (software rendering will do) |

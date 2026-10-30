# Changes

What changed in each version, the newest first. Models from earlier versions open and run
in later ones; where a tool changed what it does, it says so here.

## 0.90

Rasters on maps

- **Draw map draws rasters**: a picture as it is, and a grid of values by a colour ramp, with a
  colour of one's own for each value (such as green for seen and nothing for not seen), or as
  grey relief; with an opacity, and in the legend. Before, a raster was drawn as its outline.
- The map under the canvas shows grids in colours: classes a colour each, other values by a
  ramp over relief, rather than all in grey.

For someone new

- **A start screen** with the latest models and examples to open with a click.
- **Measure** above the map, beside Zoom to data.
- Help → **Check for updates** and **Report a problem…**, which writes a report with the
  version and the latest errors, to send as an issue on GitHub.

Rasters and terrain

- **Interpolate points to raster**: TIN, natural neighbours, inverse distance or the nearest.
- **Catchments and streams**: catchments above outlets or of all that drains off the terrain,
  streams with their Strahler order, and the flow as a raster.
- **Resample raster** and **Fill empty cells**. *Reproject* can give a raster a cell size.

Point clouds

- **Classify ground points** (SMRF) for clouds without classes, as from a drone.
- **Building heights from points**: the height of each building, and the shape, slope and ridge
  direction of its roof.
- A made-up drone point cloud and `point_cloud.ographis` among the examples.

Formats and data

- **Overture Maps download**: buildings, places, addresses, roads, land use, water and
  borders, only within a rectangle, fetching only the parts of the files that hold it.
- **DWG drawings** are read, made DXF by acadrust and read as DXF is.
- **GeoParquet is written**, as GeoParquet 1.1 with Zstandard and a box column for each row.

## 0.89

Rasters

- **Values from raster** gives each feature the value where it lies: the height at each
  address, the land cover under each tree.
- **Zonal statistics** counts the cells within each area: mean, min, max, sum, median,
  spread, the most common value, and the share or area of each class.
- **Slope and aspect**: slope in degrees or percent, aspect, position against the
  surroundings (TPI) and roughness.
- **Clip raster** keeps the cells inside or outside areas. (Reproject already warps rasters.)
- **Raster to areas** and **Features to raster** go between rasters and vectors.
- **Reclassify raster** by ranges.
- **Distance raster**: the exact distance to the nearest feature.
- **Visible area** (viewshed) from one or many places, with the eye and what is seen at
  heights above the ground and the curve of the Earth.

Point clouds

- **Point cloud to raster**: surface and terrain models, mean, count and density, with holes
  filled if asked.
- **Height above ground**, from a terrain model or the ground points of the cloud.
- **Filter point cloud** by class, return, height and area.
- **LAS and LAZ are written**, as LAS 1.4 with classes, intensity, returns, colours and the
  coordinate system.

Also

- The dots on a connection into the node at work now move with the data, from the output to
  the input.
- A new example, `raster_tools.ographis`.

## 0.88

For AI assistants

- `og nodes --json` gives every node type with its ports and settings, their keys, kinds,
  choices and defaults, and the expression functions, as JSON; it is also in
  `docs/nodes.json`. `og schema` gives a JSON Schema of the model format
  (`docs/model.schema.json`).
- **`og check`** checks a model, or a folder of models, without running it: unknown node
  types (with the nearest names), ports and settings, choices, expressions, variables that
  are not defined and inputs left open. It says what would be right, and exits with 1 on
  errors.
- `AGENTS.md` tells an assistant how to write a model, with examples to start from.

On the canvas

- **Quick add**: a double click on empty canvas, or `/`, opens a box where a tool is added by
  writing a few letters of its name. A connection let go on empty canvas opens the same box,
  with only the tools that fit.
- **Align and spread out** selected nodes (Edit → Align, or the menu of a node).
- **Scrollbars** at the right and bottom edges of the canvas.
- Dots move along the connections of the node at work while a model runs.
- **A description of the model** (Model → Description…), shown on the canvas and in the
  picture of the model.
- **Copy the model as a picture** (Ctrl+Shift+C).
- **Trial run** (Ctrl+Shift+F5, and `og run --sample N`): every input reads only its first
  features, 1000 unless Settings → General says otherwise.
- **Suggestions in expressions**: attributes and functions that begin with the word being
  written, taken with Tab or Enter. They can be turned off in Settings → General.

Data and map

- **Filter** above the table, by a word or an expression; the map and the statistics follow.
- **Background maps of your own**: XYZ tiles, WMS and WMTS, also with a user and password or
  a key, set up in Settings → Background maps and chosen above the map.
- **Labels** on the map from an attribute.
- Right-click the map to **copy the coordinates** of a point, or to **measure** a distance or
  an area, on the ellipsoid.
- Ctrl and a click picks several features in the 3D view.

New tool and formats

- **Create duplicate** (`Duplicator`) makes copies of each feature, a number of them or as
  many as an expression says, numbered.
- **Vector tiles**: .mbtiles, .pmtiles (also on the web), a folder of tiles and
  `{z}/{x}/{y}.pbf` addresses on the web, at a zoom level, with features cut by the tiles put
  together again. Not yet the MapLibre Tile format (MLT).
- A .zip with several data files, also on the web, reads the first and lists the others in
  the log; *File in the zip* chooses another.

## 0.87.1

- **Explode JSON list** no longer copies the whole answer it explodes into every element.
  With every stop of an OpenTripPlanner, megabytes times thousands of stops, that took all
  the memory there was, and the system stopped the program without a word: the example for
  OpenTripPlanner did so. *Explode lists in step* and *JSON-stat to table* had the same
  fault. The example now runs in 0.5 GB with 15 000 stops.
- A run in the program writes its log beside the model as it goes, in `<model>.runs\logs`,
  as runs from the command line do, so that it is there also when the program stops.
- The statistics of a column of long texts show the beginning of the smallest and largest.

## 0.87 — the first public preview

New tools

- **OpenStreetMap download** fetches buildings, roads, water, land use, places, railways or a
  filter of your own from an Overpass server, within a rectangle or what comes to *Extent*.
  A rectangle larger than allowed (25 km² by default) is made smaller, with a warning.
- **Extract bounds** gives xmin, ymin, xmax and ymax (and zmin, zmax) of each geometry.
- **First and last point** makes points where lines begin and end.
- **Reproject coordinate attributes** reprojects an x and a y kept in attributes.
- **Random number** with the decimals you choose, or whole numbers.

Databases and passwords

- **SQL Server**: Input and Output read and write tables with geometry through ODBC, with a
  user or the login of Windows, a spatial index and a primary key. A connection's kind is
  chosen in the settings.
- **Passwords and API keys** are kept in the Windows Credential Manager. Those in the
  settings file are moved there the first time the settings are read.

Changed

- *Reverse lines* is now **Reverse geometry**, and turns the outlines of areas too, unless
  asked not to.
- *Extract coordinates* is now **Extract coordinates from point**.
- **Geometry to 2D** drops the heights and a 3D model the stream carries, so that the map
  and the 3D view show the data flat. It used to only state that the data was 2D.
- **Make diorama** takes a list of animals, such as `sheep 8, pigs 4, 2 horses, dinosaur 1`:
  sheep, pigs, cows, horses, rabbits, dogs, cats, bears, elephants, giraffes and dinosaurs,
  each where it belongs. The form says what *Heights* wants: a terrain model read as a grid
  of heights.
- **Render 3D scene**: a background colour given, such as `#57cfda`, is used without first
  choosing *colour* as the background, and colour settings get a colour swatch.
- **Draw chart** as interactive HTML is in English.
- **HTTP request** writes over `_response` and `_status` when they are there already, as when
  the answer of one request is asked again, instead of failing with two of the same name.
- Reading IFC tells in the log how many entities and elements there are, how long the
  parsing took, and each element that takes two seconds or more.

The window

- **Several rows** are picked with Ctrl and Shift in the table, and with Ctrl in the map;
  the map draws them all, and *Copy the picked rows* copies them for a spreadsheet.
- Dragging a node near the edge of the canvas **moves the canvas** along.
- A click on a building from IFC in the 2D map picks its row also where its parts overlap.
- A row with megabytes of text, such as the answer of an API, no longer takes the program
  down when clicked: the cell shows the first 300 characters and how many there are.
- A frame of the window slower than 0.7 s is told in the log, with what took the time.

Examples and documentation

- `examples/apis/otp_routes_and_departures.ographis`: OpenTripPlanner 2.4 — stops, a
  planned trip with its legs as lines, the departures of a stop, and a count of the
  departures from one stop that reach another between two times.
- A short README; the details are in `docs/` (GUIDE, TOOLS, EXAMPLES, DESIGN, TESTING) and
  this file.

## 0.86

Terraces from a terrain model in Make diorama; Raise points (lollipops); joins inner, left
and full with the first or all matches; Geometry to text; duplicates by geometry; charts as
interactive HTML; Slides to PowerPoint and Impress; the model as a picture or PDF
(`og picture`); `og formats` and `docs/FORMATS.html`.

## 0.85

Make diorama: a round board, clouds, trees, houses, sheep and boats, moods of light, a
background colour and a title. 0.85.1 added the lists of values for trees, houses and sheep.

## 0.84

Make diorama, Round shapes, Filter 2D/3D, TIN to raster, Create features; Render 3D scene
with a parallel projection and without a floor.

## 0.83

3D models follow their features through filters.

## 0.82 and earlier

The versions before this file was kept. Ographis was called GeoBobbo up to 0.74.

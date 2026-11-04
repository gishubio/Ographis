# Changes

What changed in each version, the newest first. Models from earlier versions open and run
in later ones; where a tool changed what it does, it says so here.

## 0.94.1

- The scroll bars of the canvas stop at the edge of the model. Before, dragging one went on
  past it, as far as it was dragged.
- KML and KMZ keep every attribute, in the data of each placemark: GDAL's driver, which
  wrote them in 0.94, put them in the wrong places by their order and lost all but the name.
  They are now written by Ographis, and a KMZ is laid out so that QGIS finds its attributes too.
- *Origin* of Output applies to OBJ as well as glTF (issue #37): `none`, or `0, 0, 0`, writes
  the coordinates as they are, without moving the model; a point of your own writes it around
  that; empty, as before, near its middle. A glTF that reaches far from its origin, such as in
  SWEREF 99 TM without one, gets a warning, since glTF keeps coordinates to about seven digits.

## 0.94

Heights, sea and formats

- **Change height system**: heights above the ellipsoid, as GNSS and many point clouds give
  them, to RH 2000 and back, or between NN2000, N2000, DVR90, DHHN2016, EGM96, EGM2008, NAVD88
  and others, with the grid of the geoid. For the heights of coordinates, point clouds, grids
  and 3D models. The grids for Sweden, Norway and EGM96 come along (about 5 MB, in
  `data/proj-grids`); others are fetched from cdn.proj.org when allowed. Without the grid it
  says so, rather than leave the heights as they were. `og doctor` checks it.
- **Nautical charts in S-57** (.000, with updates applied) are read with every object class
  named to be read, and **BAG and S-102** with depth and uncertainty as two named bands. A
  GeoTIFF of several bands read as a grid keeps them all, with their names.
- **KML, KMZ, GML 3.2 and GPX** are written. KML and GPX in WGS 84 degrees; GPX with points as
  waypoints, lines as tracks, heights as ele and the other attributes in extensions.

New tools

- *Parse coordinates*: points from coordinates written as text, in degrees, minutes and
  seconds, degrees and minutes, decimal degrees or metres, with or without N and E.
- *Profile along line*: the height of a surface along lines, as points for a chart and as 3D
  lines with their length over the ground, climb and descent.
- *Volume between surfaces*: cut and fill between a surface and a reference or a level, per
  area, and the difference as a raster.

Changed

- **Input reads the files a stream names** when a stream is connected to its new port Paths,
  such as the answers HTTP request keeps as files. This did *Read files named in an
  attribute*, which is gone from the list; models that have it open with an Input in its
  place.
- *Check topology* takes a boundary, such as the municipality the areas should fill: then
  gaps that open to the outside are found too, and what reaches beyond it comes out on Beyond.
- **DWG**: the round faces of 3D solids (cylinders, cones, spheres and tori), regions and
  surfaces are read as a 3D model too. What a drawing has that is not read, such as pictures
  and references to other drawings, is said in the log.
- *Encoding* of Input also applies to DXF, with more encodings to choose (Windows-1250, -1251
  and -1257, ISO-8859-15). Text with characters that could not be read, shown as �, is said in
  the log, with whether another encoding may help. In the drawing of issue #34 they were lost
  already in the file.
- *Manage attributes*: several attributes can be ticked and kept, removed or given a type at
  once.
- *Concatenate attributes* takes fixed texts in single quotes beside the attributes: `'0', kod`.
- *Report*: the tables can have one of a few colours, and in the web page a click on a column
  heading sorts by it.
- *STAC search* sends the time as full timestamps, which Earth Search insists on:
  2025-06-01/2025-08-31 was refused with *datetime value is invalid*. A month or a year, such
  as 2025-06, is understood as the whole of it.
- *Read satellite bands* picks the pictures spread over the whole time, the least cloudy in each
  part, when there are more than *At most pictures*. Before, it took the first by time, and
  with the latest first from the catalogue a summer could be only August. Paths in a catalogue
  of your own are found beside the model, and when no picture can be read, the error says why.
  The example `sentinel_ndvi.ographis` asks for up to 100 items.
- With a series of rasters, the map shows the picture of the row picked in the table.

Fixed

- The settings window keeps its size whichever tab is chosen, also with a connection in a
  form wider than the others.
- A node that is removed takes what it read with it: a new Input given the same id showed the
  data of the old one on the map.
- The wheel zooms around the pointer in the 3D view too, as on the map.
- `build.cmd` removes the files an earlier version had when a new one is unpacked over it, and
  stops with a clear message if a file has become a folder: unpacking 0.93 over 0.92 left
  `nodes\output.rs` beside the new `nodes\output\`, and the build stopped with errors that did
  not say why.

Tests and documents

- **Performance tests**: four cases of made-up data, a million points joined to squares,
  200 000 areas buffered, the slope of 16 million heights and 5 million points made a terrain
  model, run in a release build with their time and peak memory. See docs/PERFORMANCE.md.
- What the computer needs is written down in the README and docs/GUIDE.md.

## 0.93

Large data

- **Point clouds are kept as columns**, about 30 bytes a point instead of some 1 000, and
  every point is read: the limit of two million is gone. A tile of 4 million points, of
  which 0.92 read half in over 2 GB, now takes about 120 MB, and a whole run with it some 700 MB. Height above ground, Filter
  point cloud, Thin point cloud, Point cloud to raster, Classify ground points, Building
  heights, LoD2 buildings and Reproject work on the columns; other tools get the points as
  features, with a warning when they are many. Clouds put together, such as the tiles of a
  folder, become one cloud. The map shows every n-th point, up to half a million.
- The log says how much memory a node keeps when it is over half a gigabyte.

Remote sensing

- **Read satellite bands**: the bands of the pictures that STAC search found, read onto one
  grid over the area, a picture for each time, with the names of the bands; only the part of
  each file that covers the area is fetched.
- **Mask clouds** by the scene classes of Sentinel-2 or QA_PIXEL of Landsat.
- **Spectral index**: NDVI, NDWI, NDMI, NDBI, NBR, EVI, SAVI or an expression of your own.
- **Composite over time**: the median, mean, highest, lowest or greenest, for the whole time
  or each month, season or year, with how many clear values each cell had.
- A series of pictures is written as a GeoTIFF for each time, and bands keep their names.

Vector tools

- **Check topology** (overlaps and gaps between areas) and **Eliminate slivers**.
- **Merge lines**, **Points along lines** and **Offset lines**.
- **Locate along lines** and **Place along lines**: linear referencing, as roads in NVDB.
- **Pivot** and **Unpivot**: tables from long to wide and back.

Fixed and changed

- DWG and DXF: **meshes, polyface meshes, 3D faces and the flat faces of 3D solids** are read
  as a 3D model, which was asked for in issue #34. Curved faces of solids are not yet.
- The settings window no longer moves when another tab is chosen, *General* is the first tab,
  and the background map the map under the canvas starts with can be chosen there.
- *Stylize picture*: the style that was named after a home computer of the 1980s is now *8 bit
  retro style*. Models that use it are unchanged.
- *Report* and *Slides* take the comment of a tool as its heading when it has no name.
- The example models keep the names of their tools, with what each does in its comment. New
  examples: `vector_tools.ographis` and `apis/sentinel_ndvi.ographis`. The interface tests use their own copies of the models they open (see docs/TESTING.md).

## 0.92

Maps

- **Draw map on a background**: a colour of one's own, a **picture** (PNG or JPG, placed by
  its world file or covering the map), or a **background map** of tiles such as
  OpenStreetMap, CARTO Positron, Voyager and Dark Matter, or one's own from the settings. The
  tiles are kept for a month, can be made paler so the layers stand out, and the map says
  who made them in a corner.
- **OpenStreetMap and Overture download** give each kind on a port of its own, such as
  *buildings* and *roads*, with only the attributes that kind has, and all of it on *Output*
  as before. *Choose on the map* draws the rectangle to fetch within on the map.

Formats

- **QGIS project** as a format of Output (*QGIS project with its layers*): the layers, in their
  order and on or off, with the look they have in the form — colours, widths, classes of a
  number or a colour for each text, sizes after a number and labels — the features in a
  GeoPackage beside the project and pictures as GeoTIFF, and a background map if asked.
- **3D Tiles 1.1** are written: a folder with tileset.json and glTF tiles, placed on the globe
  for Cesium, deck.gl and the 3D views of QGIS and ArcGIS, with a height to add for the
  geoid.
- **CityGML 2.0** is written: buildings from Extrude in LoD1, or LoD2 with ground, walls and
  roofs, with their attributes.
- **Cloud Optimized GeoTIFF** is written, for pictures and grids.
- **GTFS** timetables are read, from a zip or a folder: stops, routes, trips as lines,
  shapes, stop times and calendars.
- **NetCDF and Zarr** data cubes are read, such as climate and weather data: each variable a
  layer, the steps of time as attributes named after their dates (`time_2026_07_01`), or one
  step as a grid or a picture by its number or its date.

Tools

- **Report**: a web page or a PDF of A4 pages with a title, a text of one's own first and
  last, and a section for each stream connected: maps, charts and 3D scenes as pictures, the
  rest as tables.
- **Filter by value**: tick values in the list and remove them all at once, or keep only
  those.

Fixed

- DWG: arcs in the hatches of some drawings, such as the old centre of Krasnoyarsk, were
  drawn as large circles across the drawing. The layer names of drawings in Cyrillic and
  other code pages came out garbled.
- A time of day in a file, as in GTFS, could not be read.

## 0.91.1

- **Geometry from text** reads HERE's flexible polyline, as a line or an area, and JSON
  lists of points with latitude and longitude, as TomTom gives routes and reachable ranges.
- An encoded polyline that began with `{` was taken for JSON and failed, as many routes of
  OpenTripPlanner around Göteborg do. Now the kind chosen is always used.
- Web addresses with å, ä, ö, spaces or brackets in them, such as `range[type]` of HERE,
  are sent %-coded, as servers expect.
- New examples in `examples/apis`: `here_examples.ographis`, `tomtom_routing.ographis` and
  `otp_to_drottningtorget.ographis`, with a hundred made-up start points around Göteborg.

## 0.91

For Blender and 3D

- **LoD2 buildings from points**: buildings with roofs fitted to the point cloud (flat, shed,
  gable or hipped) and walls up to their edge, with how well the roof fits.
- **Terrain to 3D model**: the heights of a raster as a surface with an orthophoto or a map
  draped over it, and a block under it if asked.
- **glTF for Blender**, as settings of Output: the objects named and grouped by attributes,
  materials named by an attribute, where the model lies kept for BlenderGIS, and a sun and a
  camera. Lines and points without a 3D model are written as glTF lines and points.

Networks

- **Build network**: ends joined, lines split where they cross on the same level, doubles
  taken away, with the loose ends and the parts not joined on *Problems*.
- **Service areas** within minutes or metres, and **Allocate to nearest**: the catchment of
  each facility along the roads.

Formats

- **PMTiles are written**: vector tiles in one file, for maps on a web server without a tile
  server.
- **GeoParquet within a rectangle** reads only the row groups that reach into it, also from
  any web address, fetching only those parts.
- **GPX** (tracks, routes, waypoints) and **photos** as points where they were taken, from
  their Exif.
- **Overture Maps and OpenStreetMap download** take several kinds at once, ticked in a list,
  and only what reaches into the area: features whose box only reaches in are left out, and
  large ones, such as land cover along a mountain range, are cut at its edge.
- **DWG and DXF keep their colours** on the map under the canvas, and Draw map can draw in
  them. Before, everything was drawn in the colour of the layer.
- **PostGIS**: *Empty it and fill it again* keeps the table with its indexes, rights and
  views, in one transaction; and very large writes can be committed in parts.

Tools

- **Create raster**, **Focal statistics** and **Cost distance**.
- **Thin point cloud**: a point in each cube, or none closer than a distance, per class.
- **Buffer** on one side of a line, and with flat ends. **Remove holes**, all or those smaller
  than an area.
- **Draw chart** draws **line charts** too, over numbers or dates.
- **Stylize picture**: as on a Commodore 64, or as LEGO plates with the list of plates.

Running

- Model → **Schedule…** sets up runs in the Task Scheduler of Windows.
- A **lineage** of each run, beside the history: what was read and written, with sizes,
  times and fingerprints, and which inputs each output came from.
- A **notice when a long run is done**, if asked for in the settings.

Working in the program

- The canvas by the **keyboard**: Tab between nodes, the arrows to move them, Enter to the
  form, Ctrl+L to connect, **Ctrl+F to find a node**.
- **A map of the model** in the corner of the canvas.
- **Values suggested** in expressions after `=` and a quote.
- Screen readers are told what each node is and what it is connected to; errors and
  warnings have a sign as well as a colour; Ctrl and + or − make everything larger, and stay
  so; and *Reduce motion* in the settings.

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

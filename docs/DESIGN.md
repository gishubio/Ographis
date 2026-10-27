# Choices and limitations

Why Ographis works as it does, and what it does not do yet.

## Choices that were made

- **Planar geometry in the model's own coordinate system.** Clipping, buffering and
  overlay are done in the coordinates the stream has. Area and length are planar in
  metre-based systems and geodesic in degrees.
- **Distances are in metres.** Buffer refuses a stream in degrees and asks for a
  Reproject before it.
- **Points on a boundary count as inside**, in Clip, Join by location and Count points in
  polygons. A point on the boundary between two areas therefore matches both. It is
  counted in both, gets the attributes of the first, and is reported in a warning.
- **Join by location does not overwrite.** An attribute of the polygon with a name the
  feature already has is not fetched, and the node says which. Give a prefix to get it
  under another name. A line or an area is placed by a point that lies on it, and keeps
  its own geometry.
- **Count points in polygons counts single points.** Lines, areas and multipoints in the
  Points port are not counted, with a warning. A polygon with no points gets 0.
- **Tools are named by what they do, types keep their names.** *Sort* is `Sorter` in the
  file, as before. A node that still has the name the program gave it, such as `Sorter` or
  `Sorter_2`, is shown as *Sort* and *Sort 2* when an older model is opened. A name
  you chose is left alone. The ports have the names they had.
- **GeoJSON is written in the coordinate system of the stream.** The file says which, and
  GDAL and QGIS read it right, but programs that expect WGS 84 will not. The output says so
  in the log when the stream is in something else. Put Reproject to WGS 84 before the
  output for a file to use on the web. Coordinates are written with millimetres in metre
  systems and seven decimals in degrees.
- **Rounding is a setting of the tools that measure.** Empty keeps all decimals, 0 gives
  whole numbers.
- **Sheets and CSV get no geometry unless asked.** Excel, LibreOffice and CSV have no
  geometry of their own, so the output leaves it out. *Geometry* in its form can write
  points as x and y columns, or any geometry as WKT.
- **Overwrite is the default; append adds after what is there.** In a GeoPackage the
  table, in a workbook the sheet, otherwise the file. New features are matched to what is
  there by the names of the attributes. An attribute the table or sheet lacks is added as
  a new field or column, except in CSV, where it is left out with a warning. A stream in
  another coordinate system than the layer is refused. GeoJSON is read and written again
  whole; the other formats are added to where they are.
- **Variables are text put in place.** `$(name)` is replaced by the value before the node
  runs, in every setting of every node: paths, expressions, names and values. A number in
  an expression is written as it is, `pop > $(least)`; a text in quotes, `'$(region)'`. A
  name that is not defined is an error on the node that uses it. Variables are kept in the
  model file, and `og run --var` gives a value for one run without changing the file.
- **A web request is sent once per address in a run.** Features with the same address share
  the answer. Requests go one at a time. Status 400 and above, no answer within the time
  limit, and an address that cannot be reached all go to Failed, with the reason in
  `_error`. The answer is kept as text, up to 50 MB; reading JSON in it is for later.
- **Progress is reported for large reads.** An input that reads 100,000 features or more
  writes a line in the log every tenth of the way, when the format knows the number
  beforehand, as GeoPackage, Shapefile and CSV do. Otherwise every 500,000. Web requests
  are reported every tenth from 20.
- **Counts have a space between the thousands**: 4 303 726. Coordinates and other numbers
  are written as before.
- **The map draws everything**, as shapes up to 40,000 in view and as an image above. The
  image has no smoothing, and is drawn again when the view stands still.
- **CSV the way European files tend to look.** Semicolons and decimal commas are detected.
  Files that are not UTF-8 are read as Windows-1252. `0114` stays text.
- **A missing coordinate system is only guessed for CSV**, and the guess is logged. For
  GeoPackage, Shapefile and GeoParquet without one it is an error that asks you to set
  `crs`. Drawings and models, DXF, city models, OBJ, glTF and IFC, which seldom say, are
  read without one, with a warning: they can be looked at in 3D and written, but not
  reprojected or put on a background map until `crs` is set.
- **The output overwrites.** In a GeoPackage only its own layer is replaced.
- **The limits of Shapefiles are handled openly.** Field names are cut to ten bytes and
  logged. Mixed geometry types are split into `_point`, `_line` and `_area`.
- **Dropped files are stored by relative path** once the model has been saved in a
  folder above them, so that the folder can be moved as a whole. Otherwise by absolute path.
- **A 3D model goes with its features.** A tool that keeps some features as they were, or
  puts them in another order, such as Test, Filter by value, Remove duplicates, Sort or
  Take, keeps their part of the 3D model too, so that the buildings of a city model that
  pass a test can be rendered. A tool that changes the features gives no model, unless it
  makes one.
- **Many files, one stream.** An input of a folder reads the files in name order. The
  attributes are the union of what the files have, and a file that lacks one gets empty
  values. A file in another coordinate system is reprojected to that of the first, with a
  warning. An error names the file it came from.
- **The file name is a setting of the input**, not a tool. Further down the
  stream nothing knows which file a feature came from.
- **The date and time are those of the start of the run**, the same for every feature and
  every Stamp date and time in it. They are written as text: `2026-10-11` and `14:05:09`.
- **Excel has no geometry.** The output puts points in the columns `x` and `y` and other
  geometries in `wkt`, or leaves the geometry out. The input gives a table without geometry.
- **The first row of a sheet is taken as the column names.** The input has a setting for
  sheets where it is data.
- **Workbooks are written by the program, not by GDAL**, so that they can be styled. An
  output replaces its own sheet, in the place it had, and keeps the other sheets with their
  formatting. GDAL still reads the files. Formulas and charts in the other sheets have not
  been tried.
- **A styled sheet has its colours in the cells.** Every second row is filled when it is
  written. Sorted in Excel afterwards, the bands move with the rows. Columns are as wide as
  the name or the longest value among the first thousand rows, at most 60 characters.
- **A bold, locked top row is the default** for a workbook. Set them to No for plain data.
- **A raster becomes features.** Each cell with a value is a point in its middle or a
  square, with `col`, `row` and one attribute per band, named after the band when it has a
  description. Cells without a value in any band are left out. A raster with more than
  five million cells is only read with a limit, or as its outline.
- **The parts of a split attribute are typed like a CSV column**: whole numbers, decimal
  numbers or text, and a code with a leading zero stays text. A feature with nothing to
  split comes out once, with an empty part.
- **Filter by value compares as text, and numbers as numbers**: 2 and 2.0 are the same in
  a column of numbers, but `01` is not `1` in a column of text. Empty values go to Other.
  Removing a value removes its output and the connection from it. The other connections
  follow their values.
- **Manage attributes keeps what it does not know about**, unless *Attributes that are not
  listed* says Remove them. Once something in the list has been changed, every attribute
  that came in is in the list, and the setting only decides what happens to attributes
  that appear later. New attributes are calculated from what comes in, by the names it
  has there.
- **Neighbours are kept together by shared vertices.** With *Keep neighbours together*,
  Generalize finds what areas and lines share by vertices that are exactly the same, and
  simplifies each shared piece once. Where neighbours do not share vertices, as when one
  has a vertex the other lacks, each is simplified on its own there.
- **Aggregate keeps the geometries, Dissolve merges them.** Points, lines and areas are not
  put into one geometry: a group with several kinds becomes one feature for each.
- **The cells of a raster are marked as such** through the tools, so that Filter by geometry
  type can send them to Raster. Mixed with features from elsewhere they lose the mark.
- **The latest models are remembered** in `%APPDATA%\Ographis\recent.json`.
- **The map draws lines without sharp mitres.** Points that fall on the same pixel and
  points where a line turns straight back are left out of the drawing, since such corners
  were drawn as long spikes. The data is not changed.
- **An output with nothing connected does not run**, so that it does not write an empty
  file over a good one.
- **A disabled node lets nothing through.** A node that is fed only from disabled nodes is
  skipped, and so on downstream, so that an output does not replace a good file with an
  empty one. A port that also gets something from elsewhere runs on that. None of this
  counts as an error or a warning.
- **A file from the web is fetched to this computer and then read like any other.** It is
  kept in a folder per address under the temporary folder of the system, and fetched again
  in every run unless the input is told to keep it. An address without a file name gets
  one from what the server says it sends, so the JSON or CSV from a WFS request is read
  as that. The system proxy and certificates are used.
- **A zip file is read without unpacking**: the Shapefiles in it as layers, or else the
  first data file in it.
- **The limit of an input counts features for the input as a whole**, in the order the
  files and layers are read. What is above the limit is never read from GeoPackage,
  Shapefile and Excel. A CSV file is read in full and then cut.
- **An output that is told a format gives the file that extension.** In the program the
  list and the file name follow each other, so that the name always says what the file is.
- **An output changes types only in what it writes.** Text that is not a number becomes
  empty when written as a number, text longer than its length is cut, and both are
  counted in a warning. The stream itself is left as it was.
- **Undo goes back through the states of the model.** A drag is one step, and so is
  what is typed in a field before the field is left. The result of the latest run is kept
  and marked as out of date. Opening a model starts a new history.
- **Sorting the table is for looking.** It does not change the stream, and the number of
  a row stays its place in the stream.
- **A group box holds what lies in it**, decided by where the middle of each node is. What
  follows a box is settled when the drag starts, so a box does not sweep up what it passes.
- **A box follows its contents when all of it is dragged.** With several nodes selected, a
  box goes along if every node in it is among them, and with everything selected all
  boxes do. One node dragged alone leaves its box, which is how it is taken out of it.

## Known limitations

- GeoParquet: only geometry in WKB is read, not the native GeoArrow encoding, and the whole
  file is read before a rectangle is applied, not only the row groups within it. Writing
  GeoParquet is not there yet.
- DXF: drawings in DXF only, not DWG. Solids from ACIS (3DSOLID, REGION, BODY) are not read,
  nor the header of a binary DXF; dimensions become their lines and texts. Colours are those
  GDAL gives. Hatches become areas.
- City models: textures and other appearances of CityGML and CityJSON are not read; the
  surfaces are coloured by what they are. Geometry that an object only points to with an
  XLink elsewhere in the file is not followed. A TIN relief has no level of detail of its
  own.
- The run history is kept beside the model, so the folder of the model must be writable
  for it; when it is not, the model runs without history and lock. Locks on a shared drive
  are not checked across computers for a process that has ended, only by their age.

- 3D Tiles: tiles compressed with Draco, as Google's photorealistic tiles and many on Cesium
  ion are, are not read yet, nor instanced models (.i3dm), features told apart by a texture
  (EXT_mesh_features), or the metadata of tiles and contents in tileset.json. A tileset with
  both models and point clouds gives the models. Heights are above the ellipsoid; *Move the
  heights by* takes the geoid away as one number for the whole tileset. Reading 3DBAG has
  not been tried against its own server.
- Point clouds: each point is a feature, so at most two million are read unless more are
  allowed; thin with *Every n-th point*, or read only some classes or a rectangle. COPC is
  read as plain LAZ, from the start.
- Raster calculator counts with grids, not with pictures of colours, and with rasters that
  are not turned.
- Render 3D scene: clouds are painted on the sky and cast no shadows of their own, and the
  water shows nothing beneath its surface. Lamps light what they shine on directly, but
  not by light bounced on, and are not seen in mirrors and water; lit windows glow, but
  their light does not fall on the street. The windows drawn on plain walls are a pattern
  of 2.6 by 3 m from the bottom of each building, not the real windows. Outlines are not
  there yet. Without a denoiser, low sun, haze, depth of field and few rays give a grainy
  picture: *Best* quality smooths it.
- Extrude makes flat tops: no pitched roofs. Generate TIN from a raster places the corners
  on the middles of cells, and leaves out triangles over cells without a value by their
  middle, so the edge along such cells is ragged.
- IFC: swept disks and solids swept along curves, surfaces of extrusion and revolution,
  and unions that are to be melted into one are not read; a union keeps both parts as they
  are. Curved faces are made of triangles within 2 mm of the surface, or 1 % of the face.
  Edges along trimmed curves are drawn straight. IFC as XML (.ifcXML) and zipped (.ifcZIP)
  are not read. Very large files (several hundred MB) are held in memory while they are
  read.
- Labels: one line each, in Inter, the built-in font. Labels of lines are straight, along
  the line where it is at its middle, not bent with it. A label may cover the symbols of
  other points; only labels keep clear of one another.
- 3D models are written as OBJ and glTF, not as 3D Tiles. A model with more than 20 000
  objects, such as a large TIN, is one node in glTF.
- PDF: pages are RGB, without CMYK, crop marks or bleed. A map from Draw map is a picture in
  the PDF; *GeoSpatial PDF with layers* gives lines and areas as vectors.
- The primary key is set when a table or a layer is made. Added to one that is there, the
  rows get the key it has.
- A Cloud Optimized GeoTIFF on the web is read with the rectangle as an image or a grid.
  Read as points or squares, the whole file is read.

- Web APIs: pages that are asked for with POST and a body, as some STAC catalogues do, are
  not followed. STAC search finds items; their files are read with Input one at a time, by
  their address. Only EPSG coordinate systems can be chosen, not ESRI or IGNF codes. GTFS
  realtime (protobuf) and XML answers other than through WFS are not read.
- Everything is held in memory, also by the map, which keeps its own copy of the
  coordinates. A stream of 4 million road links needs several GB.
- Areas that cross the boundary of the clip area are clipped against the whole clip area,
  one at a time. That can be slow with many areas against a detailed boundary. Points and
  lines have an index.
- Dissolve keeps only the group-by attributes and a count.
- Join by location cannot take the feature in Join that a feature overlaps most.
- Join by location and Count points in polygons test against polygons only: there is no
  join to the nearest feature, and none by line or by distance.
- Dates are read as text, from Excel as `2026-10-11`. An output can write text, whole
  numbers and decimal numbers, not dates.
- The attributes at each step are known after a run, not before. A new node shows them
  once the model has been run with it.
- The counts fill in node by node during a run. Within a node only reading and web
  requests tell how far they have come.
- Stop and Pause act at once in reading, web requests, drawing a map and Generate TIN.
  Other tools are left to finish on their own three seconds after Stop: the run ends, but
  the tool goes on using the processor beside it until it is done, and an output that was
  writing may finish its file.
  The time a node was paused counts in its time in the log.
- What is reused is kept in memory, not on disk, and is gone when the model is closed. A
  node is run again when its settings, the variables, what is connected to it or the date
  of the file it reads have changed, but not when a file on the web has: use *Run everything
  again* (Ctrl+F5) for that. Outputs and Run model always run. *Run from here* does not run outputs and branches that are not after the
  node.
- Thiessen polygons are made from single points. Points in the same place share an area,
  with a warning. The cut against Boundary needs both streams in one coordinate system.
- Lines to polygons joins the parts of one feature, not lines from different features, and
  makes each ring an area of its own: a ring inside another does not become a hole.
- Extend lines goes straight on from the last piece of each end. It does not stop at
  other lines, as an extension to the nearest line would.
- The menus are worked from the keyboard with Alt, F10 and the arrows, but the letters are
  not underlined, and items within a menu have no letters of their own.
- The command line runs the whole model. *Run to here* and *Run from here* are in the
  window only.
- PDF has a picture and a title, nothing more: no legend, scale bar, text boxes or
  vector graphics. Features without a map are not written to PDF. The title is in
  Helvetica, which has å, ä and ö but not every letter.
- A file from the web is fetched whole before it is read, also when the input has a
  limit. Addresses that need a login are not supported. Fetched files are left in the
  temporary folder of the system.
- The files of a folder are read one after another, and all of them are read again in
  every run.
- The length of a field is only kept by formats that have such a thing: Shapefile and
  GeoPackage. The text is cut in every format.
- The input opens everything GDAL can open as vector data, but only GeoPackage,
  Shapefile, GeoJSON, Excel, LibreOffice Calc, CSV, txt and GeoTIFF are tested, and zip
  files with Shapefiles in them.
- A GeoJSON file holds one layer, and nested values in it are read as text. Newline
  delimited GeoJSON and TopoJSON have not been tried.
  `.xls` from before Excel 2007 is read only if the GDAL build has that driver.
- A tool goes into a connection by its first input and its first output, and only when
  both are free.
- Undo does not reach back before the model was opened, and does not cover what is
  not part of the model: the selection, what is folded out, and the zoom.
- Copy and paste carries nodes, notes and the connections between them, not group boxes
  or the variables of the model.
- Find nearest finds one feature, the nearest. Distances are in the units of the
  coordinate system, so a stream in degrees gives degrees, with a warning.
- Overlay areas takes the parts of the Overlay as they are. Where the Overlay areas overlap
  each other, Both gets a part for each of them.
- Split lines splits at crossings between all lines that come in, also of the same
  feature, but not where a line touches itself at a corner it already has.
- Statistics are of the stream as it came out of the latest run. The smallest and largest
  text go by the codes of the characters, not by the Swedish alphabet: upper case comes
  before lower case, and Ä before Å.
- With the attributes folded out on all nodes, nodes that stand close cover each other.
- The map has no labels, and no setting to turn them on. With many features in view it
  shows an image, where a click finds the feature but nothing is lit up under the pointer.
- The line under the top row of a sheet with *Lock the top row* is drawn by Excel and
  LibreOffice to show where the lock is. It cannot be taken away while the row is locked.
- Names from the data are for files, layers and sheets of Output, not for the PDF, PostGIS
  or a picture. `now()` is the time of the run.
- Heights (Z) are read from and written to formats that GDAL reads and writes with Z, such
  as GeoPackage, Shapefile and GeoJSON, but not to CSV, Excel or PostGIS. A tool that changes
  the geometry, such as Buffer, Clip or Simplify, makes it flat. Measures (M) are dropped.
  The 3D view draws at most 400 000 coordinates (of more, every n-th feature, spread over
  all of them, so that a point cloud shows its whole tile), 1.5 million triangles of a model
  (the smallest are left out of larger ones) and a grid at most 160 cells on a side.
- OBJ and glTF are read, not written: a model is written as its objects, with Z, to the
  formats that take them. Bump maps, normal maps, reflection maps and see-through parts of
  textures are not used; NURBS and curves in OBJ files are left out. glTF compressed with
  Draco or meshopt is not read, nor are skins, animations or cameras; a glTF placed on the
  globe through an extension, as from Cesium, is placed by the input like any other.
  Textures that are moved, turned or scaled (KHR_texture_transform) are read, but sheen,
  clearcoat and material variants are not: a velvet chair gets its colour and pattern,
  not the shimmer of the cloth.
- Render 3D scene follows one bounce of light from nearby surfaces, not more, and mirrors
  only in glass. Its pictures have no coordinates. Large pictures take time: 1600 × 1000 at
  16 rays per pixel is some seconds on eight cores for a house, more for a block.
- Read only within a rectangle is for features. A raster is still read whole.
- Shortest route keeps the whole network in memory, so a whole county of NVDB needs a
  good deal of it; it has not yet been tried on one. Turn restrictions and the cost of turns
  are not counted, and a point goes to the nearest road even when it is a motorway without
  an exit there; limit the network with Test first if that matters.
- Make zip file packs the files the outputs wrote in the same run.
- HTTP request sends one request at a time, and no login other than what fits in a header.
  GET requests to the same address are sent once a run; POST and the others every time.
- Variables are edited in the model. There is no window that asks for them when the model
  is run.
- Rasters are read, and written only as pictures: a map from *Draw map*, shaded terrain,
  or a raster read as an image. Cell values are not written. Apart from Shade terrain and
  Render 3D view there are no raster tools.
- A grid of heights is the first band of the raster. Reproject warps grids and pictures
  cell by cell, with the values between cells taken from the four nearest. A grid holds at
  most 40 million cells; a larger raster is read with coarser cells, with a warning. A
  folder of tiles becomes one grid, with the cells of the coarsest tile.
- Database and API connections are kept in `settings.json` in the folder for settings of the
  user, `%APPDATA%\Ographis` on Windows, and their passwords, keys and secrets in the
  Credential Manager of Windows, as generic credentials named `Ographis/db:<name>` and
  `Ographis/api:<name>`. Settings from before 0.9 have their passwords moved there the first
  time they are read. On other systems the passwords stay in the file, which only the user
  can read. A model names the connection, so it runs on
  another computer only where a connection has the same name; `og connections` lists them.
  Models from 0.61 and 0.62 keep their own connection settings and still run.
- The window lists tables that have geometry, not views or plain tables.
- Output to PostGIS does not create a schema; it must be there. A table that is replaced
  is dropped and created again, so views on it and rights given on it go with it. Adding
  to a table matches the attributes to its columns by name and adds those it lacks.
- OpenStreetMap is read by GDAL a layer at a time, and each layer reads the whole file. A
  country is slow; a clipped extract of a municipality or a region is quick.
- Check geometry finds what the geometry library geo finds, and repairs with GDAL, which
  needs GEOS; OSGeo4W has it. A repair that would turn an area into a line, or a line into a
  point, is not taken.
- JSON is read whole into memory, and a list of records is found among the values at the
  top or one level down. Newline delimited JSON (`.ndjson`, `.jsonl`) is read too. Paging
  through a service that sends its records a page at a time is not done: the first page is
  what is read.
- Build JSON writes numbers, text, true and false and empty as JSON has them. A date is
  written as the text it is.
- A disabled connection is drawn faint, not dashed. Moving a connection to another input
  makes it enabled.
- Draw chart draws bars, one value per name. There are no lines, pies, stacked or grouped
  bars, and no axis title other than the unit.
- GeoSpatial PDF draws areas, lines and round points with the colours of each layer. A
  layer with nothing to draw is left out. There are no labels, legend or scale bar.
- Shade terrain and Render 3D view run on the processor, on all its cores. Soft shadows on
  a grid of 4000 × 3000 cells took about 15 seconds on two cores; without shadows about 2.
  A picture wider than the grid has its light from the grid, spread over the pixels, which
  is quick.
- The 3D view is drawn from a surface of at most 1000 cells across, with the shaded relief
  of at most 3000 pixels laid on it. There is no reflection on water, no trees or buildings,
  and the camera always looks at the middle of the terrain. A raster read as an image with one band is shown in grey from its lowest to its
  highest value.
- *Draw map* has no labels, scale bar or north arrow, no symbols other than round points
  and no dashed lines. Sizes and colours by value are for Draw map, not yet for the
  GeoSpatial PDF. Gathered points are gathered by a distance on the picture, not around
  fixed centres, so the groups can change when the map is drawn wider. Very large maps are
  limited to 20,000 pixels on a side.
- Tanaka contours are drawn from the grid as it is read, piece by piece through the cells;
  lines are not joined into long lines.
- PNG and JPG are written with a world file (`.pgw`, `.jgw`) and a `.aux.xml` with the
  coordinate system. JPG has no transparency, so a see-through background becomes white.
- A LibreOffice sheet without a locked top row can be read back with Field1, Field2 …
  as names when every column is text: GDAL then cannot tell the names from the values.
  Lock the top row when it is written, or set *First row has names* to Yes in the input.
- The look of a sheet is one of a few choices. There are no borders, number formats beyond
  the decimals of the output, or conditional colours.
- In a LibreOffice sheet the widths of the columns are a guess from the number of characters.
- The values of Filter by value are compared as they are written. There is no match on
  case, ranges or patterns: use Test for that.
- Manage attributes changes the order by arrows, one step at a time, and so do the ports.
- Generalize keeps neighbours from parting, but can still let a simplified boundary cross
  another one nearby.
- The icons are drawn by the program in one style. They do not follow a theme of Windows.
- Which lists on the left are folded out is not remembered between sessions.

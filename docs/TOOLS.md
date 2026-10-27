# Inputs, tools and outputs

Every kind of node, and the format the models are saved in. `og nodes` lists them with their settings, and [FORMATS.html](FORMATS.html) the file formats.

## Inputs, tools and outputs

The tools are named by what they do. The second column is the name of the type in
model files, and what `og nodes` lists. Those names are the same as before, so
models from earlier versions open and run as they did.

| Node | In files | Does |
| --- | --- | --- |
| Input | `Reader` | Reads GeoPackage, Shapefile, GeoJSON, FlatGeobuf, SpatiaLite (.sqlite, .db), Excel, LibreOffice Calc, CSV and txt, and JSON without geometry, also zipped and from a web address, and GeoTIFF as points, squares, an outline, an image or a grid of heights, also only within a rectangle, from a Cloud Optimized GeoTIFF on the web without fetching the rest: from its overviews when it is read coarser. Point clouds from laser scanning, LAS and LAZ, a feature for each point with its height, class, intensity, returns and colour: only some classes, every n-th point, within a rectangle. COPC (`.copc.laz`), also on the web, through its octree: only the parts within the rectangle and the levels down to a spacing between the points are read, and from the web only those are fetched. 3D Tiles from a tileset.json on this computer or the web: Batched 3D Models (.b3dm), glTF tiles of 3D Tiles 1.1 with their property tables (EXT_structural_metadata), also compressed with meshopt and quantized as tyler and gltfpack pack them, composites, point clouds (.pnts) and implicit tiling with subtrees, at the coarsest level of detail, the finest or down to an error in metres, within a rectangle, placed in the coordinate system you choose, with the features of each tile and its table of attributes. **CAD drawings as DXF**: a feature for each entity, with its CAD layer, the kind of entity (LINE, LWPOLYLINE, CIRCLE, TEXT, INSERT…), its colour as #rrggbb, line type, line weight and handle, the text, height and angle of texts, and for a block the name, angle and scale and each of its attributes as an attribute of its own (ID, DJUP…). Blocks become points with their attributes, or are drawn out as their lines (*Blocks*); closed polylines, circles and ellipses become areas, or stay lines (*Closed polylines*). The log tells the AutoCAD version, the units, who saved it last and the extent. A DXF does not say its coordinate system, so one is to be given; without it the drawing is read all the same, to be looked at and written, with a warning that suggests one from the coordinates (SWEREF 99 TM, a local zone of SWEREF 99 or RT 90). **GeoParquet** (.parquet), without GDAL: geometry as WKB, its coordinate system from the metadata, compressed with Snappy, Zstandard, gzip, LZ4 or Brotli. **City models** as CityGML 1.0, 2.0 and 3.0 (.gml, .xml) and CityJSON 1.x and 2.0 (.city.json, .json, also CityJSON Sequences, .jsonl), without GDAL: a feature for each city object (building, building part, bridge, tunnel, road, water, vegetation, terrain, furniture…) with its id, type, parent, level of detail, lowest and highest height and height, and its attributes; the outline as the area and the surfaces as a 3D model, coloured by what they are (roofs, walls, ground, windows, doors). *Level of detail* chooses the finest of each object, the coarsest, or LoD 0, 1, 2 or 3. Implicit geometries and templates are placed, and the coordinate system is taken from the file; a city model that does not say one, as the small examples of cityjson.org, is read without one, with a warning and a guess from its coordinates. Buildings from **IFC** (IFC2x3, IFC4, IFC4.3): an element a feature with GlobalId, class, name, description, object type, tag, storey, type and material, and every property set and quantity as an attribute (Pset_WallCommon.IsExternal), and the model for 3D: extruded profiles (rectangles, circles, I, L, U and T sections, polylines and arcs, with holes), faceted breps, advanced breps (faces on planes, cylinders, cones, spheres, tori and B-spline surfaces, also rational, bounded by lines, circles, ellipses and B-spline curves, as Revit and other CAD write curved shapes), surface models, triangulated and polygonal face sets, mapped items, also mirrored and scaled, booleans that take away a half space (also bounded by a polygon, as a roof cuts a gable wall) or another solid, closed where they cut, with the colours of their styles or materials and glass for windows; openings cut out of walls and slabs, also openings that are not boxes, with the reveals in the hole, and taken through their wall when they stop short of its face by up to 10 mm, as openings exported from Revit often do; placed by IfcMapConversion or ePSet_MapConversion in their coordinate system, by the latitude and longitude of the site, or by the origin given. 3D models as OBJ files, with their MTL materials and textures beside them, and as glTF 2.0: .gltf with its buffers and pictures beside it or inside it, or .glb. PNG and JPG with a world file are read as images. OpenStreetMap as .osm.pbf or .osm, a layer at a time (points, lines, multilinestrings, multipolygons, other_relations), with each common tag as an attribute of its own (*Tags as attributes*: the common ones, those on at least one feature in a hundred, at most 100; all; or none). Only the chosen layer is held while the file is read, so a layer such as multilinestrings can be read from a whole country Pictures or grids from a folder of tiles become one. One file, chosen layers of a file, or all files in a folder that match a filter. Can add the file name and the layer name as attributes, and can stop at a number of features. *PostGIS…* beside *File…* and *Folder…* reads from a database instead (below). Web services are read page by page, and only within a rectangle when one is set: OGC API Features (an address with /collections), WFS and ArcGIS FeatureServer or MapServer layers; the program lists their layers. Statistics in JSON-stat, as from Eurostat, become a row per value. An API connection gives the address its key or login |
| Input from PostGIS | `PostgisReader` | Reads tables from PostgreSQL with PostGIS or from Microsoft SQL Server, with their geometry, or what an SQL query gives. A connection of the kind *SQL Server* in the settings goes through ODBC (ODBC Driver 17 or 18), with a user and password or the login of Windows. *PostGIS…* on an Input opens a window where a database connection from the settings is chosen; its tables with geometry are listed at once, and can be searched and ticked, one or many. *Only rows where* sends a condition along, so that the database sends only those rows. Several tables become one stream, with the name of each in the attribute table. *Read a file instead* makes it an Input of files again |
| Output | `Writer` | Writes GeoPackage, Shapefile, GeoJSON, FlatGeobuf (with a spatial index), Excel, LibreOffice Calc, CSV and txt, and images as PNG, JPG, GeoTIFF and PDF, and the 3D model a stream carries as OBJ (with its .mtl and textures) or glTF (.glb). A 3D model is written around an origin near its middle, kept in the file, and is put back in its place when Ographis reads it again; in glTF each object is a node with its attributes as extras. A grid of values, such as from Raster calculator, is written as a GeoTIFF of 32-bit decimal numbers with -9999 where it is empty. A PDF is one page, A0 to A5, Letter, Legal, Tabloid, a size of your own in millimetres or as large as the picture at the pixels to the inch you give, upright or turned, with a margin and a title if you want one, at the left, in the middle or at the right, and with coordinates that GIS programs can read. The picture is packed lossless or as JPEG with a quality, and the log says how large it is printed and at how many pixels to the inch, with a warning and the width to draw it in when that is too coarse for paper. A GeoPackage gets a **primary key**: a new column of numbers by the name you give (fid from the start), or an attribute whose values are all different; whole numbers are the key itself, other values keep fid as the key and get an index that allows each value once. The format is chosen from a list or follows the file name. Can write an attribute as another type, cut text at a length and round to a number of decimals. A workbook gets a font, a bold and a locked top row if you want them (both are off from the start), colours and columns as wide as their contents. Two more formats are in the list: *GeoSpatial PDF with layers* and *PostGIS table* (below) |
| Make zip file | `ZipWriter` | Packs what the outputs wrote in the run into one zip file, all of them or those named, a Shapefile with its .shx, .dbf and .prj, together with other files and folders you list, such as a README or styles. In their folders as under the folder of the model, or all at the top. Runs last and needs no connection |
| View | `Viewer` | A place to look at data: connect as many streams as you like to it, from anywhere in the model. Click it, and each connection is shown under the canvas by its number and name in the table, while the map and the 3D view show them all together, such as a model on its terrain; streams in another coordinate system are reprojected for it. A tick box before each stream shows it or hides it on the map and in 3D, and the View keeps the choice. Writes nothing |
| Run model | `ModelRunner` | Runs another model, so that work can be split into models of their own, each easy to follow, and run as a chain. It waits until the other model has run, and what is connected after it runs after that; or it starts the other model by itself with `og` and goes on, with its log in a file beside it. Variables can be given to it, also from this model's own: `year=$(year)`. Connect a stream to *After* to run it after that stream. The lines of the other model come in this log, a step in and under its name, and *Result* has a row for each of its nodes: status, features, files and seconds. A model that would run itself, also by a detour, is stopped |
| Read files named in an attribute | `AttributeFileReader` | Reads the file whose path is in an attribute, for each feature: such as the answers HTTP request keeps as files, a GeoTIFF of travel times from OpenTripPlanner or GeoJSON. The features of each file get the attributes of its row; grids are put together into one |
| GeoSpatial PDF | `LayeredPdfWriter` | The format *GeoSpatial PDF with layers* of Output. Writes the streams connected to *Layers* as a geospatial PDF where each is a layer that can be turned on and off in the PDF reader. The order and the look of the layers are set as in Draw map; a layer that is not ticked is in the PDF but off when it opens. Features keep their attributes, which the reader can show. Pictures, such as shaded terrain, can be layers. Page size, margin and a title as for PDF |
| Slides | `SlideWriter` | The formats *PowerPoint slides* and *LibreOffice Impress slides* of Output. Writes the pictures connected to *Pictures*, from Draw map, Draw chart, Render 3D scene and others, as a .pptx or an .odp file: a slide for each, as large as fits, with the name of the tool it comes from as its title. A first slide with a title and a line under it, wide (16:9) or standard (4:3) slides, and a background colour |
| Output to PostGIS | `PostgisWriter` | The format *PostGIS table* of Output, also to SQL Server, where the table gets a geometry column with a spatial index and the key chosen as its primary key (a key that is not a whole number gets a unique index instead). A window as for the input chooses the connection, and the schema and table, new or one that is there. The form sets what happens to a table that is there (replace or add to it), the geometry type (as the features are, or Point, MultiPoint, LineString, MultiLineString, Polygon, MultiPolygon, or no geometry), the coordinate system to reproject to, the name of the geometry column, and the **primary key** of a new table: a new column of numbers by the name you give (ogc_fid from the start), or an attribute whose values are all different and none empty, whole numbers or text. Names are written in lower case with spaces as `_`. The table gets a spatial index, and is written in one transaction: if something fails, nothing is written |

Attributes:

| Tool | In files | Does |
| --- | --- | --- |
| Calculate statistics | `StatisticsCalculator` | Count, sum, min, max, mean, median and the percentiles you ask for, such as 10 and 90, for the whole stream or per group |
| Check attributes | `AttributeValidator` | Checks the values of attributes, a row for each: that it has a value, its type (whole number, number, text, date), its longest length, its smallest and largest value, and a list of values it may have. Passed, or Failed with what is wrong in an attribute |
| Count | `Counter` | Numbers the features in the order they come, from 1 or a number you choose, in an attribute. Can start again for each group |
| Random number | `RandomNumber` | Gives each feature a random number between a smallest and a largest, with the decimals you choose; with no decimals, a whole number that can be either end. The same seed gives the same numbers |
| Reproject coordinate attributes | `CoordinateReprojector` | Reprojects an x and a y kept in attributes, such as columns of a spreadsheet, from one coordinate system to another, into new attributes. The geometry is left as it is |
| Classify by range | `RangeMapper` | Gives each feature a value by the range its number falls in: 1–100 gives A, 101–200 gives B, in an attribute you name. The ranges are given in rows of from, to and value, or made from the data as equal intervals or with as many features in each. A value in no range gets a text you choose, or nothing |
| Map values | `ValueMapper` | Looks up each value of an attribute in a list and writes what it becomes, such as 1 to Health centre, in the attribute or a new one. *Use the values in the data* fills in the list after a run. Values not in the list are kept, left empty or given a text. A result of only numbers is a number |
| Concatenate attributes | `StringConcatenator` | Joins several attributes into one, with a separator if you want one |
| Create attribute | `AttributeCreator` | Calculates an attribute with an expression, as the field calculator of QGIS. Under the expression stand the attributes to click in, the value it gives for the first feature, and a list of the functions and variables with a line of help each |
| Extract substring | `SubstringExtractor` | Takes the first or last characters of a text, or a number of characters from a position |
| Filter by value | `ValueFilter` | An output for each value of an attribute that you list, for example A, B and C, and Other for the rest |
| Join by attribute | `FeatureMerger` | Joins two streams on one or more attributes, as a join in SQL: **inner**, only those that match; **left**, all of Requestor; or **full**, all of both. The **first** match or **every** match, one to many. Supplier's attributes can be given a prefix, so that those with the same names as Requestor's come along too |
| Keep attributes | `AttributeKeeper` | Keeps the attributes you choose and removes all others |
| Manage attributes | `AttributeManager` | Every attribute in a list with Keep, Rename or Remove, in an order you choose, and new attributes with a value or an expression |
| Remove duplicates | `DuplicateFilter` | Sends the first feature with each key to Unique, repeats to Duplicate. The key is attributes, the **geometry** too, or the geometry alone, with the coordinates rounded to some decimals: points in the same place, lines drawn twice |
| Replace text | `StringReplacer` | Replaces a text with another: every A becomes B, or every space becomes nothing |
| Change case | `CaseChanger` | Makes the text of chosen attributes UPPER CASE, lower case, First letter capital or Every Word Capital, with Dals-Ed counted as two words. In expressions: `upper()`, `lower()` and `initcap()` |
| Set height | `HeightSetter` | Gives every coordinate a height (Z): from an attribute or an expression, such as the height of contour lines from Contour lines; from a terrain model on Terrain, such as roads laid on the ground with a value above it; or the heights they have and a value more. Can make them flat again |
| Round numbers | `Rounder` | Rounds decimal numbers in one or more attributes to a number of decimals |
| Sort | `Sorter` | Sorts by one or more attributes, one after the other, each ascending or descending |
| Take first | `Sampler` | Takes the first features that come, such as 5, or the first of each group. The rest go to Rest |
| Split attribute | `AttributeSplitter` | Splits a text at a character such as `,` or `$`. The parts become attributes of their own, or one feature each |
| Stamp date and time | `DateTimeStamper` | Adds the date, the time, or both, of the run as an attribute |
| Test | `Tester` | Splits the stream by a test: Passed and Failed. Filter by using only Passed |

JSON:

| Tool | In files | Does |
| --- | --- | --- |
| Flatten JSON | `JsonFlattener` | Makes the JSON in an attribute into attributes of their own, so that the stream is a plain table: `{"adress": {"ort": "Borås"}}` gives `adress.ort`. Lists are kept as JSON text, or get an attribute per element. A prefix can be put before the names. Features whose attribute is not JSON go to *Failed* |
| Explode JSON list | `JsonExploder` | A feature for each element of a list in JSON, at a path such as `mottagningar`, with the attributes of the feature it came from and the values of the element, numbered if you want. Several lists of the same length, separated by commas, are taken side by side, as the time series of SMHI and Open-Meteo: `hourly.time, hourly.temperature_2m`. Features without a list go to *Other* |
| JSON-stat to table | `JsonStatReader` | Statistics in JSON-stat, as SCB (PxWeb), Eurostat, Folkhälsomyndigheten and other statistics agencies give them, as a table: a row for each value with the code and the name of each category, such as region, sex and year, or with the categories of one dimension as columns, such as a column per year. Version 2.0 and the older bundles of 1.x. Codes such as 0114 stay text |
| Extract JSON values | `JsonExtractor` | Takes values out of JSON by their path, such as `adress.ort` or `kontakter[0].telefon`, into attributes. After a run the paths in the data are listed in the form to click |
| Build JSON | `JsonBuilder` | Writes attributes as a JSON object in an attribute: all or the chosen ones, names with a dot as nested objects, and the geometry as GeoJSON if you want it |

Web:

| Tool | In files | Does |
| --- | --- | --- |
| HTTP request | `HttpCaller` | Sends a request for each feature, to a fixed address or one built from its attributes, and puts the answer and its status code in attributes. GET, or POST, PUT, PATCH and DELETE with a body, such as JSON from Build JSON, and its content type. Follows answers in pages: a link to the next page in the answer (such as `next_page` or `links[rel=next].href`) or in the Link header, an offset, or a page number; each page becomes a feature with `_page`. Uses an API connection for keys, logins and OAuth2 tokens, with `{{key}}` where the key goes in the address or body. Keeps a pace, such as one request a second for Nominatim, and asks again when a service is busy (429) or briefly away (502–504), as long as it says to wait. Those that get no good answer go to Failed. Several requests at once, such as 8 to a service of your own like OpenTripPlanner, and the answers kept in a folder `http-cache` beside the model, so that a second run does not ask again. An answer can be kept as a file, such as a GeoTIFF, with its path in `_file` |
| OpenStreetMap download | `OsmDownloader` | Fetches buildings, roads, water, land use, places, railways, or what an Overpass filter of your own asks for, from an Overpass server, within a rectangle given or taken from what comes to Extent. A rectangle larger than allowed, 25 km² by default, is made smaller around its centre, with a warning. Areas are put together from ways and relations, and the common tags become attributes |
| STAC search | `StacSearch` | Finds satellite images, aerial photos and height models in a STAC catalogue, such as those of Lantmäteriet, Copernicus or Element 84: in the area of the stream on *Area* or a rectangle in degrees, at a time, in chosen collections, with a filter such as `eo:cloud_cover < 20`. Each item becomes its outline with its date, cloud cover and an attribute with the address of each of its files |
| Run SQL | `SqlExecutor` | Runs SQL against PostGIS: once, when nothing is connected to Input, or once for each feature, with `{attribute}` in the SQL replaced by its value. What the database sends back comes out on Result, with geometry when there is any, for the rest of the model. UPDATE and other statements that send nothing back can be run too. The features whose SQL ran go to Sent, the others to Failed with the reason |
| Shortest route | `ShortestRoute` | Routes along a network of lines, such as the roads of NVDB from Lastkajen, with Dijkstra's algorithm: from each feature on From to the nearest on To, or to every one. Lines are joined where their ends meet, not where they only cross, as at a bridge. Points are put on the nearest place of the nearest line, within a distance you set. Shortest by length, or quickest by a speed attribute, with its own speed against the direction of the line. One-way roads by a condition for each direction on the attributes of the network. The names of the attributes for speed and direction differ between exports of NVDB, so they are written in the form, not built in. The nearest is found for all at once, with one search backwards from every destination. The routes are lines with the attributes of both ends, `length_m`, `minutes` and how far each end lay from the road; those with no route go to Unreached |

Geometry:

| Tool | In files | Does |
| --- | --- | --- |
| Aggregate | `Aggregator` | Joins features into multi-geometries, all into one or one per group, with a count if you want one |
| Bounding box of all | `BoundingBoxAccumulator` | One rectangle around all the features, or one per group, with a count and the corners as attributes if you want them |
| Buffer | `Bufferer` | The area within a distance of each geometry. The distance can be an expression |
| Round shapes | `ShapeRounder` | Areas rounded as in a drawing for children or a game map: every corner a curve of the radius, narrow spits and points cut away and narrow gaps and inlets filled, and parts and holes smaller than a size you give removed. Lines and points pass as they are |
| Calculate area | `AreaCalculator` | Area in m², hectares or km², rounded to a number of decimals if you set one |
| Calculate length | `LengthCalculator` | Length or perimeter in m or km, rounded to a number of decimals if you set one |
| Clip | `Clipper` | Clips points, lines and areas against areas: Inside and Outside |
| Concave hull | `ConcaveHuller` | An area that follows a spread of points, or the corners of lines and areas, inwards where they leave room, around each feature, all of them or each group. *Concavity* says how tightly: 0.5 close, 2 rough |
| Convex hull | `ConvexHuller` | The smallest convex area around each feature, all of them or each group, with a count. Points in a row give a line |
| Create features | `FeatureCreator` | Points, lines or areas from coordinates you write, a feature on each line (`319500 6399700, 320100 6400200`), or from WKT and GeoJSON, in a coordinate system you choose. An area closes itself. Each gets a number and a name if you give them. A place to start from, a study area to clip by, or points to test a model with |
| Geometry from text | `GeometryFromText` | Makes geometry of text in an attribute, as web services give it: WKT (also with `SRID=…;` in front and with Z), GeoJSON and Esri JSON. The coordinate system is the one chosen, or the one the text names, or WGS 84 for GeoJSON. Text that cannot be read goes to *Failed*. Also Google's encoded polyline, as OpenTripPlanner, OSRM and Valhalla give routes |
| Geometry to text | `GeometryToText` | The geometry of each feature as text in an attribute: WKT, EWKT with SRID= in front, or GeoJSON, with heights (Z) where it has them and the decimals you want. The geometry is kept or dropped |
| Points to lines | `PointsToLines` | Joins points into lines, one for each value of an attribute, in the order of another, as the shapes of GTFS, a GPS track or a way from Overpass. Can close each line into an area. Heights go along |
| Create points from x and y | `PointCreator` | Makes a point of each feature from two attributes with x and y, in the coordinate system you choose. Only 2D. Features whose values are not numbers go to Failed |
| Count points in polygons | `PointCounter` | Each polygon gets the number of points that lie in it, as an attribute you name |
| Deaggregate | `Deaggregator` | Splits multi-geometries into their parts, with a part number if you want one |
| Dissolve | `Dissolver` | Merges areas into one per group |
| Check geometry | `GeometryValidator` | Finds geometries that are not valid, such as an area whose boundary crosses itself, and says what is wrong. Repairs them with GDAL if you want: Valid, Repaired and Invalid |
| Create grid | `GridCreator` | Squares or hexagons of a size over what comes in on *Extent*: every cell in its extent, or the ones that touch it. Each cell has a number, a row and a column. The grid starts at whole multiples of the size, so grids from different data fit together |
| Extract coordinates from point | `CoordinateExtractor` | Puts x and y of each point in attributes, rounded if you want. Lines and areas give a point inside them, their centre or their first point |
| Extract bounds | `BoundsExtractor` | Puts the smallest and largest x and y of each geometry in attributes, xmin, ymin, xmax and ymax with a prefix if you want, and zmin and zmax of the heights |
| First and last point | `LineEndsExtractor` | Makes a point of where each line begins, on First, and where it ends, on Last, with the attributes of the line. What is not a line goes to Other |
| Extend lines | `LineExtender` | Makes lines longer at one or both ends, straight on from the end, by a distance such as 1 metre |
| Every pair | `PairMaker` | Pairs every feature of *From* with every one of *To*, or with those within a distance: the origins and destinations of a matrix of travel times. Each pair has the attributes of both, with a prefix for those of To, the coordinates of both in WGS 84 for a route planner, and a line between them. A feature is not paired with itself |
| Draw map | `MapRenderer` | Draws the streams connected to *Layers* into a picture with coordinates. Each layer has a fill and a line colour, a line width and a point size, and the order is set in the form. The width in pixels, a margin, the background and the coordinate system are set too; the height follows from the area drawn. The result is one feature, the outline of the picture, that carries the image. Under each layer: **Values** (size and colour by an attribute, order, clusters), **Labels** and **Icons**. Labels: the text of an attribute or an expression, beside points (where there is room around them, in the order mapmakers like), along lines and turned with them, in the middle of areas, or at x and y from two attributes; all of them, or only those that do not overlap one another or reach outside the map, with the largest values of an attribute placed first; a size, also growing with an attribute; bold, capitals, a colour and a halo. Icons: the map icons of Maki (Mapbox, CC0), about 200, such as hospital, doctor, police, charging-station, lodging, bus, school; one icon for the whole layer, or one for each point from an attribute such as amenity of OpenStreetMap (doctors, charging_station, hotel) or Swedish words (vårdcentral, apotek, laddplats, hotell); in the fill colour, or white on a circle of the fill colour. The **legend** shows the icons, one row for each value, and the colours and sizes of the values |
| Find nearest | `NeighborFinder` | For each feature in *Base*, the nearest feature in *Candidates*, at any distance or within a largest one: the distance, and its attributes with a prefix if you want one. Works for points, lines and areas, measured between the nearest points of the two |
| Filter 2D/3D | `DimensionFilter` | Features with a height for each coordinate, and the objects of a 3D model with their part of it, to *3D*; flat ones to *2D*; none to *No geometry* |
| Filter by geometry type | `GeometryFilter` | Sends points, lines and areas to an output each, the cells of a raster to Raster, and features without geometry to Other. The points of a point cloud go to Point; to take ground, buildings or vegetation out of a cloud, use *Filter by value* or *Test* on `classification` |
| Geometry to 2D | `Force2D` | Makes the geometry two-dimensional: drops the heights (Z) and measures, and a 3D model the stream carries, so that the map and the 3D view show it flat |
| Generalize | `Generalizer` | Simplifies lines and boundaries with a tolerance (Douglas–Peucker). *Keep neighbours together* simplifies a shared boundary once, so that no gaps or overlaps arise |
| Intersect | `Intersector` | Cuts points, lines and areas by the areas of *Overlay*. Each part that lies in an area gets the attributes of both and goes to *Intersection*: a road cut where it goes from one zone to the next. Where the areas overlap a part comes for each. What lies outside them all goes to *Outside* |
| Join by location | `SpatialJoiner` | Each feature gets the attributes of the features in Join that it touches or overlaps, lies within, contains, crosses, only touches, overlaps, or is within a distance of, or that its point lies in, as before 0.77. Points, lines and areas in both: each road the zones it crosses, each stop the road it is on. The first one found, every one as a feature of its own, or summarized: the count, numbers added up, averaged, smallest or largest, and text listed. All attributes or the ones you choose, with a prefix if you want one. Features that find nothing go to Unjoined |
| Move by x, y and z | `Offsetter` | Moves the geometry a distance along x and along y, in the units of the stream's coordinate system, and up or down: the heights of features with Z, 3D models and grids of heights. A map from Draw map moves with it |
| Lines to polygons | `LineToPolygon` | Turns closed lines into areas. Parts of a line that meet end to end, within a distance you can set, are joined into rings first. Lines that do not close go to Failed |
| Polygons to lines | `PolygonToLine` | Turns each area into its boundary as lines, holes included. What is not an area goes to Other |
| Overlay areas | `AreaOnAreaOverlayer` | Lays *Overlay* over *Areas*. Where they overlap the part has the attributes of both and goes to *Both*; what is left of each goes to *Areas only* and *Overlay only* |
| Reverse geometry | `LineReverser` | Turns the direction of lines, so that they start where they ended, with their heights, and the outlines of areas, which then go the other way round. The parts of a multi geometry come in the opposite order. Areas can be left as they are |
| Rotate | `Rotator` | Turns the geometry by an angle in degrees, counter-clockwise, around the middle of each feature, the middle of all or a point you give. The angle can be an attribute or an expression. A 3D model turns with its features |
| Remove geometry | `GeometryRemover` | Takes away the geometry, so that the stream is a plain table |
| Replace with bounding box | `BoundingBoxReplacer` | Replaces each geometry with the rectangle around it, with the corners as attributes if you want them |
| Replace with center point | `CenterPointReplacer` | Replaces a geometry with its centre of mass, a point inside it, or the centre of its bounding box |
| Reproject | `Reprojector` | Reprojects to another coordinate system |
| Split lines | `LineSplitter` | Splits lines at the points connected to *Points*, within a tolerance, where the lines cross or meet each other, or both. Each part keeps the attributes of its line, with a part number if you want one |
| Thiessen polygons | `VoronoiDiagrammer` | Gives each point the area that is nearer to it than to any other point, with the attributes of the point. The areas are cut by the polygons connected to *Boundary*, or by a box around the points with a margin |
| Scale | `Scaler` | Makes the geometry larger or smaller by a factor, the same in x and y or one each, around the origin, the middle of each feature or the middle of all. 0.001 turns millimetres into metres. Heights and 3D models are scaled too, from 0 or from the lowest point, unless the heights are to be kept or the factors differ; Move by x, y and z moves a 3D model along as well |
| Snap | `Snapper` | Moves the points of the features within a distance of the features on *Fixed*, which stay where they are: to their nearest vertex, or to the nearest place on their lines and edges. Without Fixed, the features snap to each other. Points, lines and areas |
| Split lines into segments | `LineExploder` | A line from each corner to the next, with the attributes of the line and which segment it is. The outlines of areas too if you want |
| Swap x and y | `XySwapper` | Swaps x and y of all geometry, where they have been mixed up: latitude as x, or northing and easting in the wrong order |
| Set coordinate system | `CrsSetter` | Says which coordinate system the stream is in, as an EPSG code such as EPSG:3006, without changing the coordinates |

Charts:

| Tool | In files | Does |
| --- | --- | --- |
| Draw chart | `ChartDrawer` | A bar chart as a picture. The names under columns can be turned 45° or 90°, so that many fit, such as 49 municipalities: a bar for each name, as long as its value, such as the area of each municipality, or the number of features with each name. Sorted largest or smallest first, alphabetically (å, ä and ö after z) or as they come, and the first ten or so if you want. A title, an explanation under it and a note at the foot, such as the source. Numbers as 1 234,5 or 1,234.5 with a unit, on every bar, on the largest and smallest or none, and a line at the mean. Bars across for many or long names, columns for a few. One colour, and a second for the bars you point out. Text in Inter, which is built in. Write it with Output as PNG or PDF Also written as an **interactive web page** (.html) of its own, with nothing to fetch: the value of a bar under the pointer, buttons to sort by size or by name, light or dark as the reader's computer is set |

Raster:

| Tool | In files | Does |
| --- | --- | --- |
| Merge rasters | `RasterMosaicker` | Puts the pictures or grids connected to *Rasters* together into one. The one connected first is on top. Others are reprojected to the coordinate system of the first |
| Generalize raster | `RasterGeneralizer` | Makes a raster coarser: a number of cells across and down become one, with the mean, the most common value for classes such as land cover, the median, the smallest, the largest or the sum. Pictures get the mean colour |
| Raster calculator | `RasterCalculator` | Counts cell by cell with the rasters connected to *Rasters*: A is the first, B the second, A_2 band 2 of A. Any expression: `A - B`, `(A - B) / A * 100`, `if(A > 100, B, null)`, `max(A, B, C)`, `CASE WHEN … END`. The result has the cells of A, where they all overlap or over the whole of A; the others are reprojected and read at the middle of each cell. An empty cell makes the result empty, or counts as 0. Written as a GeoTIFF of decimal numbers |

3D and terrain. *Render 3D scene* is the tool for pictures of buildings and other models, on terrain or not, and the slow, fine one; *Quick terrain view* shows terrain alone in seconds. *Shade terrain*, *Tanaka contours* and *Contour lines* make maps seen from above, with coordinates:

| Tool | In files | Does |
| --- | --- | --- |
| Tanaka contours | `TanakaContours` | Illuminated contours, after Kitiro Tanaka: the contour lines of a terrain model, light on slopes that face the light and dark on those that face away, widest where they face it squarely, over bands of grey, land colours or blue between the heights. The heights can be evened out first, for calm lines over flat land. A picture with coordinates, and the pieces of line with their height and light |
| Render 3D scene | `SceneRenderer` | Pictures of 3D models, such as buildings read from OBJ files, glTF or 3D Tiles, or made with Extrude or Generate TIN, by following rays of light: lit by the sun, with soft shadows, and by the sky, which reaches into the shade and darkens corners, with their materials, textures and glass. On a light floor, under the sky as it is at that hour (blue at noon, golden at sunset, Preetham's model), or on the terrain connected to *Terrain* with the map on *Map* laid over it. A TIN from Generate TIN on *Terrain* is drawn as it is, also with nothing on *Models*. **Clouds** in % dim the sun, soften the shadows and are seen in the sky; **haze** lets far things fade into the horizon. **Water**: a surface at a height, or the areas on *Water*, with waves, that mirrors the sky and what stands by it and glitters in the sun. **Reflections** on smooth and metallic surfaces, and the light **bounced** once, twice or three times. The models **white** as an architect's model or in **light wood**, with glass kept, and materials named as glass, window or fönster made glass. The sun stands where you say or where it does on a date and at a time, and a **sun study** gives a sheet of pictures, a row for each date and one for each time, each also as PNG in a folder if you want. **At dusk**: windows lit from inside, a share of them at random, both panes of glass and rows of windows drawn on plain walls (a floor every 3 m, as on blocks from Extrude); **lamps** at the points on *Lights*, at their own height or some metres above the ground, with a strength and a colour, that light what is around them and are seen as bright points; materials that **glow**, by name or by the emission in their file. **Depth of field**: what is nearer or further than the focus is blurred, as with a model photographed close up. **Parallel projection**, as an isometric drawing or a game map (camera height 35 for isometric), and the models without a **floor**, so that on a see-through background they float as a diorama. A **background colour** of your own, a **title** at the top, and the **light**: neutral, warm sun and cool shade, golden, or soft and dreamy. Large scenes, such as a diorama of an island, have their corners darkened at their own scale. As JPEG the picture is laid on white, or on the background colour. A picture can take some minutes; the hints say what costs time |
| Contour lines | `ContourGenerator` | Contour lines from the heights on *Heights*, at an interval from a start, with the height in an attribute, or the areas between them |
| Shade terrain | `TerrainShader` | Shaded relief seen from above, from a terrain model read as a grid and connected to *Heights*. A sun from a direction and a height lights it, the terrain casts soft or sharp shadows, valleys get less light from the sky, the colours follow the height, and a haze can lie in the low parts. A map from *Draw map* connected to *Map* is laid on the terrain and shaded with it. The heights can be made higher to stand out. The picture has coordinates, for PNG, GeoTIFF or PDF |
| Quick terrain view | `TerrainRenderer` | Terrain alone seen from the side in seconds, as from a camera: from a direction and a height, with a lens angle and a zoom. The light, the shadows, the colours and the map are as in *Shade terrain*, with fog that grows with the distance and a sky behind. The terrain can stand on a block, as in a block diagram. The picture has no coordinates. Called *Render 3D view* before 0.79 |
| Make diorama | `DioramaMaker` | Areas of land cover as a toy landscape, as in a picture book or a board game: a 3D model for Render 3D scene, the 3D view and 3D files. The land is rounded (Round shapes), stands as a slab on a **board** whose top is the water, with a **beach** of sand along the shore and sides like a cake, and the kinds of land lie on it as thin plates in **toy** or **pastel** colours. The colours come from words in the values of an attribute, in Swedish or English (skog, barr, åker, bebyggelse, sjö, hav, forest, field, water…), or as you give them, such as `Åker=#e0d77a`. The largest kind is the ground. Water is cut out of the land, except a sea drawn under it all. The board follows the shape of the land and the water, is a rounded rectangle, is **round**, or is left out so that the land floats. **Objects**, each with its shadow: trees in forest (pines in coniferous forest, round trees in other forest, a few in parks) and houses with bright roofs in built-up areas, as close as a share you give; **animals** from a list such as `sheep 8, pigs 4, 2 horses, dinosaur 1` (sheep, pigs, cows, horses, rabbits, dogs, cats, bears, elephants, giraffes and dinosaurs, in English or Swedish), each where it belongs: farm animals on fields and meadows, dogs and cats in town, bears in the forest, **sailing boats** on the water and **fluffy clouds** above, whose shadows fall on the land. The size of the objects goes with the area unless given, and another variation puts them elsewhere. Which kinds get pines, round trees, houses and grazing animals can also be given by their values, for land cover named otherwise; with no forest found, the node says which kinds there are. Sizes are taken from the extent unless given. With a terrain model on *Heights*, read with Input as a grid of heights (cells as heights), such as a GeoTIFF from Lantmäteriet in the same coordinate system as the land cover, the land rises in **terraces**, as layers of a cake, each rounded, with the kinds of land and the objects on them, as high as you want. The pieces come out as areas, and the objects as points, with layer, kind, colour, z_base and z_top |
| Extrude | `Extruder` | Raises areas into blocks: a 3D model with walls, a top and a bottom, with holes kept, for Render 3D scene, the 3D view and OBJ and glTF. The height is a number, such as 20, or an attribute or an expression such as `floors * 3`, above the bottom or as the height of the top. The blocks stand at a height you give, at the lowest height the areas have, or on the terrain on *Terrain* at its lowest point under each. Colours of the walls and the top, and names for the objects from an attribute. The areas come out with their attributes and z_base, z_top and height |
| Generate TIN | `TinGenerator` | A triangulated irregular network from a raster on *Heights* read as a grid of heights: it starts from the edge of the cells with a value and adds the cell furthest from the triangles, round after round, until every cell is within the tolerance in metres or there are as many corners as you allow, so there are many triangles where the terrain is rough and few where it is even. Triangles over cells without a value are left out. Points with heights, such as from a point cloud, are joined as they are. **3D**: the triangles with their heights and a 3D model, for Render 3D scene, the 3D view and OBJ and glTF. **2D**: flat triangles. Both with z_min, z_max, z_mean, slope and aspect |
| Raise points | `PointRaiser` | Points raised to a height, from an attribute or an expression, with a stick from where they stand: lollipops for a 3D map, such as the inhabitants of each town. The sticks start at 0, on the terrain on *Terrain*, or at the points' own heights. *Tops* gives the raised points and *Sticks* the vertical lines with a 3D model of a post and a ball on each, in colours and a size you give |
| TIN to raster | `TinRasterizer` | A raster of heights from a TIN in 3D or any 3D model: the height of the triangles at the middle of each cell, the highest where they lie over each other, as roofs over floors, and no value outside them. The cell size is given or a round number near a thousandth of the width |

`PointOnAreaOverlayer` from earlier versions did what *Join by location* and *Count points
in polygons* now do, in one tool. It still runs in models that have it, shown as
*Overlay points on areas*, but is no longer in the list.

Expressions follow QGIS: fields are written by name or in `"double quotes"`, text in
`'single quotes'`.

```
population >= 10000 AND municipality <> 'Mark'
round("area m2" / 10000, 1)
lpad(to_string(code), 4, '0') || ' ' || upper(name)
CASE WHEN height > 20 THEN 'high' WHEN height > 10 THEN 'middle' ELSE 'low' END
round(population / $area * 1000000, 1)
regexp_substr(address, '(\d{3} ?\d{2})')
date_add('$(departure)', 15 * ($id - 1), 'minutes')
```

- **Operators**: `+ - * /`, `//` whole division, `%` the rest, `^` a power (`-2 ^ 2` is −4),
  `||` joins text; `= <> < > <= >=`, `AND`, `OR`, `NOT`, `IS NULL`, `IN (…)`, `LIKE` and
  `ILIKE`; `CASE WHEN … THEN … ELSE … END`.
- **Variables** of the geometry: `$area`, `$length`, `$perimeter`, `$x`, `$y` and `$z` of a
  point, `$xmin`, `$ymin`, `$xmax`, `$ymax`, `$num_points`, and `$id`, the number of the
  feature in the stream from 1.
- **Numbers**: `round`, `abs`, `sqrt`, `floor`, `ceil`, `min`, `max`, `clamp`, `power`, `exp`,
  `ln`, `log10`, `log`, `sin`, `cos`, `tan`, `asin`, `acos`, `atan`, `atan2`, `pi`, `radians`,
  `degrees`.
- **Text**: `upper`, `lower`, `initcap`, `length`, `trim`, `substr`, `left`, `right`,
  `replace`, `lpad`, `rpad`, `concat`, `strpos`, `starts_with`, `ends_with`, and regular
  expressions with `regexp_replace`, `regexp_match` and `regexp_substr`.
- **Times**: `now`, `format_time`, `epoch`, `year`, `month`, `day`, `hour`, `minute`,
  `second`, `day_of_week` (1 Monday), `week` (as in Sweden), `date_add` and `date_diff`.
- **Others**: `if`, `coalesce`, `nullif`, `to_int`, `to_real`, `to_string`.

## The model format

A model is a JSON file. Paths start from the folder the file is in. `x` and `y` are the
position of the node on the canvas and do not affect the run.

```json
{
  "format": "ographis/1",
  "name": "Build test",
  "nodes": [
    { "id": "in", "type": "Reader", "name": "Subregions", "params": { "path": "subregions.gpkg" } },
    { "id": "sweref", "type": "Reprojector", "params": { "crs": "EPSG:3006" } },
    { "id": "out", "type": "Writer", "params": { "path": "out/subregions_sweref.shp" } }
  ],
  "edges": [
    { "from": ["in", "Output"], "to": ["sweref", "Input"] },
    { "from": ["sweref", "Reprojected"], "to": ["out", "Input"] }
  ]
}
```

A node can have a `comment`. Next to `nodes` and `edges` the file can hold `groups`, the
named boxes on the canvas, and `notes`, the free-standing notes. Neither affects the run.

```json
"groups": [{ "name": "Input", "x": 0, "y": 20, "w": 250, "h": 430, "color": "yellow", "locked": true }],
```

The colour is one of grey (empty), yellow, orange, red, pink, purple, blue, teal, green and
brown, or a colour of its own as `#8a2be2`.

```
"notes": [{ "text": "The stops are made up.", "x": 290, "y": 480 }]
```

A node with `"disabled": true` does not run. `navigator` holds the ids of the inputs and
outputs in the order they have been put in the navigator. An output can be told the format
and how to write single attributes:

```json
{ "id": "out", "type": "Writer", "params": { "path": "out/result", "format": "shp",
  "fields": { "name": { "length": 20 }, "code": { "type": "integer" }, "share": { "decimals": 2 } } } }
```

An input of a folder gives the folder as `path` and a filter as `pattern`. Several layers
are given as a list, and `*` stands for all of them.

```json
{ "id": "stops", "type": "Reader", "params": { "path": "stops", "pattern": "*.csv", "filename_attr": "file" } },
{ "id": "two", "type": "Reader", "params": { "path": "areas.gpkg", "layer": ["north", "south"] } },
{ "id": "web", "type": "Reader", "params": { "path": "https://example.org/open/roads.zip", "limit": "1000" } },
{ "id": "grid", "type": "Reader", "params": { "path": "heights.tif", "raster_as": "cells" } },
{ "id": "dem", "type": "Reader", "params": { "path": "terrain.tif", "raster_as": "grid", "cell_size": "10" } }
```

The look of a workbook, the rows of Manage attributes and the values of Filter by value:

```json
{ "id": "xl", "type": "Writer", "params": { "path": "out/towns.xlsx", "font": "Arial", "colors": "blue", "freeze": "false" } },
{ "id": "tidy", "type": "AttributeManager", "params": {
  "attributes": [{ "name": "pop", "action": "rename", "to": "population" }, { "name": "x", "action": "remove" }, { "name": "name" }],
  "others": "keep",
  "create": [{ "name": "county", "value": "'Västra Götaland'" }] } },
{ "id": "by_class", "type": "ValueFilter", "params": { "attribute": "class", "values": ["A", "B", "C"] } }
```

A connection from a Filter by value names the value as its port: `"from": ["by_class", "B"]`.
The order a user has given the ports of a node is kept among its parameters, as
`"_port_order": { "in": ["Clippee", "Clipper"] }`. The run does not read it.
Manage attributes from before 0.53, with `keep`, `remove` and `rename`, still runs, and is
turned into the list when it is opened in the program after a run.

Files with the earlier format name `flodesbanken/1` are still read, and are saved as
`ographis/1`.

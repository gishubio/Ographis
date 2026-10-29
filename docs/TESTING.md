# What is tested

`cargo test` runs 409 tests: expressions, CSV, geometry, whole models, reading and
writing real files, fetching from a web server on the same computer, and the user
interface. The interface tests run the program without a window, with simulated clicks,
drags and keys: load, run, pick a row, sort the table, drag a connection between two
ports, drag a tool onto a connection, click a connection and delete it, drag a selection
box, move, duplicate, disable and delete nodes, undo and redo, pan, zoom, drop files and
folders, choose layers, group and move a group, write a comment and a note, fold out
attributes, change a path and the order in the navigator, follow a run node by node, save,
the question about unsaved changes, fold the lists of tools out and in, drag everything
with its group boxes, open a model saved with the earlier tool names, take the values of
Filter by value from the data and see the connections follow, work the list in Manage
attributes, give a workbook a look, drop a GeoTIFF, open a model from the list of the
latest, add a tool through the menu of the canvas, move ports, draw 300,000 roads on the
map and zoom in to them, follow a large read in the log, page through the table, set
variables, pause and resume a run, run from and to a node with the cache, stop a run with Shift+F5, move and resize comment boxes, resize a group box from
every edge and corner, clear a search with its ×, order the layers of Draw map and see
the drawn map, see the run history and a run refused while the model is running, pick a building by a click in 3D, and see
the settings of a CAD drawing and a city model. Further tests add to every format, and send web requests to a server on the
same computer. They save screenshots in `target/screenshots`.

The tests run side by side. Once, in some thirty such runs during the work on 0.4, the
test program itself crashed at start in native code, before any test had finished. It has
not been reproduced, under a debugger or otherwise, and the cause is not known. The tests
now take turns with the graphics driver, which is where such a crash is most likely to
come from, but that is a guess. It has not happened since.

Beyond the tests, results from the engine have been compared with independent tools:

| What | Compared with | Difference |
| --- | --- | --- |
| Reprojection WGS 84 → SWEREF 99 TM and 12 00 | proj4js | under 1 mm |
| Clipping 50,000 lines against 20 areas | GEOS through `ogr2ogr -clipsrc` | same count, 0.1 mm over 254,000 km |
| Clipping 500,000 points | GEOS through `ogr2ogr -clipsrc` | same count |
| Area of areas in SWEREF 99 TM | GEOS through `ST_Area` | none |
| Buffer 2 km around five routes, dissolved by type | GEOS through `ST_Union(ST_Buffer(…))` | 0.001 % with 8 segments per quarter circle, 0.01 % with 30 |
| Points in areas, 39 towns in 4 subregions | GEOS through `ST_Contains` | same counts |
| A workbook written by the program: text, numbers, empty cells, a code with a leading zero | openpyxl | same values and types |
| A workbook written by openpyxl with three sheets, dates and an empty sheet | read by the program | same values. The empty sheet is not listed |
| Styled workbooks, .xlsx and .ods, written by the program | LibreOffice 24.2, opened and printed to PDF | colours, bold top row and widths as set |
| glTF written by Extrude and Generate TIN | Khronos glTF Validator 2 | no errors or warnings |
| IFC: the samples of buildingSMART (IFC4 Simple-Scene: architecture, structure, HVAC, bridge, road; ISO reference view; IFC2x3), and IfcOpenHouse in IFC2x3 and IFC4 | read and rendered, compared by eye with the pictures of the samples | all elements with a shape read; the wall of the reference view gets its opening and window |
| Primary keys in PostGIS: new column, whole numbers, text | PostgreSQL 16 with PostGIS, `pg_index` | the key columns asked for |
| IFC element by element: AC20-FZK-Haus (83 elements, walls cut by both slopes of the roof) and AC20-Institute-Var-2 (702 elements, 570 openings) of KIT, Trapelo Design Intent (3 294 elements, 3 million triangles, a floor finish with an opening of 150 corners) and Wellness center Sama (779 elements, doors a hair off their walls) from Revit, Tabel_Chairs (advanced breps with B-spline surfaces and tori, in a mapped item scaled ten times), and the Simple-Scene and ISO reference view samples of buildingSMART in IFC2x3 and IFC4 | IfcOpenShell 0.9, the box, area and volume of each element | within 2 cm, 3 % of the area and 3 % of the volume; annotations and spatial zones are not read. In Trapelo one kind of furniture differs by 4 % in area, from how arcs are made into triangles; in Wellness center IfcOpenShell leaves a skin of 0.6 mm over two doors that Ographis takes through |
| CityJSON: the samples of cjio (Delft, Rotterdam, Zürich, a building in several LoDs), and the blocks of the example written by `tools/make_format_examples.py` | read and rendered, compared by eye | all city objects, with their attributes and semantic surfaces |
| CityGML: Potsdam LoD1 (2.0), the LoD2 and LoD3 buildings and the railway (3.0) of citygml4j, FZK-Haus (3.0) and a part of Lyon (1.0) | read and rendered, compared by eye | all buildings in place, implicit geometries placed, LoD chosen as asked |
| DXF written by ezdxf: polylines, lines, texts and blocks with attributes on four layers | read by the program | layers, colours, line types, texts and the block attributes as written |
| GeoParquet written by pyarrow, uncompressed, Snappy and Zstandard, with and without a coordinate system | read by the program | same values, types and points |
| FlatGeobuf and SpatiaLite written by the program | read back, and by `ogrinfo` | same features and attributes |

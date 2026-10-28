# Examples and web APIs

The example models in `examples/`, the data they use, and the web services they talk to.

## Example data

`examples/municipalities.csv` holds the municipality codes of Västra Götaland county from
Statistics Sweden. The rest is made up in its shape: the positions and populations of the
towns are approximate, the subregions are schematic, the routes are straight lines
between towns, the bus stops in `examples/stops` are invented, and so are the heights in
`examples/heights.tif`.

| Model | Shows |
| --- | --- |
| `buildtest.ographis` | The smallest useful model: read, reproject, write |
| `skaraborg.ographis` | Clip, test, join, calculate, measure, and write to three formats |
| `overlay.ographis` | Join by location, count points in polygons, statistics per group, sort, buffer, dissolve and label points |
| `stops.ographis` | A folder of files read as one stream, the file name as an attribute, the text tools, a date stamp, Excel out, and groups, a comment and a note on the canvas |
| `lines.ographis` | Split attribute into one feature per line, Filter by value with an output per line, and styled sheets in Excel and LibreOffice |
| `grid.ographis` | A GeoTIFF read as squares, towns counted in each, and a styled Excel sheet |
| `village.ographis` | A made-up village in OpenStreetMap's format: areas, roads and places read from `village.osm.pbf`, the places checked and given coordinates, and all of it as a PDF with layers |
| `terrain.ographis` | A made-up terrain model with a table mountain, a river and a lake: shaded relief with haze written as GeoTIFF, and a 3D view from the south-west as PNG, both with the water from a GeoJSON drawn on them with Draw map, and contour lines every 10 m |
| `nearest_and_grid.ographis` | The nearest route for each town, the routes split where they cross, hexagons of 10 km with the number of towns in each, short names for the subregions with Map values, and a zone 5 km from the routes overlaid with the subregions |
| `thiessen.ographis` | The area nearest each town, cut by the subregions, numbered with Count, with the borders as lines and the extent of the subregions |
| `earthquakes.ographis` | The earthquakes of the last day from USGS on the countries of the world, sized by magnitude with the largest underneath, and a legend. Needs the internet |
| `tanaka.ographis` | Tanaka contours of the made-up terrain, in land colours as PNG and in grey as GeoTIFF |
| `contours_3d.ographis` | Contour lines of the made-up terrain at their heights, written with Z, and a View of them with the terrain. Click Look at both and tick 3D above the map |
| `house.ographis` | A house from an OBJ file with textures, and its neighbour, the same house as a glTF (.glb), placed on the made-up terrain, rendered in a light studio and on the terrain in the afternoon sun of the spring equinox, and the objects written with Z. An evening by the water on midsummer's day under a few clouds, and a sun study of the white houses at 9, 12 and 15 at the equinox, midsummer and midwinter |
| `extrude_tin.ographis` | Made-up building footprints (`blocks.geojson`) raised by their number of floors with Extrude onto the made-up terrain, a TIN of the terrain around them in 3D and in 2D with slope and aspect, the blocks on the TIN in the afternoon sun, the same at dusk with lit windows, street lamps from `lamps.geojson` and a shallow depth of field, the triangles as GeoPackage and the blocks as glTF |
| `labels.ographis` | Labels: the towns sized and placed first by population, the routes named along their lines, the subregions in capitals in their middle, and a legend. Made-up places in Skövde (`places.geojson`) with Maki icons from their kind, written in English as in OpenStreetMap or in Swedish, labelled, with a legend of the icons, as PNG and as an A3 PDF |
| `ifc.ographis` | A made-up house in IFC4 (`house/villa.ifc`, made by `tools/make_ifc_example.py`) with walls, openings, windows, a door, a floor slab and a gable roof, property sets and quantities, placed on the made-up terrain by its map conversion: its windows found with Test, its elements in a workbook, a picture in the afternoon sun and the model as glTF |
| `diorama.ographis` | Made-up islands off the coast (`islands.geojson`) with land cover named as in Lantmäteriet's and a made-up terrain model (`islands_heights.tif`) for the terraces, made into a diorama and rendered as a toy picture with parallel projection on a see-through background, as PNG, and as glTF for Blender |
| `lollipops.ographis` | The towns raised by the square root of their population with Raise points, as lollipops on the subregions made a floor with Extrude, rendered with parallel projection on a background colour, and the sticks written with their heights to GeoPackage |
| `tiles.ographis` | Three copies of the house as a 3D Tiles tileset (`tiles/`, made by `tools/make_tiles_example.py`), with a name and a year for each, read and placed on the made-up terrain, rendered in the afternoon sun and listed in a workbook |
| `json.ographis` | Open data from Västra Götalandsregionen read as JSON from the web: the hospitals as points from xkoord (north) and ykoord (east) in SWEREF 99 TM, written to GeoPackage, and their addresses as JSON with Build JSON. Needs the internet |
| `chart.ographis` | Two bar charts as PNG and as interactive web pages: the 15 largest towns with two pointed out, and the area of the subregions in km² with a line at the mean; both as slides for PowerPoint after a first slide with a title |
| `map.ographis` | Draw map with three layers, areas, routes and towns, written as PNG, GeoTIFF and an A4 PDF with a title |
| `geometry_tools.ographis` | The routes cut by subregion with Intersect and their length per subregion, the routes that cross each subregion summarized by Join by location, the towns' subregion and a concave hull around the towns of each, a convex hull around all, and the segments of the routes |
| `rasters.ographis` | The made-up terrain made coarser with Generalize raster, the terrain less the coarse one with Raster calculator, written as GeoTIFF, and height classes with the most common class in squares of 320 m |
| `formats.ographis` | The formats new in 0.81, from `examples/formats` (made by `tools/make_format_examples.py`): a made-up CAD drawing (`plan.dxf`) with buildings, roads, texts and wells as blocks with ID and depth, the wells picked out with Test by their block and written as FlatGeobuf; the towns as GeoParquet into a workbook; the blocks as a CityJSON city model in LoD2 with gable roofs on the made-up terrain in the afternoon sun; and the same blocks as CityGML in LoD1 written as glTF; and the subregions as vector tiles, from a folder of tiles at zoom 7, put together and reprojected to SWEREF 99 TM (also as `subregions.pmtiles`) |
| `apis/` | Models for common web APIs, all needing the internet: population from SCB (PxWeb, JSON-stat) and Eurostat, inhabitants per municipality from Kolada page by page, events on the roads from Trafikverket with the key in an API connection, a temperature forecast from Open-Meteo as a chart, lakes from an OGC API Features service, a layer of an ArcGIS FeatureServer, Sentinel-2 images over Göteborg from a STAC catalogue, addresses made into points with Nominatim at one request a second, unemployment in the NUTS 2 regions of Europe on a map from Eurostat and GISCO, the temperature at every SMHI station the latest hour, tomorrow's highest temperature in six towns from Open-Meteo as a chart, a key figure for the municipalities of Västra Götaland from Kolada as a chart, a city block as 3D Tiles from the samples of Cesium on GitHub, travel times between the towns from an OpenTripPlanner of your own (`otp_travel_times.ographis`, with its address in the variable otp), routes, departures and a count of departures between two stops from the REST API of OpenTripPlanner 2.4 (`otp_routes_and_departures.ographis`), and `chain.ographis`, a chain that runs three of them in turn with Run model. The services may have changed since they were written |

## Web APIs

- **API connections** are kept under *Edit → Settings → API connections*, like database
  connections: a key in a header or in the address, a key only where `{{key}}` is written
  (as Trafikverket wants it in the XML that is sent), a token, a user and a password, or an
  OAuth2 client with its id, secret and token address (as Lantmäteriet). The model names the
  connection; the key never goes into the model file, and is hidden in messages. A connection
  can be bound to addresses that begin with a given text, so that it is not sent elsewhere.
- **Times** from APIs are written in Swedish time with `format_time(time, format, zone)`:
  ISO 8601 with or without a zone, or seconds or milliseconds since 1970 as ArcGIS gives
  them. `epoch(time)` gives seconds to count with.
- **OpenTripPlanner** of your own: *Every pair* makes the origins and destinations with
  their coordinates in WGS 84, *HTTP request* sends the GraphQL query with POST, eight at a
  time and with the answers kept, *Extract JSON values* takes the duration and the route out,
  and *Geometry from text* makes the route a line from its encoded polyline. A travel time
  surface comes as a GeoTIFF with *The answer* as a file, read with *Read files named in an
  attribute*. `date_add` spreads the departures over an hour, and the percentiles of
  *Calculate statistics* sum them up. See `examples/apis/otp_travel_times.ographis`.
  The REST API of OpenTripPlanner 2.4 (`/otp/routers/default/…`) is used in
  `examples/apis/otp_routes_and_departures.ographis`: every stop as a point, a planned trip
  between two coordinates with each leg as a line, all departures of a stop in a day, and
  the departures from one stop that reach another between 09:00 and 14:00, counted. The
  stops, times and server are variables.
- **Coordinate systems**: every one of the more than 6 000 EPSG systems PROJ knows can be
  searched for by name, code or area under *Search all…* in a coordinate system choice, and
  is shown by its name.

# Examples and web APIs

The example models in `examples/`, the data they use, and the web services they talk to.

## Example data

`examples/municipalities.csv` holds the municipality codes of Västra Götaland county from
Statistics Sweden. The rest is made up in its shape: the positions and populations of the
towns are approximate, the subregions are schematic, the routes are straight lines
between towns, the bus stops in `examples/stops` are invented, and so are the heights in
`examples/heights.tif`. The small town in `examples/analysis` is made up from start to end:
streets, people in squares, districts, health centres, a walk with a GPS and wind.

| Model | Shows |
| --- | --- |
| `buildtest.ographis` | The smallest useful model: read, reproject, write |
| `skaraborg.ographis` | Clip, test, join, calculate, measure, and write to three formats |
| `changes.ographis` | Two versions of a list of hospitals compared by *Find changes*: what is new, what has changed and how, and what is gone, each as a layer of a GeoPackage |
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
| `vector_tools.ographis` | Overlaps and gaps between the subregions, the routes merged into a line each with a point every 5 km and moved 1 km to the side, and made-up populations by year turned wide with Pivot and long again with Unpivot |
| `profile_and_volume.ographis` | Four places written as text in different ways (`places_as_text.csv`) made into points, the fourth, in metres, to Failed; a walk between them with its profile over the example terrain every 20 m, as a chart and as a 3D line, and the earth above 100 m within 200 m of the walk, in cubic metres |
| `web_map.ographis` | The towns, routes and subregions as a web map, once with Leaflet and once with MapLibre GL: open `out/web_map/index.html` in a browser, click a town for its attributes and turn layers on and off in the legend |
| `share.ographis` | Ways to share what a model made: the made-up places in Skövde drawn on OpenStreetMap (made paler, with a legend; without the internet the map is drawn without it), a report as a web page with the map and a table of the places, a QGIS project with the places coloured by kind, sized by visitors and labelled, and the made-up blocks raised with Extrude and written as 3D Tiles for Cesium and as CityGML in LoD2 |
| `labels.ographis` | Labels: the towns sized and placed first by population, the routes named along their lines, the subregions in capitals in their middle, and a legend. Made-up places in Skövde (`places.geojson`) with Maki icons from their kind, written in English as in OpenStreetMap or in Swedish, labelled, with a legend of the icons, as PNG and as an A3 PDF |
| `ifc.ographis` | A made-up house in IFC4 (`house/villa.ifc`, made by `tools/make_ifc_example.py`) with walls, openings, windows, a door, a floor slab and a gable roof, property sets and quantities, placed on the made-up terrain by its map conversion: its windows found with Test, its elements in a workbook, a picture in the afternoon sun and the model as glTF |
| `hospital.ographis` | A made-up hospital in IFC4 (`hospital/hospital.ifc`, made by `tools/make_hospital_ifc.py`): a ground floor with emergency, entrance and imaging, and four floors of wards on a grid of columns, with brick exterior walls, partitions, doors to the corridor, windows in openings, 162 rooms as spaces with their departments, a stair, two lifts, beds, floor slabs and a flat roof with a parapet and a plant room, property sets, quantities, materials and colours, placed in Göteborg by its map conversion: its elements sorted in a workbook, a picture in the sun, and a floor plan of level 1 drawn from the walls as seen from a metre above the floor, written to a GeoPackage, and to PostGIS when that node is turned on |
| `blender.ographis` | The made-up terrain as a 3D model with its shaded relief draped over it and a block under it, and the blocks raised on it, as glTF for Blender: named by their id, grouped and given materials by their floors, with a sun on a midsummer evening and a camera, both files around the same origin so that they fit together |
| `diorama.ographis` | Made-up islands off the coast (`islands.geojson`) with land cover named as in Lantmäteriet's and a made-up terrain model (`islands_heights.tif`) for the terraces, made into a diorama and rendered as a toy picture with parallel projection on a see-through background, as PNG, and as glTF for Blender |
| `game_engine.ographis` | The same islands made ready for Unreal Engine: a heightmap of 16 bits in a size Unreal takes, a mask for each kind of land, and IMPORT.txt with the scale and place to set, in `out/game`. Change the engine to Unity or Godot |
| `buildings.ographis` | A made-up village drawn with the attributes of OpenStreetMap, made into buildings by *Make buildings*: houses with gable and hipped roofs in their colours, flats of brick, an office of glass, a church with its tower, a hall of sheet metal and a garage, on the made-up terrain. Rendered as a picture, and written as a walk-through in `out/walk_village` |
| `walk.ographis` | The made-up terrain and the blocks raised on it as a walk-through in the browser: open `out/walk_blocks/index.html`, click and walk along the street among the blocks, fly with F, and move the sun of midsummer |
| `lollipops.ographis` | The towns raised by the square root of their population with Raise points, as lollipops on the subregions made a floor with Extrude, rendered with parallel projection on a background colour, and the sticks written with their heights to GeoPackage |
| `tiles.ographis` | Three copies of the house as a 3D Tiles tileset (`tiles/`, made by `tools/make_tiles_example.py`), with a name and a year for each, read and placed on the made-up terrain, rendered in the afternoon sun and listed in a workbook |
| `json.ographis` | Open data from Västra Götalandsregionen read as JSON from the web: the hospitals as points from xkoord (north) and ykoord (east) in SWEREF 99 TM, written to GeoPackage, and their addresses as JSON with Build JSON. Needs the internet |
| `chart.ographis` | Two bar charts as PNG and as interactive web pages: the 15 largest towns with two pointed out, and the area of the subregions in km² with a line at the mean; both as slides for PowerPoint after a first slide with a title |
| `statistics.ographis` | The towns in four natural classes by size, a histogram of their sizes and box plots of them in each subregion, a heat map of the inhabitants per km² within 25 km, each subregion's share of the inhabitants and its rank on a dashboard web page with the map and the bars linked, and the growth of three municipalities since 2024 as an index, a small chart for each |
| `map_series.ographis` | A page for each subregion of Västra Götaland in one PDF: centred on it, named by it, the rest paler, the towns as circles by population, nested in the legend, and the land as dots, one for every 10 km²; the same pages also as a GIF, one after another |
| `cartography.ographis` | Commuting between the towns of Västra Götaland (made-up numbers, `commuting.csv`) as flows as wide as their number with an arrow head where they go, on the subregions named in two lines with their area, at 1:1 300 000 on A4 landscape, with a title, an inset of Göteborg, the towns over 50 000 in bold, lines broken under the labels and a grid of degrees, as PNG and PDF |
| `map.ographis` | Draw map with three layers, areas, routes and towns, written as PNG, GeoTIFF and an A4 PDF with a title |
| `geometry_tools.ographis` | The routes cut by subregion with Intersect and their length per subregion, the routes that cross each subregion summarized by Join by location, the towns' subregion and a concave hull around the towns of each, a convex hull around all, and the segments of the routes |
| `point_cloud.ographis` | A made-up point cloud as from a drone, without classes, in `pointcloud/`: the ground found with *Classify ground points*, a terrain and a surface model, the points above 2 m with their height above the ground as LAZ, and the five buildings in `footprints.geojson` measured, with the shapes of their roofs (two gables, a flat roof, a shed and a hipped roof, which is told as other) |
| `splats.ographis` | The made-up drone points made Gaussian splats, flat along roofs, walls and ground, written as SPZ for a viewer of splats and seen in 3D, and the footprints written as OpenUSD with a layer for each shape of roof |
| `raster_tools.ographis` | The raster tools of 0.89 on the made-up terrain: the slope in three classes made into areas, the mean and largest slope by each lake, a raster of the distance to water, what can be seen from two lookouts in `lookouts.csv` (a tower with the eye at 25 m), and the height of the ground at each lookout |
| `rasters.ographis` | The made-up terrain made coarser with Resample raster, cells into one, the terrain less the coarse one with Raster calculator, written as GeoTIFF, and height classes with the most common class in squares of 320 m |
| `formats.ographis` | The formats new in 0.81, from `examples/formats` (made by `tools/make_format_examples.py`): a made-up CAD drawing (`plan.dxf`) with buildings, roads, texts and wells as blocks with ID and depth, the wells picked out with Test by their block and written as FlatGeobuf; the towns as GeoParquet into a workbook; the blocks as a CityJSON city model in LoD2 with gable roofs on the made-up terrain in the afternoon sun; and the same blocks as CityGML in LoD1 written as glTF; and the subregions as vector tiles, from a folder of tiles at zoom 7, put together and reprojected to SWEREF 99 TM (also as `subregions.pmtiles`) |
| `proximity.ographis` | Proximity to care in a made-up town (`analysis/`, made by `tools/make_analysis_example.py`): the streets as a network walked at 5 km/h, the areas within 10, 20 and 30 minutes of the two health centres, the minutes from each square of people to the nearest, the people moved to districts with *Reallocate areas* and the share within 20 minutes in each, three health centres chosen among eight places with *Choose locations*, and a map |
| `analysis.ographis` | The analysis tools of 0.96 on the made-up town: hot spots of people and Moran's I, a walk with a GPS made into tracks and stops with the point where the GPS jumped left out, where it lingered as clusters, the wind drawn as arrows, the hollows of the made-up terrain and the streets in them, and the terrain sorted into kinds of land with *Classify image* |
| `cost_distance.ographis` | Minutes on foot across the made-up terrain, without roads, from the two health centres: the slope as a cost by Tobler's hiking function, *Cost distance*, lines and bands of 15 minutes, and a map |
| `print_map.ographis` | The made-up town as a map for print in A3: relief lit from several directions, roads with an edge and names that bend along them, districts as dash and dot with their names spread out, a scale bar, a north arrow, coordinates in the edge and the source, as a PDF of shapes with coordinates and a layer for each layer, and a PNG |
| `relief_map.ographis` | The made-up terrain as a map of heights in colours with relief lit from several sides, contours every 10 m with the index contours every 50 m heavier and named, in a frame with an index of six squares across and the scale in figures, the register of the names and their squares as CSV, and the same map draped over the terrain in 3D with mist in the valleys |
| `weather_3d.ographis` | The made-up village (`village_names.geojson`, `village_trees.geojson`) in 3D in five kinds of weather and light: falling snow with snow on the roofs and bare trees, a winter night with northern lights, lit windows and street lamps, a thunderstorm with rain and wet streets, an autumn morning with mist, and as a drawing with lines along the edges, each with the names of three places floating over them |
| `sea_chart.ographis` | The made-up islands as a sea chart of old, in the old topographic style: a sandy land with waterlines out into the sea, contours with every 20 m heavier and named, soundings in fathoms in a honeycomb that grow deeper away from the land, a compass star, the scale in figures and a frame with an index of squares, as PNG and PDF, and the register of the index as CSV |
| `retro_64.ographis` | The made-up terrain as on a home computer of the 1980s: coarse pixels of 100 m, ten bands of height in the colours of a 16-colour palette, on a blue screen with a start-up text as the title |
| `village_through_the_year.ographis` | The made-up village in 3D five times, in spring with blossom, on a midsummer evening, on a misty autumn morning, in falling snow and on a winter night under the northern lights, as a slide deck with a slide for each and as PNG |
| `toy_island_night.ographis` | The made-up islands as a toy diorama on a winter night, with snow, lit windows, northern lights and stars, and the kinds of land named over them |
| `finer_cartography.ographis` | Lit contours (Tanaka) over heights in colours; the valley of a made-up river in Swiss relief with hachures, the rivers widening downstream and named with spaced letters, the roads with the signs of their numbers, a bridge, a tunnel and one-way streets, also as a PDF in inks with black printed over the colours; where the old live, the colour by their share and the faintness by how many live there (value by alpha), also as the green-blind see it; and a Dorling cartogram and a tile map of the districts (`made_up_rivers.geojson`, `made_up_roads.geojson`, `made_up_districts.geojson`) |
| `inner_sea_floor.ographis` | The floor of a made-up inland sea in the manner of a bathymetric poster: the depths in blues from pale turquoise to deep navy with relief and the land as paper, the depths as a band of colours in the legend, seamounts and the deepest point with a line of description under each name, the basins in italics and the lands in spaced small capitals, a profile along a dashed line from A to B with the places on it, and a band along the bottom with the sources (`made_up_sea_floor.tif` and the `made_up_sea_*.geojson` files) |
| `clip_by_grid.ographis` | Cuts 3D blocks by the squares of a grid, each square on its own: a block across a line between squares is cut in two and closed with a wall, and each piece gets the number, row and column of its square. The pieces are written to a glTF file for each square and drawn from above in a colour for each row (`blocks.geojson`) |
| `sea_floor_styles.ographis` | The same made-up sea in five manners of bathymetric maps: depths in steps as layers of paper with soft shadows; land and sea in a colour each with strong relief; an atlas map with flat blue steps, cream land and the names of lands and waters; the sea alone in blues over a land of grey relief, with a white coast and a frame of coordinates; and heights and depths in colours with relief (`made_up_sea_floor.tif`, `made_up_sea_waters.geojson` and `made_up_sea_lands.geojson`) |
| `sea_floor_in_3d.ographis` | Two views of a made-up sea floor from the side: a seamount with a crater on its top in the colours of a sonar survey, violet in the deep to red on the top, and a rough sea floor in flat steps of 150 m, as layers cut from card, on a block that casts a soft shadow on a dark blue ground (`made_up_seamount.tif` and `made_up_sea_hills.tif`) |
| `even_finer_cartography.ographis` | An archipelago in depths of blue and heights in green and brown, the names of the islands along their coasts in small caps, a height profile of a trail, a globe in the corner and a frame along the degrees; a valley in texture shading with contour numbers reading uphill, on the sheets of a 10 km grid; districts drawn by hand with the lines of county borders; a village with its houses moved off the road and joined into blocks, on a UTM grid with MGRS names; and the islands as four PDF sheets to put together and as one folded page (`made_up_archipelago.tif`, `made_up_isles.geojson`, `made_up_trail.geojson`, `made_up_houses.geojson`) |
| `winter_village.ographis` | The made-up village in 3D: a frosty morning with rime, icicles, a frozen pond and smoke; sunrise in the mist with rays of sun; an advent evening with candles and stars in the windows, light cones under the lamps and the trails of cars; fireworks on New Year's Eve; a rainbow with the shadows of clouds; an autumn gale; a tilt-shift model village; and a December day from sunrise to sunset as a sheet and a GIF (`village_road.geojson`, `village_pond.geojson`). Each picture takes a while: draft quality is quicker while trying |
| `apis/` | Models for common web APIs, all needing the internet: population from SCB (PxWeb, JSON-stat) and Eurostat, inhabitants per municipality from Kolada page by page, events on the roads from Trafikverket with the key in an API connection, a temperature forecast from Open-Meteo as a chart, lakes from an OGC API Features service, a layer of an ArcGIS FeatureServer, Sentinel-2 images over Göteborg from a STAC catalogue, NDVI of a summer around Skövde from Sentinel-2 with the clouds masked, as one picture and one for each month (`sentinel_ndvi.ographis`, which could not be run where it was made: the catalogue could not be reached), addresses made into points with Nominatim at one request a second, unemployment in the NUTS 2 regions of Europe on a map from Eurostat and GISCO, the temperature at every SMHI station the latest hour, tomorrow's highest temperature in six towns from Open-Meteo as a chart, a key figure for the municipalities of Västra Götaland from Kolada as a chart, a city block as 3D Tiles from the samples of Cesium on GitHub, travel times between the towns from an OpenTripPlanner of your own (`otp_travel_times.ographis`, with its address in the variable otp), routes, departures and a count of departures between two stops from the REST API of OpenTripPlanner 2.4 (`otp_routes_and_departures.ographis`), a hundred start points around Göteborg (`gothenburg_starts.csv`, made up) routed by walking and public transport to Drottningtorget on a Wednesday at 07:30 with the GraphQL API of OpenTripPlanner, on a map with the legs coloured by mode (`otp_to_drottningtorget.ographis`), HERE: addresses geocoded, car routes with the delay of the traffic, isolines of 10, 20 and 30 minutes and pharmacies near a point (`here_examples.ographis`), TomTom: car routes in traffic with alternatives, the time to each town and reachable ranges (`tomtom_routing.ographis`), the last two with a key in an API connection, and `bolagsverket_companies.ographis`, name, form, address and industry of companies from the free API for high-value datasets of Bolagsverket (needs a client id and secret in an API connection), `transportstyrelsen_driving_licences.ographis`, holders of driving licences from the register of Transportstyrelsen as published by Trafikanalys, by sex over the years and by county, as charts, `trafikanalys_trains_and_roads.ographis`, trains on time and deaths on the roads from Trafikanalys, as charts, `svk_power_flows.ographis`, the flows of electricity over the borders every 15 minutes of a day from Svenska kraftnät, as charts and CSV (these four were checked against made-up answers, as the services could not be reached where they were made), and `chain.ographis`, a chain that runs three of them in turn with Run model. `github_statistics.ographis` draws Ographis on GitHub: issues opened and closed per week and open against closed, stars and forks over time, downloads per release, and clones, views and referrers the last 14 days, which need a token in an API connection named GitHub; each run adds the counts to history files, which `github_history.ographis` draws over time. It was tried against a made-up copy of the GitHub API, since GitHub could not be reached where it was made. The services may have changed since they were written |

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
  surface comes as a GeoTIFF with *The answer* as a file, read by an Input with the answers
  connected to its *Paths*. `date_add` spreads the departures over an hour, and the percentiles of
  *Calculate statistics* sum them up. See `examples/apis/otp_travel_times.ographis`.
  The REST API of OpenTripPlanner 2.4 (`/otp/routers/default/…`) is used in
  `examples/apis/otp_routes_and_departures.ographis`: every stop as a point, a planned trip
  between two coordinates with each leg as a line, all departures of a stop in a day, and
  the departures from one stop that reach another between 09:00 and 14:00, counted. The
  stops, times and server are variables. `examples/apis/otp_to_drottningtorget.ographis`
  routes a table of start points to one stop with planConnection of OpenTripPlanner 2.5 and
  later, explodes the legs of each trip, and draws them with *Filter by value* giving each
  mode a layer of its own colour on Draw map.
- **Service areas on OpenStreetMap**: `examples/apis/osm_service_areas.ographis` fetches
  the streets, paths and schools of the middle of Skövde with *OpenStreetMap download*,
  leaves out motorways, builds a network of them, and draws how far one walks to the
  nearest school in 5, 10 and 15 minutes at 5 km/h. Another rectangle in the two downloads
  makes it for another town; NVDB from Trafikverket can be read in place of the roads, with
  its speed limits for driving.
- **Skövde from Lantmäteriet**: `examples/apis/lantmateriet.ographis` fetches heights, a
  photo from above, Topografi 10 and the addresses of the middle of Skövde with *Fetch
  geodata*, one tool for all four, and makes a 3D model for Blender, a picture and a
  GeoPackage. It needs an API connection called Lantmäteriet, of the kind *A user and a
  password*, with the user and password of Geotorget, and the products ordered there. Another
  rectangle in the variable `area` makes it for another place in Sweden.
- **A volcano on Hawaii**: `examples/apis/volcano_hawaii.ographis` fetches Kīlauea with
  *Fetch a place*: heights of 30 m from the Copernicus DEM on Amazon's open data, and a
  picture of the summer from Sentinel-2 at Element 84, the least cloudy put together where
  they are clear, in UTM zone 5. *Terrain model* drapes the picture on the heights for a
  .glb that Blender imports, and *Render 3D scene* draws it. *Lighten 3D model* makes it
  light enough to walk on the volcano in the browser, packed, in `out/volcano_walk`. The
  place and the size are variables: Matterhorn, Isla Isabela or anywhere on Earth.
- **Hunneberg with roads and lakes**: `examples/apis/hunneberg_roads.ographis` fetches
  Hunneberg and Halleberg the same way, and the roads, paths and lakes within the square
  from OpenStreetMap with *OpenStreetMap download*. *Draw map* draws them over the
  picture, the larger roads wider, into a map of some 3 m a pixel, which is draped over the
  terrain for Blender and for *Render 3D scene*.
- **HERE and TomTom**: their keys go in API connections named HERE and TomTom, of the kind
  *A key only where {{key}} is written*. *Geometry from text* reads HERE's flexible polylines,
  as lines for routes and as areas for isolines, and TomTom's lists of points, as lines for
  routes and as areas for reachable ranges. See `examples/apis/here_examples.ographis` and
  `examples/apis/tomtom_routing.ographis`.
- **Swedish public APIs**, one model each, written to try the support for APIs. They were
  run against a made-up copy of each service, since the services could not be reached where
  they were made; the paths marked so in their comments are from the descriptions of the
  services, so check them with *Try one request* first.
  - `ngp_comprehensive_plans.ographis`: the comprehensive plans of Göteborg and Kungsbacka
    from the national geodata platform, one search for each municipality in
    `plan_areas.csv`, with every file of each plan kept (API connection Lantmäteriet).
  - `ngp_detailed_plans.ographis`: the detailed plans of Göteborg and Kungsbacka, one
    search for each rectangle in `detailed_plan_areas.csv`, a feature for each provision,
    those of the neighbours left out by the code of the municipality that made them, and the
    documents of the plans kept when the variable keep_files_in names a folder.
  - `scb_workplaces.ographis`: the workplaces of a municipality from the business register
    of SCB: every variable of every workplace in Sweden, 5 000 to a page by a cursor, and those of the municipality kept, as points where their addresses
    lie (API connection *SCB företagsregister*, a key in the header X-API-Key).
  - `visit_sweden_lodging.ographis`: places to stay from the national API of Visit Sweden,
    linked data in JSON-LD, where the place and its coordinates are found by their `@type`.
  - `goteborg_heights_wcs.ographis`: the heights of central Göteborg from the WCS of the
    City of Gothenburg, read only within a rectangle at 2 m cells, written as a GeoTIFF and
    shaded.
  - `krisinformation_news.ographis`: the news of Krisinformation.se, a row per area.
  - `jobsearch_gothenburg.ographis`: the job ads in Göteborg from JobSearch of
    Arbetsförmedlingen, as points.
  - `police_events.ographis`: the 500 latest events of the police in a county, the name put
    in the address with `url_encode`. The service places every event in the middle of its
    county.
  - `trafa_new_cars.ographis`: new cars per county from Trafikanalys, where each row is a
    list of cells found by their column, as `[Column=reglan].Value`.
  - `transportstyrelsen_vehicles.ographis`: vehicles in traffic per year from the vehicle
    register of Transportstyrelsen, which Trafikanalys publishes, and what the product holds.
  - `boverket_purposes.ographis`: the Ändamålskatalog of Boverket; the address and the key
    come from Boverket's API portal.
  - `skolverket_school_units.ographis`: all of the about 12 000 school units of Skolverket's
    register as points in a GeoPackage, from the details of each, four requests at a time,
    with the coordinates of the visiting address found by `[type=BESOKSADRESS]`.
  - `smhi_warnings.ographis`: the weather warnings of SMHI in force, as areas, read from the
    GeoJSON that *Explode JSON list* keeps whole.
- **Coordinate systems**: every one of the more than 6 000 EPSG systems PROJ knows can be
  searched for by name, code or area under *Search all…* in a coordinate system choice, and
  is shown by its name.

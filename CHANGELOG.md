# Changes

What changed in each version, the newest first. Models from earlier versions open and run
in later ones; where a tool changed what it does, it says so here.

## 0.96.8

In short: a release of finish, for maps and for 3D.

- **Maps as a mapmaker draws them**: lit contours, Swiss relief, hachures, texture shading
  and depths in blues; names along rivers, coasts and long lakes, in small caps and spaced;
  lines of borders, road signs, bridges and tunnels; a height profile, a globe and the grids
  of UTM and map sheets; and a style drawn by hand.
- **Maps for print**: CMYK and PDF/X-4, black printed over the colours, sheets to put
  together and folds.
- **The weather and the seasons in 3D**: snow, rain, fog, wind and lightning; frost, ice,
  icicles and smoke; rays of sun, the shadows of clouds and rainbows.
- **Evening and night in 3D**: the moon, stars and northern lights; lit windows with advent
  lights, lamps with cones of light, the trails of cars and fireworks.
- **The sea floor**: depths in blues with relief and the land as paper, depths in steps as on
  an atlas or as layers of paper, a band of colours in the legend, and sea floors in 3D in
  the colours of a sonar survey or in steps cut from card.
- **New tools**: Make a cartogram, Generalize buildings and Move away from lines; and
  pictures **one after another as a GIF**, for a day of sun or a map series.
- **Fewer tools, each doing more**: ten tools are now choices in the tools they belong
  with, Clip raster and Intersect in Clip among them, and Clip clips by each area on its
  own, 3D models too. Models from earlier versions are read as they were meant.
- **Every model in a folder checked in one go**, with a report of what went wrong, node by
  node: for testing before a release.

### Added

- **Clip by each clip area on its own** (#65). *Clip areas: Each on its own, with its
  attributes* in *Clip* cuts what lies in several clip areas, such as the squares of a
  grid, into a piece for each, and gives each piece the attributes of its area, with a
  prefix if you like. Points, lines, areas and 3D models alike: a mesh from a DWG is cut
  into an object for each square, which can then be written to a file for each square
  with `{square_cell_id}` in the name of the file. What lies in no area goes to Outside.
- **Surface** in *Filter by geometry type*: the objects of a 3D model, such as the meshes
  of a DWG, a TIN or buildings, go there with their triangles. Before, they went to Area
  and lost their triangles on the way.
- **Ographis starts maximized**, filling the screen.
- **Every model in a folder checked in one go**, before a release for example:
  `og run <folder> --recursive --quiet --no-history --report check.html`. At the end it
  says what went wrong in each model, node by node, without the rest of the logs, and the
  report is a web page, Markdown or JSON. `--recursive` takes the folders in it too,
  `--quiet` prints a line for each model as it is done.
- **Names that follow long areas.** *Curved along long areas* under the labels of a layer in
  *Draw map* writes the name of a long lake, a valley or a fjord along a line through its
  middle, bent as the area bends and spread out over it.
- **Index contours.** *Index contours by* an attribute, such as the height, and *every* 50:
  those contours are drawn heavier and only they are named, as on a topographic map.
- **A band inside borders**, as wide as you say, in the colour of the line and pale, as
  mapmakers draw the borders of countries and nature reserves.
- **Waterlines**: thin lines along the shore outside areas of water, fainter further out, as
  on old maps.
- **Relief in Draw map**: a terrain read as a grid can be drawn as *soft grey relief* lit
  from several sides, or as *heights in colours, with relief*: green lowland to brown and
  white heights, lit the same way.
- **An index of squares** across the map, with letters along the top and numbers down the
  side in the frame, and a new output *Register* of every name and the square it is in, as
  for a town map with a list of streets.
- **Which labels first.** A *rank* for each layer: layers with a higher rank place their
  labels first, so that the names of towns win over the names of lakes. *Corners first*
  places the labels of points up to the right of them first, then the other corners, as
  mapmakers do.
- **The scale in figures**, such as 1:50 000, under the scale bar or instead of it.
- **PDF for print**: the colours as CMYK, and with an ICC profile such as one of FOGRA or
  ISO Coated, a PDF/X-4 with the profile as its output intent and a trim box.
- **Weather in 3D.** *Render 3D scene* has **falling snow**, **snow on the ground**, the
  roofs and the trees (on what is flat first, in drifts), **rain** with wet streets and
  puddles that mirror the buildings, **lightning** whose flash lights the scene, or a whole
  **thunderstorm**, and **valley fog** that lies in the low parts, so that hills and tall
  buildings stand out of it.
- **Night in 3D**: the moon where the sun would stand, stars, and **northern lights** over
  the north, with lit windows and street lamps.
- **Trees in four seasons**: light green and blossom in spring, yellow, orange and red in
  autumn, and bare branches in winter, the conifers still green.
- **A drawing.** The models *as a drawing*: paper white, with **lines along the edges** where
  one thing stands in front of another and where a surface turns. The lines can be had on
  their own look too.
- **Names over places in 3D**, from the points on the new input *Names*, floating over each
  place on a thin line down to it, larger the nearer they are, where the camera sees them.
- **A map from Draw map on the terrain is as sharp as the map**: it is looked up where each
  ray meets the terrain, not at the corners of its triangles, so that contours and names
  draped over the terrain stay crisp.
- The examples `relief_map.ographis` (relief in colours, index contours, an index of squares
  with its register, and the map draped over the terrain with mist) and
  `weather_3d.ographis` (a village in snow, on a night with northern lights, in a
  thunderstorm, on an autumn morning with mist, and as a drawing).
- More examples: `sea_chart.ographis`, an old sea chart of the islands with soundings;
  `retro_64.ographis`, the terrain in 16 colours as on a home computer of the 1980s;
  `village_through_the_year.ographis`, the village in five seasons and lights as a slide
  deck; and `toy_island_night.ographis`, the toy island on a winter night under the
  northern lights.
- API examples for Bolagsverket (companies from the free high-value datasets, with OAuth2),
  driving licence holders from the register of Transportstyrelsen (through Trafikanalys,
  as charts), trains on time and deaths on the roads from Trafikanalys, and the power flows
  over the borders from Svenska kraftnät.

- **The finer craft**, in *Draw map*:
  - **Lit contours** (Tanaka): the edges of the areas between contours light towards the
    north-west and dark away from it, so that the land rises from the paper.
  - **Swiss relief**: hazy, pale lowland and clear heights, warm light and blue-grey
    shadows; and **hachures**, strokes down the slopes, thicker where it is steeper.
  - **Rivers that widen downstream**, and lines **as wide as a value**, such as the order
    of a stream or the traffic on a road; names **spread out along lines**.
  - **Bridges, tunnels and one-way streets** by expressions: bridges with dark edges and
    wings, tunnels dashed and pale, arrows along one-way streets.
  - **Road signs** with the numbers of roads along them: E roads green, national roads
    blue, others white.
  - The symbols of **marsh, conifers, deciduous trees, orchards, rock and sand** as
    patterns of areas.
  - Labels **in capitals where** an expression is true, such as for the large towns, and
    **as large as their area**, as the names of lakes; the names of **points that have no
    room** are put further out with a line in to them.
  - Areas **faint by a value** (value by alpha): the colour by one value and the
    faintness by another, so that what concerns few is not as loud.
  - The map **as the colour-blind see it** (deuteranopia, protanopia, tritanopia) or in
    grey, and a **look at the labels** that warns of those that stand out too little from
    what is under them.
  - The inset gets **its own scale bar**.
  - **Black printed over the colours** in a PDF in inks, so that black text and lines get
    no white edges when the plates are a little out of register.
  - A ramp **Heights: green to brown** for maps of heights.
- **Make a cartogram** (`CartogramMaker`), a new tool: circles as large as a value pushed
  apart from where the areas are (Dorling), or a square for each area on a grid (a tile map).
- **GTOPO30**, SRTM30 and GMTED are read as their tiles come: a .dem or .bil with its .hdr.
- The example `finer_cartography.ographis`, and marsh and forest symbols on the sea chart.

- **Still finer cartography**, in *Draw map*:
  - **Texture shading** of a terrain (*Texture: ridges and valleys*): every ridge and
    valley at every size, large and small alike, as engraved, rather than lit from one side.
  - **Depths in blues**: heights and depths together, the sea in blues that darken with the
    depth and the land in greens and browns.
  - **A height profile** of a line, such as a trail, from the grid of heights in the map,
    drawn in a corner with its highest and lowest point and its length.
  - **Contour numbers that read uphill**, the top of the figures towards the higher ground,
    and no numbers on small rings and short pieces.
  - **Names along the coast**: the names of islands and lakes written along the inside of
    their shore, bending with it.
  - **Letter spacing and small caps** for labels.
  - **Lines of borders**: country (dash and dot), county (dash and two dots) and
    municipality (short dashes), each fitted to its corners so that a dash runs through
    each, and a border between two areas drawn once.
  - **Drawn by hand**: a style of ink and watercolour, the lines a little unsteady.
  - **The grid of UTM and MGRS**, its lines named as `33V VE 18`, and **the sheets** of a
    10 km grid of SWEREF 99 TM with their names, such as `6470 410`, in each.
  - **A frame along the degrees**: with a grid of degrees, the map cut along its outermost
    lines of latitude and longitude, as maps of large areas are.
  - **A globe in a corner** with the land of the world (Natural Earth, public domain) and
    the place of the map on it in red.
  - The lines of the grid end at the curved edge of a frame along the degrees, and are
    named there; a height profile and its line on the map are marked A and B at their ends;
    and the legend shows a border as its line.
- **Maps of the sea floor**, as bathymetric posters are drawn:
  - *Sea floor* for a raster: the depths in blues from pale turquoise to deep navy, lit so
    that ridges, canyons and seamounts show, and the land as paper with a soft relief.
    Relief over cells of kilometres is lifted so that its slopes show, and relief drawn
    larger than its cells is blended between them rather than stepped.
  - **A band of colours in the legend** with values under it, for depths, heights and ramps.
  - **A described label**: *Lines after the first smaller* sets a line of description,
    such as "summit 510 m below sea level", under a name in bold.
  - **A richer height profile**: its own title, the places along the line written over it
    (*Places along the profile*), heights at round steps with faint lines across, the
    water in the blues of its depth, how much the heights are exaggerated, and its width.
  - **A band along the bottom** of the map, with the sources on the left and the credit on
    the right.
  - A layer can be left out of the legend.
  - **Steps of depth**: a colour from each value down to the next, as on an atlas map,
    each with its name in the legend. Empty gives the depths of an atlas, at 200 m,
    1 000 m and every 1 000 m down, from pale lilac through blues to black.
  - **Steps as layers of paper**: the same, each step casting a soft shadow on the
    deeper ones.
  - **Land and sea in a colour each**, both with a strong relief, as a model seen from
    above.
  - **The sea alone** in blues with relief, the land see-through, over a land of grey
    relief or anything else.
  - Relief over cells of kilometres stands out also in *Soft grey relief*, *Heights in
    colours* and *Land and sea*, and the floor of the sea in *Land and sea* has relief.
- **Sea floors in 3D** in *Quick terrain view* and *Shade terrain*:
  - The colours *Rainbow*, violet in the deep through blue, green and yellow to red on
    the tops, as sonar surveys are shown, and *Sea*, navy through turquoise and green to
    pale yellow. Both are also ramps for maps and blocks.
  - **Steps every**: the heights in flat steps, as layers cut from card, whose sides on
    the block show the layers in their colours.
  - A background of one colour, on which the block casts a soft shadow.
  - **Heights in degrees**, as GEBCO gives them, are seen with their cells in metres
    where they lie, rather than as a thin needle with only a warning. A relief seen from
    above keeps the coordinates in degrees.
- **Blocks coloured by a value** in *Extrude*: *Colour by* a number, such as the population
  of each square, gives each block its colour from a ramp, evenly from the smallest to the
  largest or in quantiles, with its top a little lighter; a value such as `#4a7fb0` gives a
  colour of its own.
- **A GIF from Output**: a map series, or any picture, written as .gif shows its pages one
  after another.
- **Generalize buildings** (`BuildingGeneralizer`), a new tool: houses close together become
  blocks, small corners are taken away and the smallest are left out, for a smaller scale.
- **Move away from lines** (`FeatureDisplacer`), a new tool: houses and the like are moved
  straight away from the nearest road until they are clear of it as it is drawn.
- **PDF in sheets and with folds**: *Sheets* such as 2x2 cuts a map into pages to be put
  together, each a little over its neighbours and with its neighbours named in its heading;
  *Folds* such as 4x2 marks where a map is to be folded.
- **Winter, light and lens in 3D.** *Render 3D scene* has, in sections of their own:
  - **Frost and rime** on everything out in the cold, glinting, and **ferns of ice** on the
    windows; **ice on the water** with cracks and snow blown over it; **icicles** under
    the eaves; and **smoke from chimneys**, straight up in still air and laid down by wind.
  - **Advent lights**: candlesticks on the sills and stars in some of the lit windows.
  - **Rays of sun** through cloud and mist, **the shadows of clouds** over the land, and a
    **rainbow** opposite a low sun, with its fainter second bow.
  - **Light cones under the lamps**, stronger in fog and snow, and the **trails of car
    lights** along roads at night: lines on *Lights* are roads.
  - **Wind**, from where you say: it slants the rain and the snow, raises waves, lays down
    the smoke, bends the trees and moves the shadows of the clouds.
  - **Fireworks** over the scene, whose light colours what is below them.
  - **Tilt-shift**, so that a town looks like a model of itself.
  - **A whole day**: a picture every so many hours from sunrise to sunset of a date, as a
    sheet, and the pictures of a day or a sun study **as a moving picture** (GIF).
  - The windows of the buildings from *Make buildings* are lit too, not only plain walls.
  - Rays of sun come from the glare of the sun, so that what stands in front of it casts
    its ray of shade; rainbows and light cones are drawn on all the cores.
  - The settings of *Render 3D scene* are in sections: the picture and the camera, the
    background and the look, the sun, a series of pictures, materials, trees, water, rain,
    wind and fog, light in the air, winter and cold, evening and night, the lens and names.
- The example `inner_sea_floor.ographis`: the floor of a made-up inland sea as a
  bathymetric poster, with seamounts, basins, a profile from A to B and a band of sources.
- The example `sea_floor_styles.ographis`: the same sea in five manners of bathymetric
  maps: steps as layers of paper, land and sea in two colours, an atlas map, the sea over
  a grey land with a frame of coordinates, and heights and depths in colours.
- The example `sea_floor_in_3d.ographis`: a made-up seamount in the colours of a sonar
  survey, and a rough sea floor in steps on a block over a dark blue ground.
- The examples `even_finer_cartography.ographis` (an archipelago in depth tints with names
  along the coasts, a height profile and a globe; a valley in texture shading on a sheet
  grid; districts drawn by hand with county borders; a village generalized and moved off
  its road on a UTM grid; and the islands as PDF sheets and folded) and
  `winter_village.ographis` (a frosty morning, sunrise in the mist, an advent evening, New
  Year's Eve, a rainbow, an autumn gale, a model village and a December day as a GIF).

### Changed

- **Nine tools are now choices in others**, so that there is one place for each thing.
  Models with them are read as the tool they are part of, with their settings and
  connections, and a search for the old name finds the new place:
  - *Intersect* is *Clip* with *Each on its own*, which also cuts 3D models and keeps the
    heights of the pieces. Intersection is Inside.
  - *Count points in polygons* is *Join by location*, *Counted only*. A line among the
    polygons now counts the points that lie on it.
  - *Classify by range* is *Classify values*, *Ranges of my own, each with its value*.
    One that made equal intervals or as many in each writes the name of the class where
    it did, and the number of the class beside it, in `<name>_number`.
  - *Generalize raster* is *Resample raster*, *Or cells into one*. Resample raster also
    takes the median, the smallest, the largest or the sum of the cells within.
  - *Bounding box of all* is *Replace with bounding box*, *A box around: All together*.
  - *Calculate length* is *Calculate area*, *Calculate: The length*, and the tool is
    called *Calculate area and length*. It can write both at once.
  - *Zonal statistics* is *Values from raster*, *Areas get: Statistics*.
  - *Flatten JSON* is *Extract JSON values* without any path.
  - *Split lines into segments* is *Split lines*, *Every corner*. Points and areas that
    are not split go to Other.
- **Clip raster is now part of Clip.** A raster, a grid or a picture, connected to
  *Clippee* keeps the cells inside the areas on Inside, cut to the areas or as large as it
  was, and the rest on Outside. Before, a raster connected to *Clip* was dropped without a
  word. Models with *Clip raster* are read as *Clip*, with their connections; one that kept
  the cells outside takes them from Outside. A search for "clip raster" finds *Clip*.

### Fixed

- **Every area in the 3D view.** A stream with more corners than the view draws at once,
  such as the 290 municipalities of Sweden, lost every other area, so that the country
  looked full of holes. Lines and areas are now drawn simpler instead, every one of them,
  and the view says so; only points, as of a cloud, are still drawn every n-th.
- **A label said twice** in *Draw map*: where two lines with the same name cross, such as
  the equator and the prime meridian of Natural Earth's graticules, both named 0, the
  second line's label was moved along it to a place of its own, far from the others. Now
  a line whose place for its label already shows the same words is left without.
- **Lantmäteriet's orthophotos** in *Fetch geodata*: the latest collection was taken even
  when it did not cover the area, so that a place outside it gave "orto-h-2010 has nothing in
  the area". Now every collection that matches is searched, and the newest picture that
  covers the area is taken. The error lists the collections searched.
- The dependency parquet is updated to 60.2.0 (RUSTSEC-2026-0595).
- Labels spread out along a straight line at an angle were set as if at another angle.
- **The photos of Lantmäteriet are fetched much faster**: a photo is read from the overview
  of its file that is as fine as the cell size asks, rather than in full and made smaller
  after, so that cells of 0.5 m from photos of 25 cm take a quarter of the data and cells
  of 2 m a sixty-fourth. Files on the web are also read with fewer requests. The historical
  black-and-white photos are no longer searched unless chosen under Collection, and the
  log names the first 30 collections and how many more, not all of them.
- **The whole log can be marked**: it is one text, so that Ctrl+A marks all of it, and
  marking with the mouse past its top or bottom scrolls it along. A click on a line about
  a node still goes to the node.
- A rainbow is also seen in a studio, on the room behind the models, and stands out on a
  light sky or wall.
- The glow around lightning fades smoothly into the air, with no edges where it ended, and
  the cloud lit by a bolt is no longer cut off at the horizon.
- Small caps are set as small caps: capitals for the capitals, and smaller capitals for the
  small letters.
- Wet ground in a low sun mirrored so much that a field looked like a lake; rime glittered
  as noise in a draft picture.
- A grid of UTM or sheets no longer also draws the grid of the map's own coordinates.
- The `image` dependency also reads and writes GIF, for the moving pictures.

## 0.96.7

### Added

- **Labels of several lines.** The label of a layer in *Draw map* can have a line for each
  attribute or expression, such as the name of a subregion and its area under it: Shift+Enter
  in the form, or `newline()` in an expression. The lines are centred under each other.
- **Numbers as people write them.** `format_number(x)` in expressions writes `7 937`, with
  decimals if asked, `format_number(1234.5, 1)` as `1 234,5`, and `'en'` as a third value
  for `1,234.5`.
- **Labels as a mapmaker sets them**: in *italics*, as water is named; *bold where* an
  expression holds, such as `population > 50000`; names of areas only on areas from a number
  of pixels, or **beside them with a thin line** when they do not fit; names **again and
  again** along long lines, such as a road or a river, every so many pixels; and the names
  of areas move within their area to make room for the names of towns.
- **Lines break under labels.** Roads, borders and outlines stop where a label is, so that
  the names stand clear, in the picture and as clipping in a PDF and an SVG.
- **Colour ramps the other way**, and a ramp **between two colours of your own**, chosen with
  the colour picker.
- **A fixed scale on paper.** *Size* in *Draw map* can be *At a scale on a paper*, such as
  1:50 000 on A3 landscape (A4 to A1, landscape or portrait): the map is centred on the data,
  and a warning says if the data reaches outside the paper.
- **Title, subtitle and logo.** Written large at the top left, and a picture of your own in a
  corner, as a share of the width. The scale bar and the north arrow move aside for it.
- **An inset.** *Inset of* takes an expression, such as `name = 'Göteborg'`: what it picks
  out is drawn larger in a corner, and a frame on the map shows where it is.
- **Simpler lines at the scale.** *Simplify to the scale* leaves out corners closer than a
  pixel to the one before, so that large layers draw faster and make smaller PDFs.
- **Flow maps.** *Flow lines* (`FlowLines`) makes a curve between two places for each row of
  a table of flows, such as commuters, bent to its right so that flows both ways lie side by
  side. With *Size by* in *Draw map* the lines are as wide as their value, also in the legend.
- **Arrow heads on lines**, at the end, the start or both, as long as a number of widths of
  the line, for flows, one-way streets or routes; the line stops under the head, and the
  legend shows it too. Lines can also stop a few pixels short of their ends, so that a flow
  ends beside its town rather than under it.
- **Railways and tramways.** Two new line styles: *Railway*, black and white in turn, and
  *Tram, with ticks*, a thin line with short marks across it.
- **Even class limits.** *even numbers* beside the classes rounds the limits, as 1 000 – 2 000
  rather than 1 237 – 2 191, as far as each stays near where it was.
- **Bivariate maps.** *and by* a second attribute colours areas by both at once, each in
  thirds, in a square of nine colours, with the square and its two arrows in the legend.
- **Dot density maps.** *Dots by* strews a dot for every so many of a value over each area,
  the same each time the map is drawn, with "1 dot = …" in the legend.
- **Nested circles in the legend.** *within each other* shows the sizes of points as circles
  standing within each other, each with a line to its value.
- **Bars in the legend.** Under the classes, a bar for each, as high as how many features it
  has, so that an uneven classification shows at once.
- **What the map is about.** *The map is about* takes an expression, such as a county: the
  rest is paler, the areas are outlined, and they can have a soft shadow or lines along their
  coast.
- **Layers by scale.** A layer can be drawn only between two scales, such as small roads from
  1:50 000 and closer. It leaves the legend too when it is not drawn.
- **Small multiples.** *A map for each value of* draws the map once for each value of an
  attribute, such as a year, side by side at one scale, each named, with one legend.
- **Map series.** *A page for each feature, named by* draws a map of each feature of a layer,
  such as each municipality, centred on it, named by it and with the rest paler: the pages of
  one PDF, or a PNG or an SVG each. `examples/map_series.ographis` makes one of the subregions.
- **Things around the map keep clear of each other.** The legend, the overview map, the scale
  bar, the north arrow, the logo and the inset are placed before anything is drawn: those
  that share a corner are stacked, in a column beside them when there is no more room, and
  the labels keep clear of them all.
- **In the middle of the top or the bottom.** *Upper middle* and *Lower middle* beside the
  four corners, for the legend, the scale bar, the north arrow, the overview map, the logo and
  the inset.
- **Clear of the coordinates.** With a grid, what goes along the top and the left edge begins
  past the coordinates written there, and the coordinates keep clear of the title, the legend
  and the rest.
- **The title where you want it.** *Title in the* places the title and the subtitle in a
  corner or in the middle of the top or the bottom, lined up with that side. It can have a
  box behind it, with an edge, its own font from a TTF or an OTF file, sizes and colours, and
  bold or not.
- **One font for the whole map.** *Font of the map* takes a TTF or an OTF for the labels,
  the legend, the scale bar, the coordinates and the title, such as the font of an
  organisation.
- **A frame with the coordinates outside.** *Frame* draws a line around the map with a white
  margin, and the coordinates of the grid are written in the margin on all four sides, with
  ticks, as on a printed map. At a scale, the map keeps its size and the margin comes
  outside it.
- **The legend as you want it.** The colour of its box and of its edge, or none, and its rows
  in up to six columns, each begun at a heading when one is near.
- **Scale bar and north arrow in more styles.** The scale bar as fields dark and light or as
  a line with ticks, with 1 to 10 fields and in kilometres or metres; the north arrow as an
  arrow half dark, a thin arrow or a compass star.
- **Layers laid on in other ways.** *laid on* under a layer: multiply, screen, darken, lighten
  or overlay, so that shaded relief or an aerial photo shows through the colours. Kept as
  such in a PDF and an SVG.
- **Only the area itself.** *The rest* under *The map is about* can be *Left out*: only the
  areas are drawn, in their own shape, background map and labels too, rather than a
  rectangle.
- **Smooth lines and areas.** *smooth* under a layer rounds off its corners, so that coarse
  data looks smooth at small scales.
- **A glow or a shadow.** *around* under a layer gives its features a soft glow around them or
  a shadow under them, in a colour of your own.
- **Notes on the map.** *Notes on the map*, one to a line: east, north and the text. Each is
  written in a box with a line to its place, where it keeps clear of the rest; the labels
  keep clear of it.
- **Map templates.** *Save as a template…* keeps the look of a map, all but its layers,
  title, notes and what it is about, in a .ogmap file; *Use a template…* gives a map that
  look, and *Template* takes from the file whatever the map does not set itself, so that
  a change to the template changes every map that uses it.
- **Bleed and crop marks.** A PDF can reach a few millimetres past where it is cut, with
  marks outside that show the printer where to cut, and the page as cut and the bleed named
  in the PDF.
- **Settings in sections.** The settings of *Draw map* are in sections that fold: layers, size
  and paper, background, title, legend, grid, overview map and inset, what the map is about,
  several maps or pages, and lines and labels. A section is open when anything in it is set.
- **A colour picker** beside every setting that is a colour, such as the colour of the
  background or of the frame of the overview map.
- **The look of the overview map.** The colours of its areas and lines, of its frame and of
  the box that shows where the map is.
- **Checked for print.** When a map is made for paper, a logo that would be enlarged, a raster
  whose cells become blocks and lines thinner than 0.1 mm get a warning.
- `examples/cartography.ographis` shows them: commuting between the towns of Västra Götaland
  (made-up numbers) at 1:1 300 000 on A4.

## 0.96.6

### Added

- **More charts** in *Draw chart*: a *histogram* of how values are spread; a *scatter plot*
  of one number against another, coloured by group, with a trend line, its equation and
  R²; *box plots* of the spread in each group; *stacked* bars, also as shares of 100 %, and
  *grouped* bars, with a legend; and a *population pyramid*. All are drawn as the bars and
  lines are, as shapes in a PDF.
- **Small multiples.** *One chart for each* in *Draw chart* draws a small chart for each
  value of an attribute, such as each municipality, side by side on one shared scale, so
  that they can be compared at a glance, or each on a scale of its own. Bars and lines too.
- **Charts on the map.** A layer of *Draw map* can have a pie or a few bars on each area or
  point, of the values of the attributes ticked under *Charts*, all as large, as large as
  their total or as an attribute, with the colours in the legend.
- **Dashboard web page** (`DashboardWriter`): one .html with a map of the areas coloured by
  a value and a bar for each area beside it, linked: the pointer on a bar lights up its
  area, and on an area its bar.
- **Classify values** (`Classifier`): natural breaks (Jenks), quantiles, equal intervals,
  standard deviations or breaks of your own, with the class and its name for the legend of
  each feature, and a table of the classes.
- **Shares, ranks and running totals** (`RunningStatistics`): the share of the total, the
  rank, the running total and share, and a moving mean, for all or for each group.
- **Change over time** (`ChangeOverTime`): the change since the time before, in numbers and
  percent, an index with a base time of 100, and the mean yearly growth.
- **Rates** (`RateCalculator`): cases per inhabitant, with small numbers hidden, a rate
  smoothed towards the mean for small populations (empirical Bayes), and rates directly
  standardised by age to a standard population.
- **Heat map** (`KernelDensity`): how dense points are, as a raster, weighed by an attribute
  if asked, per km², per hectare or within the radius.
- **Linear regression** (`Regression`): coefficients with their standard errors, t and p,
  R² and adjusted R², and what the model gives and misses for each feature.
- **More figures in *Calculate statistics***: under *Also*, the standard deviation, the
  coefficient of variation, the range, the number of different values and a weighted mean.
  What it gave before is unchanged.
- `examples/statistics.ographis` shows them on the towns and subregions of the examples.
- **Help in the program.** Help → *Ographis help*, or F1, opens a window with a few pages on
  getting started and a page for every tool: what it does, what comes in and goes out, each
  setting with its help, its choices and what it is when not set, and the examples that use
  it. The pages are made from the tools themselves, so they always tell what this version
  does. F1 with a node selected, or *Help* at the top of its form, opens the page of its tool.
- **From error to error.** F8 goes to the next node with an error in the latest run, and
  Shift+F8 to the one before; a click on the errors at the top, or on an error in the log,
  goes there too, and moves the canvas to it. *Next error* is above the log.
- **To what is wrong.** Under the error in the form, a button leads to what it is most likely
  about: *Open the connection …* opens the database or API connection in the settings, and
  *Go to …* marks the setting, such as a path to a file that is not there, or a setting the
  error names.
- **Places in API answers as features.** *Places in the answer* in *HTTP request* makes an
  answer of GeoJSON, a list of objects with `lat` and `lon` (as Nominatim gives them), or a
  list of coordinates into a feature for each place, with its values as attributes, without
  a Flatten JSON and a Make points after it.
- **A grid of degrees.** *Grid* in *Draw map* can be *Latitude and longitude, in degrees*:
  the lines of WGS 84 over a map in another coordinate system, such as SWEREF 99 TM, where
  they curve and lean, with their degrees, such as 58°N and 12°E, in the edge. *Grid every*
  then counts degrees.
- **Colours all can read.** *Draw map* and *Draw chart* warn when two colours of a legend are
  easy to tell apart for most but hard for those who see red and green alike, about one man
  in twelve, and say what to choose instead. The ramps and the colours that are given out
  from the start pass the test.
- **Legends of classes in their order.** A map coloured by `class` from *Classify values* gets
  the names of the classes from `class_label` in its legend, in the order of the classes, also
  with more classes than the theme has. Coloured by the names themselves, those that begin
  with a number, such as 10 – 50, come in the order of their numbers.

### Changed

- Dependabot leaves rstar to follow geo, which still uses rstar 0.12.
- url 2.5.9, for RUSTSEC-2026-0592; base64 0.23, wkt 0.15 and bytes 1.12.2.

## 0.96.5

### Added

- **Find changes** (`ChangeDetector`): compares two versions of the same data by a key, such
  as last month's register and this month's: *Added*, *Changed* (with an attribute that
  names what changed, such as `beds, geometry`), *Removed* and *Unchanged*. The attributes
  compared are chosen or all that both have, numbers may differ by a tolerance, and the
  geometry can be left out; without a key the features are compared whole. To keep a copy
  elsewhere up to date, only what changed needs to be written. See `examples/changes.ographis`.
- **The command palette.** Ctrl+K (or Ctrl+Shift+P) opens one search box for every command
  of the menus, the nodes of the model, the tools to add, the latest models and the
  examples: a few letters, the arrow keys and Enter. Each command shows its menu and keys.
- **A model described as a web page.** File → *Document the model as a web page…*, and
  `og doc`, write one page with a picture of the model, what it reads and writes, its
  variables and connections, and each step in the order the data goes through it, with its
  comment and settings, for handing over, reviews and intranets. Passwords and keys are
  left out.
- **DuckDB databases**, with DuckDB built in. *Input* reads their tables as layers, or a
  *Query* in their place, which can also read Parquet, CSV and JSON files with DuckDB's own
  functions; *Output* writes a table, written over or added to. The geometry is GEOMETRY,
  which DuckDB keeps without any extension, with the coordinate system as the comment of
  its column.
- **GeoArrow.** Arrow IPC files and streams (.arrow, .feather, .arrows), as data science
  tools pass tables between them, are read with the geometry as WKB or in the native
  layouts of GeoArrow, also compressed, and written with Zstandard, the geometry as WKB and
  the coordinate system in the metadata.
- **WebP pictures.** *Output* writes maps, charts and 3D scenes as WebP (.webp): smaller
  than PNG and JPEG, with the see-through parts kept. *Picture quality* (until now *JPEG
  quality*) sets how much is given up for size, and at 100 every pixel is kept. A picture
  with coordinates gets a world file (.wpw) and its coordinate system beside it. *Input*
  reads WebP as a raster, also when only a world file says where it lies, *Draw map* takes
  one as its background picture, a picture fetched by *HTTP request* is named .webp, and a
  model can be saved as a WebP picture.
- **Detailed plans of Göteborg and Kungsbacka**: `examples/apis/ngp_detailed_plans.ographis`
  fetches every detailed plan of the national geodata platform within a rectangle around
  each municipality, and keeps their files in a folder when asked. The comprehensive plans
  example now fetches Göteborg and Kungsbacka too.
- **A hospital in IFC** to try and show the reading of IFC with: `examples/hospital/hospital.ifc`,
  made by `tools/make_hospital_ifc.py`, five floors with about 800 elements and 162 rooms,
  and `examples/hospital.ographis`, which lists them and draws the building in 3D.
- **Heights of Göteborg from a WCS**: `examples/apis/goteborg_heights_wcs.ographis` reads
  the height model of the City of Gothenburg within a rectangle at a cell size chosen, and
  shades it.
- **Workplaces from the business register of SCB**: `examples/apis/scb_workplaces.ographis`
  fetches every variable of the workplaces in Sweden, 5 000 to a page by a cursor, and keeps
  those of a municipality as points where their addresses lie, with the key in an API
  connection.

### Changed

- **The formats of an output in groups.** The list of formats in *Output* is in groups
  under headings: tables, GIS data, maps and pictures, web maps and projects, 3D, point
  clouds, and CAD and 3D printing, the common ones first in each.
- **The address of a Google sheet.** With *Google Sheets* as the format, the field is
  named *Address of the sheet*, shows what such an address looks like and has no button to
  browse for a file. The format is named *Google Sheets* only.
- **Pages say how far they have come.** A request that follows many pages says in the
  status line how many it has fetched, *HTTP request: 12 pages so far*, instead of only
  that it is running.

- **The documents of plans, one row each.** For plans and other items, *Files* of *Fetch
  geodata* has a row for each document, however many items refer to it: its title, its
  type, its roles, its address, how many items refer to it and, when the files are kept,
  where it lies. A PDF or a map of a plan is found by its title, not among the addresses
  of every provision.

- **A path finds a value in a list.** `providers[roles=producer].kod` finds the provider
  whose roles, a list such as `["producer", "licensor"]`, hold producer, as
  `links[rel=next]` finds the link whose rel is next.

### Security

- **Keys are taken out of the logs.** The log of each run and the log of requests to web
  services, which lie beside the model, wrote an address as it was, so that a key given in
  it, such as `?api_key=…`, could be read there. They are now written with
  `api_key=(taken out)`, as the reports of problems already were.
- **Libraries with known holes are updated.** rustls, rustls-webpki and the root
  certificates (the checking of certificates in HTTPS), zip to 9 and quick-xml to 0.42 (files
  crafted to take much memory or time), ctutils, rustix and tokio. `cargo audit` finds no
  known hole in what Ographis uses; what is left is in parts of two libraries for
  spreadsheets that Ographis does not reach, listed with the reasons in `deny.toml`.
- **Checked every week.** A *Security* workflow on GitHub checks the libraries against the
  RustSec database with cargo-deny at every change and every Monday, together with their
  licences and sources, and Dependabot proposes updates as pull requests. `SECURITY.md`
  says how to tell about a hole and what is checked.

### Fixed

- **A picture of a model looks as the canvas does.** *Save the model as a picture*, *Copy
  the model as a picture*, `og picture` and the page of `og doc` draw the nodes as the
  canvas lays them out: the inputs, then how the node is set up, then the outputs, each node
  as wide as what it holds, the ports in letters of one width, comments in yellow boxes with
  a line to their node, group boxes with their names in a strip and in all their colours,
  such as teal and grey, and the grid of the canvas.
- **Plans from the national geodata platform are found.** Its search takes the rectangle
  in SWEREF 99 TM, not in degrees as other catalogues do, so *Detaljplan* and
  *Översiktsplan* found nothing. A search for items such as plans now goes on past 2 000,
  as a city has thousands, and a search that finds nothing names the product.
- **Pages by a cursor ask for their size.** *Parameter of the page size* and *Page size*
  are sent with every request, the first too, when the pages follow a cursor; they were
  left out, so a service gave its own, often small, pages. *At most pages* is shown for a
  cursor too.

## 0.96.4

### Added

- **Fetch geodata**, one tool for the products of a national mapping agency, chosen from a
  list. From Lantmäteriet: Markhöjdmodell as a grid of heights, Ortofoto as a picture,
  Laserdata skog as a point cloud, Belägenhetsadress, Kommun, Län och Rike and Topografi 10, 50, 250 and 1M as features,
  NMK 50 and NMK 250, the national military maps, as pictures in their colours, and from the
  national geodata platform Detaljplan and Översiktsplan, the plans with what is said of
  each as attributes and their documents kept when asked for. The area
  is a rectangle, chosen on the map, or what comes in on Extent. The files are found in the
  provider's STAC catalogue; heights and photos are read only where they reach into the
  area, at the cell size asked for, and where several years cover a place the latest is
  used. The login is an API connection: the user and password of Geotorget, or OAuth2
  with a key and secret of Lantmäteriet's API portal, whose address for tokens is filled in. *Files* gives each file
  with where it lies. A new product or provider is a line in its list, not a new tool.
  Example: `examples/apis/lantmateriet.ographis`.
  - *Keep the files in* keeps every file of the area whole in a folder, all the provider
    offers with each, such as pictures and PDF, under their own names by collection. A file
    that is there already is not fetched again, and *Files* says where each is kept.
    Without it, only what is needed is read, and files that must be fetched whole are kept
    for a while in the temporary folder of the computer.
  - Point clouds are counted from the headers of the files before anything is fetched, and
    the run stops with a word at *Most points*, 50 million unless raised, rather than fill
    the memory. *Every n-th point* and *Spacing between points* thin them.
  - A photo is put together as colours, a quarter of the memory of before.
- **HTTP request follows a cursor**: the pages of a service that gives a value such as
  `nextCursorId` in each answer, sent back as a parameter of the next request, until it is
  missing.
- **Try one request** in HTTP request sends only the first request and shows the answer:
  the status, the time, the headers and the JSON as a tree. A value clicked in the tree
  becomes *Cursor at* or *Next page at*, with the pages set to match, or *Items at* for a
  list, or is taken out by a new *Extract JSON values* after the node. A link among links
  is told by its rel, as `links[rel=next].href`.
- **Test the connection** among the API connections in the settings: a new token for
  OAuth2 and Google service accounts, and a request to a *Test address*, or else the
  address the connection is for. It says what went wrong, such as a key that was not
  accepted or an address with nothing at it.
- **Fetch geodata for each area on its own.** *Several on Extent* → *Each on its own*
  fetches for each feature that comes in, such as a municipality at a time, each with its
  own rectangle and largest area, and what is fetched gets the attributes of its area.
- **GeoJSON kept whole in Explode JSON list.** *Objects in an element* → *An attribute per
  value, but GeoJSON kept whole* leaves an area such as those of SMHI's warnings as one
  attribute, which *Geometry from text* reads as it is; *Kept whole* keeps every object as
  JSON.
- **`url_encode(text)`** makes a value safe to put in a web address, also with &, # and
  å, ä and ö.
- **Ten models for Swedish public APIs** in `examples/apis`: comprehensive plans from
  the national geodata platform, Visit Sweden, Krisinformation.se, job ads from Arbetsförmedlingen, events of the police,
  Trafikanalys, the vehicle register of Transportstyrelsen, Boverket, Skolverket and the
  warnings of SMHI. See EXAMPLES.
- **Settings to another computer.** Settings → *General* → *Export settings…* writes the
  database and API connections, the background maps and the other settings to a file,
  without any password, key or secret, for a colleague or another computer. *Import
  settings…* takes them in: those with a name that is there already take its place but
  keep its password, and it says how many still need one written in.
- **WCS in Input.** A coverage service is read as a raster: an address with
  `service=WCS`, or *WCS* under *The address is*. The coverages are its layers. Only
  the part within the rectangle is asked for, at the cell size asked for, as a grid, a
  picture, points or squares; without a rectangle, only the outline. An API connection
  gives it a login.

### Changed

- **How long is left.** A long step says about how long it has left, from how long the
  part done took, in the status line and the log: *Fetch geodata: 3 of 12 files (25 %) ·
  about 30 s left*. Files from the web are counted one by one, and a step that counts
  files, tiles or rounds names them instead of calling them features.
- **The error of a node can be copied**, with its name and its warnings, by *Copy* in
  its form, and the message under the canvas by right-clicking it.
- **How many rows are selected** in the table is shown beside the filter.
- **No value in the table is an empty cell**, no longer a dash, and an empty text is
  shown as `''`, so that the two are told apart.
- **The arrow keys in the table.** Once the table under the canvas is clicked, up and down
  pick the row before or after, and left and right scroll the attributes to the side,
  instead of moving the selected node. Buttons ◀ ▶ beside the filter scroll it too, for
  tables with many attributes.
- **A run does not change the size of the nodes.** How long a node took, how it went,
  whether it is cached and *▸ n attributes* are shown under the node instead of in it, and
  the counts on the ports have their room before the run. Nodes lined up in a group stay
  as they were put. From 100 000 a count on a port is written short, as *120 k*, and in
  full when pointed at.

### Fixed

- **Pages and http-cache.** An answer kept in http-cache with its pages is only taken again
  with the same settings for the pages: a changed *Cursor at* or *Items at* asks anew,
  instead of giving the pages that were followed before.
- **The help of a setting shows once** when the pointer rests on its name, not twice.
- **A crash when showing a few features with very many corners**, such as the borders of
  the whole country: the map asked the graphics card for more than it takes in one go.
  Above 400 000 corners in view the features are drawn as an image, as many features are.
- **A long error** under the canvas is cut beside the buttons of the map, and shown whole
  when pointed at and in the log, instead of pushing the panel over the settings of the
  node.
- **Walk-through:** flying goes where one looks, also up and down, not only along the
  ground. In a model higher than a kilometre, such as a volcano, the walk starts on the
  ground and not under it at the bottom of the model.
- **Ctrl+Shift+C** puts a picture of the model on the clipboard again. On Windows the
  shortcut came as *Copy*, so the selected nodes were copied instead.
- **Model → Schedule** shows what the Task Scheduler says with å, ä and ö, and *Is it
  there?* and *Remove it* say plainly when there is no task of that name.

## 0.96.3

Any place on Earth in 3D, a walk-through in the browser, buildings that look real, lighter
models for realtime, drawings for CAD and models for 3D printing, and Dissolve on land
cover on every core.

**Assistants: `og mcp`**

- `og mcp` makes Ographis a server of the Model Context Protocol, for Claude Desktop, Claude
  Code and editors. An assistant describes files and services in short, finds the tools and
  their settings, builds a model that the user sees on the canvas as it grows, checks it,
  tries it on the first 100 features of each input without writing anything, looks at the
  rows of a node a page at a time and at a map, 3D view or chart of what came out, and runs
  it in full once the trial went well. It can read the model open in Ographis, with its
  changes and the node selected.
- It uses only the folders it is given, never sees passwords or keys, never writes over a
  file it did not make, and reaches the web or writes to a database only when allowed.
- The guide to the model file, the reference of every node, the examples and the models in
  the folders are resources; *Build a model*, *Describe data*, *Fix a model* and *Explain a
  model* are prompts.
- `build.cmd -Package` also makes an extension of Claude Desktop (`.mcpb`) and the entry of
  the MCP registry. `tools/mcp-eval` gives typical tasks to real assistants, several models
  at a time, and judges their work. See `docs/MCP.md`.
- A model file changed outside Ographis is read again in the window when it has no changes
  there.

**Any place on Earth**

- **Fetch a place** is a new tool: heights and a picture from above of any place, by its
  name or its latitude and longitude, a square of the size you give in its UTM zone. The
  heights come from the Copernicus DEM GLO-30, and the picture is made of the least cloudy
  Sentinel-2 pictures of a time you choose, the clouds taken away. Both are read over the
  web, only the part that is needed. `examples/apis/volcano_hawaii.ographis` fetches
  Kīlauea. The log says what it does as it goes: the place found, the area, each file of
  heights and each picture as it is read, how cloudy it was and how long it took. The
  values of the pictures become reflectance by the scale and offset of the catalogue, also
  for pictures made since 2022, whose values have an offset added, and not twice where the
  catalogue has taken it away already; the dark ground of forest and lava is lifted by a
  curve, so that it is not black.
- Input reads heights from **SRTM** (.hgt), **DTED** (.dt0, .dt1, .dt2) and **ASCII grids**
  (.asc).

**3D in the browser**

- Output writes a **3D walk-through**: a folder with a page that opens in a browser, with
  the model inside it. Walk at the height of the eyes, following the ground and stopped by
  walls, run with Shift or fly with F; click an object for its attributes; move the sun and
  the shadows with a date and a time. The speed is 3 m/s to begin with and is changed with
  the wheel or + and − in the page; flying goes across a fortieth of a large model a second.
  See `examples/walk.ographis`.

**Buildings that look real**

- **Make buildings** is a new tool: footprints raised to their height or storeys, with the
  roof their attributes give, flat, shed, gable or hipped, along or across, facades with a
  window in every bay of every storey, and the colours and materials of their kind:
  plaster for houses, glass for offices and shops, sheet metal for industry, stone for
  churches and plain walls for garages, or those the attributes give. The attributes of
  OpenStreetMap, Overture and the property map are recognised by themselves. See
  `examples/buildings.ographis`.

**Lighter models for realtime**

- **Lighten 3D model** is a new tool: fewer triangles, by meshoptimizer, to a number or a
  largest change in metres, with the edges of each object kept; textures no larger than
  needed; and plain colours and textures put together into one texture, an atlas.
- glTF, 3D Tiles and the walk-through can be **packed** by meshoptimizer
  (EXT_meshopt_compression), with opaque textures as JPEG.
- **3D Tiles are coarser further away**: tiles of tiles, each with a lighter model of what
  they hold, so that a viewer such as Cesium shows a large model at once and streams the
  detail as one comes near. A single large object, such as a terrain, is split as well.

**CAD and 3D printing**

- Output writes **DXF and DWG** (2010, 2013 and 2018), by acadrust, also from a template
  drawing whose layers, line types, text styles and blocks are used: the CAD layer, colour,
  line type and block by attributes, points as blocks with their attributes filled in, and
  text from an attribute.
- Output writes **STL, 3MF and STEP**, in millimetres at a scale or as large as a printer
  plate; 3MF with its colours and STEP as faceted solids with colours. Input reads them: STL,
  3MF with its colours and units, and STEP where its faces are flat.

**Fixed**

- The 3D view shows a picture draped over a model, such as a satellite picture or a shaded
  relief on a terrain, where each triangle lies on it, rather than one colour for all.
- A 3D model, such as one read from IFC, keeps its shapes through tools that change only
  the attributes, such as *Keep attributes*, and so through filters after them: a storey of
  a building can be picked and walked through.
- A terrain with a picture is no longer tinted green in *Render 3D scene*, and the facades
  of *Make buildings* not darkened by their colour twice.

**Faster**

- *Dissolve* puts the areas into clusters that touch, found on every core, and merges many
  small clusters at once. Land cover, where every area touches others, is merged on all
  cores: a test of 24 000 areas of eight kinds takes 1.5 s on two cores, from 2.4 s.

## 0.96.2

Faster on many cores, XML and LandXML, IFC checked against IDS, Gaussian splats, real
places for game engines, OpenUSD, ready rules of the road, and smaller things for
databases, 3D and web developers, before 0.97.

**Faster**

- *Dissolve* spreads its work over the cores, also when there are only a few groups: the
  largest groups go first, and a group of more than 128 areas is merged in pieces of
  neighbours, one on each core, that are then joined two by two along their seams. Areas
  that stay apart find what they were made of by their boxes first, which was the slow
  part with many parts.
- *Buffer*, *Clip*, *Intersect* and *Join by location* work feature by feature on every
  core.

**XML and LandXML**

- Input reads **XML** as records, as JSON is: the elements that come again become
  features, found by themselves or at a path, with elements within elements as adress.ort.
- **Extract XML values** is a new tool: values out of XML in an attribute, by their paths.
- Output writes **XML**: an element for each feature and for each attribute.
- Input reads **LandXML**: surfaces as triangles, points and alignments with their arcs,
  each a layer. Output writes a TIN and points as LandXML.

**IFC**

- **Check against IDS** is a new tool: the objects of an IFC model checked against an
  Information Delivery Specification, by class, attributes, property sets, material and
  storey, with what each object lacks.
- *Outline from above* has a setting to show the cut in 3D or not.

**Routes on roads**

- *Shortest route*, *Service areas*, *Choose locations* and *Allocate to nearest* have ready
  **rules of the road** for OpenStreetMap and NVDB: one-way streets, roundabouts, speed
  limits and the roads that may be used by car, by bicycle or on foot, without conditions
  written by hand. Motorways, ferries, gravel roads and tolls can be kept away from.

**3D for other programs**

- **Real place for a game engine** is a new tool: a heightmap of 16 bits in the sizes of
  Unreal Engine, Unity and Godot, masks for forest, water, roads and fields, positions of
  trees, poles and houses to place as instances, and the roads as splines, with a file that
  says how to import them.
- Output writes **OpenUSD**, .usda and .usdz, with a layer for each value of an attribute,
  variants by another, and the coordinate system and origin in its metadata, for Houdini,
  Blender, NVIDIA Omniverse and the viewers of Apple.

**Gaussian splats**

- Input reads **Gaussian splats** from PLY and SPZ, and places them on the map, such as in
  SWEREF 99, by the axis that points up, a scale, an origin and a turn. Output writes them.
- The **3D view draws splats**, soft and in their colours, also with buildings and other
  map data in a View.
- **Make Gaussian splats** is a new tool: a point cloud from a scanner or a drone made
  splats, flat along the surfaces.
- Input reads a **PLY of points**, as scanners and drones give.

**Web**

- *HTTP request* can **write every request to a text file**: when, where, what came back
  and how long it took, for documentation and to look back on.

**Databases**

- Output to PostGIS writes **comments** on the table and its columns (COMMENT ON).
- Input from PostGIS reads **only the columns** named, so that a wide table is read faster.

**More**

- **Clip** closes a 3D model cut by the edge with a new wall along it.
- Output writes a **JSON Schema and TypeScript types** beside GeoJSON and JSON.
- A **start folder** in Settings, where Open and Save as begin.

## 0.96.1

Maps and 3D for the examples of 1.0, JSON and Google Sheets, before 0.97 takes on more CAD
and BIM.

**Draw map**

- **Ready styles** for the whole map: Light, Dark, Old topographic and Print in black and
  white. The colours of the layers, the labels and the rasters are made to fit, each layer
  keeping its own hue, and a background of a colour gets the paper of the style.
- **Patterns** over the fill of areas: hatched, cross-hatched, lines, squares or dots, in the
  colour of the line or one of their own, also in the legend and as lines in a PDF.
- An **opacity** for each layer, so that a background map or relief shows through.
- An **overview map** in a corner, with the map as a red frame: of what is connected to the
  new port *Overview*, such as the outline of the county, or of the map seen from further
  away, with its background map.

**Render 3D scene**

- **Ready places for the camera**: bird's eye, axonometric or straight down.
- **Roofs by their kind** from an attribute, such as roof_type of LoD2 buildings: flat,
  gable, hipped, shed and mansard, in English or Swedish, or a colour of their own.
- **Trees** on the points on the new port *Trees*: leafy with a round crown, or conifers as
  cones by an attribute with words such as gran, tall or barr, standing on the terrain.

**Outline from above**

- In 3D the node shows the model that comes in, with the height of the cut as a
  see-through plane.

**JSON**

- **JSON** is a format of Output: a list of objects, JSON Lines or one object, names with a
  dot as objects within objects, JSON in attributes as JSON, and the geometry as GeoJSON or
  WKT if asked.
- *Build JSON* gathers features into **lists**: one of all of them, or one for each value of
  an attribute, such as the rooms of each storey.

**Google Sheets**

- Input reads a **Google sheet** by its address: one shared with a link at once, and a
  private one through an API connection of the new kind *A Google service account*, whose
  key file is read in Settings.
- Output writes to a Google sheet, over what is there or after it, and adds the sheet when
  it is not there.

**Changed**

- `CONTRIBUTING.md` and `AGENTS.md` say that Ographis is described in its own words, without
  comparing it with other ETL programs.

## 0.96

Tools of your own made of other tools, variables that are asked for, no data that vanishes
without a word, and maps for print as sharp PDFs with coordinates and layers. The betas
below say all that came in; these are the last changes since beta 3.

**PDF and SVG of shapes, placed and in layers**

- A map from *Draw map* written to PDF as shapes now says where it lies, as a PDF of pixels
  does: QGIS and other programs that read GeoSpatial PDF place it on the map, and it stays
  sharp at any size.
- Each layer of the map is a **layer of the PDF**, by its name in the form or that of its
  stream, with its labels in a layer of their own, and the background and the frame in
  theirs: turned off and on in Acrobat Reader, and kept as layers in QGIS. An SVG gets the
  same layers, as Inkscape shows them.

**Fixed**

- *Manage attributes*: with many attributes ticked and removed at once, the panel on the
  right could grow over the whole model and not be dragged back. Each row now keeps to the
  width of the form, with its lists on a line under the name when the form is narrow, and
  the panel is at most 60 % of the window.

## 0.96 beta 3

Maps for print, before 0.98 takes the rest of the cartography.

**Draw map**

- **Lines** solid, dashed, dotted or dash and dot, and an **edge** under a line in a colour
  of its own, as roads have on a road map. Edges are drawn under all the lines of a layer,
  so that roads that meet run into each other.
- **Names that bend** along roads and rivers, letter by letter, where the line does not
  turn too sharply; *straight along lines* keeps the old way. Names **spread out** over an
  area, as the name of a lake or a district.
- **A scale bar**, **a north arrow** and **a grid** of lines or marks in the edge, with the
  coordinates written in it, each in a corner of one's choice; the coordinates keep clear
  of the scale bar and the arrow.
- **Source**: a text such as © Lantmäteriet, written small in the corner after the credit
  of the background map.
- The legend shows lines with their edge and dashes.

**PDF and SVG of shapes**

- A map from *Draw map*, and a chart, is kept as shapes as it is drawn, and Output writes
  it to **PDF as lines, areas and text**, sharp at any size and editable in Inkscape or
  Illustrator. Rasters, background maps, icons and heatmaps are pictures in it, and the
  letters are their outlines. A PDF of pixels placed on the map is a choice.
- Output writes the same as **SVG**.

**Terrain**

- *Shade terrain* and *Quick terrain view* can light the terrain from **several
  directions**, as a multidirectional hillshade: slopes turned from the sun keep their
  form. The shadows are still cast by the sun.

**Examples**

- `print_map.ographis`: the made-up town as a map for print, A3, with relief lit from
  several directions, roads with edges and their names along them, districts spread out,
  a scale bar, a north arrow and coordinates in the edge.

## 0.96 beta 2

What came up when beta 1 was tried.

**Nothing vanishes without a word**

- When features come in and nothing goes out, when an input reads nothing, or when an output
  gets nothing to write, and the tool has not said why, the log now says so, at the node
  where it happens.
- *Service areas* warns when the limits are in metres but look like minutes, such as the
  5, 10, 15 it starts with when *By* is the length, which gave areas too small to see. It
  says how much road was reached, and warns when nothing was. By time without a speed,
  every road is at the speed where none is given, such as 5 km/h for walking, instead of
  stopping.
- A test runs every tool on a mixed stream and fails if one gives nothing without saying
  why; `CONTRIBUTING.md` and `AGENTS.md` say that every tool, also new ones, tells what it
  left out.

**Changed**

- *Dissolve* keeps areas that do not touch apart, each a feature of its own with the
  number merged into it. Before, everything in a group became one multi-area; that is now a
  choice. Models that rely on the old way should choose *One multi-area for each group*.
- GeoJSON is written in WGS 84, as RFC 7946 says, and as web maps take for granted. The
  coordinate system of the stream can be kept as a choice.

**Faster IFC**

- A file with many cut-outs, such as pipes and fittings from Tekla, read about forty times
  faster: whether a point lies inside a solid is looked up in a grid of its triangles
  instead of against all of them.
- Revolved solids (`IfcRevolvedAreaSolid`), such as the elbows of pipes, are read.

**New**

- *Sum line lengths in areas* (`LineLengthInAreas`): the length of the lines in each area,
  such as the road network in each municipality, in all or for each kind of road.
- *Calculate length* has mm, cm, dm, Swedish miles, inches, feet, yards, miles and nautical
  miles; *Calculate area* ares, square feet, acres and square miles.
- *Concatenate attributes* can use fixed texts as separators of their own, so that
  different ones go between different attributes, such as *12 - Main street, Skövde*.
- *Make zip file* takes its other files and folders one on each row, with buttons to
  choose them, and a new row comes when one is filled in.
- *Draw map* gives a number with no more values than classes, such as 5, 10 and 15 minutes,
  a class for each value instead of ranges between them.

**Examples**

- `apis/osm_service_areas.ographis`: how far one walks to the nearest school in Skövde, on
  streets and paths from OpenStreetMap, fetched when it runs.
- `cost_distance.ographis`: minutes on foot across the made-up terrain from the health
  centres, by the slope and Tobler's hiking function.

## 0.96 beta 1

A beta: everything of 0.96 is in, to be tried before 0.96 is released.

**Tools of your own**

- **Make into a tool**: selected nodes become one node, a *Tool* with a model inside. What
  came into them from other nodes comes in on its ports, and what went out goes out on its
  ports, named after the nodes. A double-click on the tool, or Enter, opens the model inside
  on the canvas, with the path back above it; *Back*, the names in the path or Escape go out
  again, and what was changed inside is one step to undo. Saving and running take the whole
  model also while a tool is open.
- A tool runs once, **once for each value** of an attribute, with `$(value)` inside, or
  **repeated** a number of times with what comes out going in again, with `$(round)` inside.
  The variables of the model count inside.
- *Tool input* and *Tool output* are its ports inside, in the order they lie from the top.
- **Unpack** puts the nodes of a tool back in the model, connected as before, in a group box
  with its name. Group boxes stay boxes: nodes become a tool only when asked.
- `og check` checks the model inside a tool too.

**The model**

- **Asked before the run**: a variable with *Ask* ticked is asked for when Run is pressed,
  with its value offered, and the value given is kept for the next time.
- **A setting can take its value from data**: `$(node.attribute)` is the value of the
  attribute in the first feature the node with that id gives, such as
  `population > $(limits.least)`. The node runs after it, as if connected, and the cache
  knows when the value has changed.
- **Point at a connection** to see what went through it in the latest run: how many
  features, of which kind, the coordinate system and the attributes.
- **Ports that do not fit**: a connection between ports that surely do not go together, such
  as features into a tool that needs a raster, or a raster into one for point clouds, is drawn
  in orange before the run and says why when pointed at. `og check` warns of it too. A GeoTIFF
  read as points counts as features.

**Analysis**

- **Hot spots** (`HotSpots`): Getis-Ord Gi*, with gi_z, gi_p, hot_spot and hot_class from
  -3 to 3 for a map.
- **Spatial autocorrelation** (`SpatialAutocorrelation`): Moran's I with a p-value from
  permutations, and Local Moran (LISA) for each feature.
- **Point clusters** (`PointClusters`): DBSCAN, with an area around each cluster.
- **Reallocate areas** (`AreaReallocator`): counts moved from one set of areas to another by
  the share of the area, such as people in squares to districts.
- **Tracks and stops** (`TrackBuilder`): GPS points into tracks by device and time, jumps
  left out, and the stops with when and how long.
- **Choose locations** (`LocationChooser`): the best places among candidates for a number of
  facilities along a network, as least travel in all or as many as possible within a limit,
  with places that must be kept.
- **Classify image** (`ImageClassifier`): the cells of a raster or picture into classes, by
  themselves (k-means) or from example areas.
- **Hollows** (`Depressions`): where water collects on a terrain model, with depth, volume
  and area.
- *Service areas* can give an **outline** of what is reached instead of the roads, with or
  without holes: isochrones on a network of one's own.
- *Zonal statistics* counts a series of rasters over time: a row for each zone and time.
- *Draw map* draws **arrows** turned by an attribute, such as wind, and sized by another, and
  a **heatmap** of points, each counting by an attribute when one is given. The legend shows
  both.

**Databases**

- Output to PostGIS can **update the rows by a key** and add the new ones, and delete those
  not in the stream as a choice, in one transaction. A key that is empty or comes twice is an
  error before anything is written.

**Smaller things**

- A tool that gets nothing in now lets nothing out, with a note in the log, instead of
  stopping because an attribute is missing, such as *Create attribute* after a test that
  passed nothing.
- The help of a setting is shown when the pointer is on its name, so the forms are shorter.
  *Show the help under every setting* in the settings shows it as before.
- *Filter point cloud* says from which run the classes it lists come.
- Output has **Open folder** and **Open**, or *Open in browser* for a web page, once it has
  written its file.
- Arrows and other signs that the font of the program lacks are drawn from Hack, so that no
  boxes are shown on any system.
- Help → *Report a problem…* can **save a zip to attach**, with the model without its
  passwords and keys; `og report` makes the same from the command line.
- `.github/labels.json` and `tools/github_labels.py` set the labels of the issues on GitHub.

**Examples**

- `proximity.ographis`: proximity to care in a made-up town, with service areas, the
  minutes to the nearest health centre, the share within 20 minutes per district, and three
  new health centres chosen.
- `analysis.ographis`: hot spots, Moran's I, a GPS walk into tracks and stops, clusters, wind
  as arrows, hollows and *Classify image*, on the same town (`examples/analysis`, made by
  `tools/make_analysis_example.py`).
- `apis/github_statistics.ographis` draws charts of a repository on GitHub, its issues,
  stars, forks, downloads per release, clones, views and referrers, and adds the counts to
  history files at each run; `apis/github_history.ographis` draws them over time.

## 0.95.3

- **Save as a connection**: a PostGIS input or output with the server, database, user and
  password written in it, as models from before 0.63 have, moves them to a connection in the
  settings with one click, and then names only the connection. The password leaves the
  model. A connection that is the same is used again.
- **More**: the settings that are seldom changed wait under *More* at the end of the form, in
  PostGIS output (transaction, geometry column, primary key, SQL before and after,
  statistics), PostGIS input (coordinate system, largest number), Output (primary key,
  GeoPackage version, JPEG quality, how a GeoTIFF or PDF is packed, 3D Tiles) and *Draw
  map* (margin, paler background). *More* is open when one of them is set, and says how many.
- *Draw map* for print says, before it is run, how many pixels wide the map becomes, such as
  180 mm at 300 to the inch gives 2 126, and warns when it is wider than a map can be. The
  node on the canvas shows 180 mm at 300 dpi instead of the width in pixels.
- Fixed: Ographis could crash, as a double free, when a GeoPackage was closed while another
  part of the run read GML, KML or another XML at the same moment. GDAL loaded libspatialite
  into every GeoPackage, and libspatialite cleans up libxml2 for the whole program when one is
  closed. Ographis no longer loads libspatialite; SpatiaLite files are read as before, by GDAL
  itself. The check on GitHub stopped on this.

## 0.95.2

- **Outline from above** is a new tool: 3D objects, such as walls and doors read from IFC,
  as areas in 2D, the shadow each casts when lit from straight above, with all that sticks
  out, such as the handle of a door. Seen from a height, such as 1 m above the floor of the
  storey, what lies above is left out, so that the wall over a door leaves a gap, as in a
  plan; or only what the cut goes through. The height counts from the storey, the lowest
  point of the object or the coordinate system, and parts smaller than an area can be left
  out.
- Objects read from IFC get the height of the floor of their storey, `storey_z`.
- The names of the attributes under *Data types and sizes* of Output are shown whole, such
  as pixel_size of a raster; only very long names are cut, and shown whole on hover.
- *Filter point cloud* gives each class ticked a port of its own from the start, named by
  its number and name, such as 2 - ground; a class without a name by its number. Ticked all
  on *Points* is still a choice, and a model saved earlier with a connection from *Points*
  keeps it.
- Databases, PostGIS and SQL Server:
  - Output can run **SQL before** and **SQL after** the write, such as a DELETE of the rows
    of one municipality before they are written again, or a GRANT and a view after it.
    `{table}` stands for the table written, with its schema. The SQL runs in the same
    transaction as the write, so if it fails nothing is written.
  - The table gets new statistics (ANALYZE, or UPDATE STATISTICS on SQL Server) after the
    write, so the database plans the next query well; it can be turned off.
  - Under *Table* the form shows the names the table and the columns get in the database,
    such as Towns 2026 → towns_2026, and warns of names over 63 characters, which
    PostgreSQL would cut.
  - Input from a database can read **only within** a rectangle, chosen on the map as for a
    download, so that the database sends only the features there.
  - A password written in the model is warned of in the form and by `og check`; a connection
    from the settings, or $(password), keeps it out of the file.

## 0.95.1

- *Filter point cloud* lists the classes of the points that came in at the latest run, with
  how many of each, to tick, and can give each class ticked a port of its own, such as Ground
  and Building, with the rest on Other.
- Point clouds are drawn in 3D in their own colours, as an E57 or a LAS file with colours
  has them, and by class, height or intensity when chosen beside *heights ×*. Points without
  colours of their own are coloured by height, as before.
- *Draw map* draws for print: a width in mm and pixels to the inch, such as 180 mm at 300.
  The map gets as many pixels as that takes, lines, points and labels keep the size they have
  on the screen, measured on paper, and Output writes the resolution into the file by itself.
  *Pixels to the inch* in Output only says how large a picture prints, which its help now says.
- Choosing the rectangle of a download on the map: Sweden is no longer covered by a blue
  area when no rectangle is set; two small points at its corners frame the map instead, and a
  rectangle that is set is drawn as its outline.
- One Enter takes the rectangle drawn on the map. The first went to the canvas before.
- Once the rectangle is taken, the map stays with it drawn, while the download is selected;
  before, the map went blank. This holds for both OpenStreetMap and Overture Maps download.

## 0.95

Web maps, 3D clipping and large data

- **Web map** is a new format of Output: a folder with index.html, drawn with Leaflet or
  MapLibre GL, to open in a browser or put on a web server. The layers look as they do in
  *Draw map*, with classes of colour, sizes after a number and labels; a legend turns them on
  and off, a click on a feature shows its attributes, and the map starts zoomed to the data
  over a background map of your choice. The features go beside the page in WGS 84, as GeoJSON
  that also opens from the disk, or with MapLibre as PMTiles for large layers. A background
  map that needs a password or a key is left out, so that it is not written into the page.
- **Clip cuts TINs, surfaces and 3D models** (issue #38): each triangle the boundary crosses
  is cut in its own plane, with its heights, normals and texture coordinates, and walls are
  cut by upright planes along the boundary. Areas and lines with heights keep them too: a new
  corner gets its height from the edge or the plane it lies on. Clipping against a long
  boundary is several times faster, for flat features as well.
- **E57**, as laser scanners on the ground give them, is read as a point cloud: every scan
  placed by its pose, with colours and intensity.
- **Less memory for large vector data**: CSV is read with about a third less, GeoPackage and
  PostGIS are written without a copy of the table, CSV is written a part at a time, and
  `og run` lets go of what a step made once the steps after it have used it. 1 000 000 points
  joined to areas and written to GeoPackage took 1 112 MB and now take 675 MB.

The canvas

- Each node shows how long it took at the last run, and the one that took most of it stands
  out.
- The ports show what came out of them: a circle for features, a square for a raster or a
  picture, a triangle for a 3D model and a star for a point cloud.
- A line drawn with Ctrl and the right button cuts the connections it crosses.
- F connects the selected nodes from left to right.
- A node dragged with Alt held is lifted out of its connections, and what came in goes on to
  where it went.
- A node folds to its title with the arrow in its header, and stays folded in the file.

Output

- *GeoPackage version*: 1.2, 1.3 or 1.4 for a new file, for programs that read only older ones.
- *Pixels to the inch* for PNG, JPEG and GeoTIFF, kept in the file for printing; *JPEG quality*;
  and how a GeoTIFF is packed: Deflate, LZW, JPEG or not at all.

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

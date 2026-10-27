# Changes

What changed in each version, the newest first. Models from earlier versions open and run
in later ones; where a tool changed what it does, it says so here.

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

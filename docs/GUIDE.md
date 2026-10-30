# Using Ographis

The graphical program, the command line `og`, and what to do when something goes wrong. Back to the [README](../README.md).

## The graphical program

Under the menus is a row of icons for what is done most: *New model*, *Open*, *Save*,
*Run*, *Stop*, *Pause*, *Undo* and *Redo*. Rest the pointer on an icon and its name is shown. The menus look
and work like those of other Windows programs, and File → *Open recent* lists the latest
five models.

| To | Do this |
| --- | --- |
| Add a node | Fold out a list on the left by its arrow and click in it. The lists are folded in from the start, and the search box searches all of them, also by the names the tools had before. The × at the end of a search box empties it. Or right-click the canvas: *Add input*, *Add output*, or *Add tool* with a search box of its own |
| Add a tool by typing | Double-click empty canvas, or press `/` with the pointer over it. Write a few letters of the name, such as `buff`, choose with the arrow keys and press Enter. With nothing written, the tools used last come first |
| Add an input for a file | Drop the file on the window from Explorer. Several files give several inputs |
| Read some of the layers in a file | Drop a GeoPackage or a workbook with several layers. A window asks which ones to read, and whether each gets an input of its own or all go into one stream. The ticks are also in the form of the input |
| Read a folder of files as one stream | Drop the folder on the window, or click *Folder…* in an input. The node shows how many files it stands for |
| Read a file from the web | Write its address, `https://…`, as the path of an input |
| Read JSON that is not GeoJSON | Give an input the file or the web address, such as an open data service. The list of records is found by itself, such as the list in `results`, or set *Records at (JSON)*. Objects inside a record become attributes such as `adress.ort` |
| Try a model on part of large data | Set *Largest number of features* in the input, or Model → *Trial run* (Ctrl+Shift+F5): every input reads only the first features, 1000 unless Settings → *General* says otherwise, and the log says it was a trial |
| Read vector tiles | Give an input an .mbtiles or .pmtiles file, a folder of tiles (`z/x/y.pbf`), or a web address with `{z}/{x}/{y}` in it. Choose the zoom level; the pieces of a feature that the tiles cut apart are put together again, unless *Pieces* says keep them. From the web a rectangle to read within is needed |
| Read a zip file | Give the path or the web address of the .zip. When it holds several data files the first is read and the log lists the others; choose another with *File in the zip* |
| Open a model | File → Open, or drop the `.json` file on the window |
| Connect | Drag from an output port to an input port. Drop on empty canvas to type the new node, which is connected at once |
| Put a tool into a connection | Drag the tool onto the connection, which is drawn heavier when the tool is over it, and let go |
| Remove a connection | Click it and press Delete, or right-click it and choose *Delete connection*. Right-click the ring of a port to remove all its connections |
| Move a connection | Drag it from the input it goes to, and drop it on another input. A port is hit a few points outside its ring |
| Switch a connection off | Right-click it and choose *Disable connection*, or click it and use the button in the panel. It is drawn faint and lets nothing through, until it is enabled again |
| Set up database connections | Edit → *Settings…*, or Ctrl+, and *Database connections*: a name, the server, port, database, user and password, and *Test connection*. Inputs and outputs choose a connection by its name; the model keeps only the name |
| Avoid connections that cross | Right-click the name of a port and choose *Move up* or *Move down*. The port moves with its connections, and its name is marked for a moment |
| Set up a node | Click it. The form appears on the right |
| Switch a node off | Right-click it and choose *Disable*. It does not run and lets nothing through |
| Comment a node | Write in *Comment* in its form. The comment is shown in a box above the node, with a thin line to it. Drag the box to move it and an edge or a corner to resize it, down to the width of its longest word. It starts as high as its text. It follows when the node is moved |
| Add a free-standing note | Edit → Add a note, or *Note* in the list on the left. Write the text in the panel on the right. Drag any edge or corner of the note to make it larger or smaller, as with the comment of a node |
| Open a link in a comment or a note | Click it. Addresses that start with `https://`, `http://` or `www.` are underlined and open in the browser |
| Select several nodes | Drag a box on empty canvas. Shift + click or Shift + drag adds, Ctrl + click removes, Ctrl+A selects all, Esc none |
| Move nodes | Drag a node. Selected nodes move together, and a group box follows when all that lies in it is selected |
| Move everything | Ctrl+A and drag a node. Nodes, notes and group boxes all go along |
| Take back a change | Ctrl+Z, and Ctrl+Y to do it again. Everything that changes the model can be taken back: moving, adding, deleting, connecting and settings |
| Copy group boxes | Select all that a box holds, or everything, and copy. The box goes along, also to another window |
| Shape a connection | The secondary button on a connection: *Add bend point*. Drag the point where you want the connection to go; it is drawn as a smooth curve through its points. The secondary button on a point removes it. The points are saved with the model |
| Look at data in 3D | Tick *3D* above the map under the canvas. Features with heights, and a terrain model read as a grid, are drawn from an angle, coloured by the height. Drag to turn, drag with the secondary button to move, scroll to zoom; *heights ×* draws them taller for flat land  A click on a feature, such as a wall of a building, picks it, and its row is chosen in the table; a click beside them lets go. *Zoom to data*, or a double click, shows all of it again. The surfaces are drawn with a depth buffer, so what is nearer always covers what is behind. Ctrl and a click picks several |
| Copy values from the table | The secondary button on a value under the canvas: *Copy value*, *Copy row* (with the names, for a spreadsheet) or *Copy column* |
| Read only a part | *Read* on Input: *Only within a rectangle*, with its sides and its coordinate system. Each side is a number or a variable, such as `$(xmin)`; models that read within the variables before 0.75 get them written in the sides. A GeoPackage is then read through its spatial index, which is quick also for all of NVDB |
| Read a 3D model | Point Input to an .obj, .gltf or .glb file. The materials and textures of an OBJ (.mtl and pictures) are read from beside it; a .glb holds its own, and a .gltf has them beside it or inside it. glTF is read through the nodes of its scene, each placed by its transform, with its materials: the base colour and its texture, how metallic and rough it is, and see-through parts. Neither format says where it stands, so the input has the units (glTF is in metres; an OBJ more than 200 across is taken as centimetres), which axis is up (y for glTF, z for OBJ, unless you say otherwise), the origin as east, north and height, and how far it is turned. Each object of the file becomes a feature with its faces as a MultiPolygon Z, its main material and its height; the model itself goes along for the 3D view and Render 3D scene. A model without a coordinate system is drawn in the map in its own units, without a background map |
| Group nodes | Select them and press Ctrl+G. Drag the box by its name and the nodes in it follow. Resize it by any edge or corner. Ten colours to choose from in its panel, or one of its own from the square beside them |
| Set up a group | Click its name. Name, colour and the lock are in the panel on the right. Unlocked, the box moves without the nodes |
| Duplicate or delete | Ctrl+D and Delete act on the selection. So does a right-click on a node |
| Copy nodes to another model | Ctrl+C copies the selected nodes with the connections between them, Ctrl+X cuts them, and Ctrl+V pastes them where the pointer is, in this window or another window of Ographis. Paths are made whole when copied, so they still point to the same files. Also in the Edit menu, and Paste in the menu of the canvas |
| Move the canvas | Hold Space and drag, drag with the middle mouse button, or drag the scrollbars at the right and the bottom edge |
| Start | The start screen lists the latest models and examples to open with a click. Help → *Start screen* shows it again |
| Line nodes up | Select several and choose Edit → *Align*, or *Align* on the menu of a node: left, centre, right, top, middle, bottom, or spread them out evenly across or down |
| Describe the model | Model → *Description…*. The text is shown in a plate on the canvas, which can be hidden, and under the title in a picture of the model |
| Copy the model as a picture | Edit → *Copy the model as a picture*, or Ctrl+Shift+C, and paste it into a document or an e-mail |
| Zoom | Scroll. *Zoom to fit*, or Home, shows all nodes |
| Run | The Run button or F5. The run has a thread of its own, and the counts on the ports fill in node by node while it goes on. Dots move along the connections of the node at work. With *Reuse what has not changed* (Model menu, on for new models) only what has changed since the last run is run again, and outputs are always written |
| Run everything again | Model → *Run everything again*, or Ctrl+F5: reads every file and asks every service again, whatever is kept from the last run |
| Stop a run | The Stop button, Model → Stop, or Shift+F5. What has finished keeps its results, the node at work is broken off where it can be, and the rest is marked as stopped in the log. A tool at a step that cannot be broken off is left to finish on its own after three seconds: the run ends at once, and what the tool gives later is not used |
| Pause a run | The Pause button, Model → Pause, or F6. The run waits before the next node, and within reading, web requests and drawing a map. The same button, now *Resume*, or F6 again, lets it go on |
| Use the menus from the keyboard | Alt and the first letter, such as Alt+F for File, or F10. The arrows up and down move in the menu, left and right go to the menu beside it, Enter chooses and Esc closes. With a menu open, pointing at another opens that one |
| Run part of a model | Right-click a node. *Run to here* runs the node and what it needs, and nothing after it. *Run from here* runs the node and everything after it, with what comes before taken from the cache. It needs Model → *Reuse what has not changed*, which is saved with the model. With the cache on, a node whose result is kept has a small cylinder in its title: filled while the result is up to date, an outline when the node or something before it has changed since |
| Draw a map | Connect the streams to *Layers* of *Draw map*. Each is a layer, listed in its form with fill, line, width and point size. The arrows change the order, or drag a layer by its handle ⠿; the top row is drawn on top. A click on a colour opens the colour picker, with its code to type as in QGIS, such as `#80d3ff` (or `#80d3ff80` with how opaque it is). The secondary button on a colour, or on its name, copies it, and on another pastes it; the code is also put on the clipboard. Escape closes the picker. Connect an output to write the map as PNG, JPG or GeoTIFF, or click the node to see it on the map under the canvas |
| Show values on a map | Fold out *Values* under a layer of Draw map. *Size by* makes points larger with a number, by area. *Colour by* colours points, lines or areas by classes of a number (equal intervals, as many in each, or natural breaks) on a colour ramp, or by a text, a colour each. Areas coloured by a number are a choropleth map. *Order* draws the largest underneath, or the smallest. *Gather points within* draws points closer than a number of pixels as one, with their number in it. *Legend* puts a key in a corner of the map |
| Make points from coordinates in a table | *Create points from x and y*: choose the two attributes and the coordinate system |
| See data | Click a node or an output port. Table and map appear at the bottom |
| Shade terrain or see it in 3D | Read the terrain model with *Cells as* set to *Heights or other values on a grid*, and connect it to *Heights* of Shade terrain or Quick terrain view, or to *Terrain* of Render 3D scene. A map from Draw map in the same coordinate system goes to *Map*. A grid is shown on the map under the canvas as grey relief |
| Measure the terrain | Read the terrain model as a grid and connect it to *Slope and aspect*, *Visible area*, *Values from raster* or *Zonal statistics*. *Reclassify raster* and *Raster to areas* make classes and areas of the result. See `examples/raster_tools.ographis` |
| Work with a point cloud | Read the LAS or LAZ file. *Classify ground points* finds the ground in a cloud without classes, *Filter point cloud* keeps classes, returns and areas, *Height above ground* measures trees and roofs, *Building heights from points* measures buildings and their roofs, and *Point cloud to raster* makes terrain and surface models. An output writes points with heights as .las or .laz again. See `examples/point_cloud.ographis` |
| Draw a raster on a map | Connect it to *Layers* of *Draw map*. Its row then says *Draw as*: a colour ramp, a colour for each value, or grey relief. For each value tick whether it is drawn, choose its colour and give it a name for the legend, such as *Seen*; *Opacity* lets the map under it show |
| Fetch open map data | *Overture Maps download* within a rectangle, or *OpenStreetMap download*. Both need the internet |
| Drop files where you want them | An input for a dropped file is put where it was dropped on the canvas. Several files go one below the other |
| Find rows | Write in *Filter* above the table: a word finds the rows that have it in any value, and an expression such as `population > 10000` or `= name LIKE 'S%'` the rows where it is true. The header says how many of all are shown; the map and the statistics follow |
| Write an expression | Begin a word: attributes of the stream and functions that begin with it are suggested under the field. Tab or Enter takes one, Escape closes the list. Settings → *General* can turn the suggestions off |
| Sort the table | Click the name of a column. A second click sorts the other way, a third gives the order of the stream back |
| See the attributes after a step | Click *▸ n attributes* under the node. View → *Show the attributes on all nodes* folds out all of them |
| Find an input or an output | *Data sets* at the top left lists what is read and written. Click a name to select the node |
| Change what is read or written | Click the name under *Data sets* and change the path under it, or click *…* to choose a file |
| See a file in Explorer | Right-click an input or an output, on the canvas or under *Data sets*, and choose *Show in folder* |
| See statistics of the data | Click *Statistics* above the table. For each attribute: how many have a value and how many are empty, how many different values, the smallest and the largest, and for numbers the mean and the sum |
| Change the order under Data sets | Drag a name up or down |
| Know which file or layer a feature came from | Tick *Add the file name as an attribute* or *Add the layer name as an attribute* in the input. The attribute gets a name, which can be changed |
| Find a feature | Click a row in the table or the feature on the map. Page Up and Page Down move a page of rows at a time |
| Pick several | Ctrl and a click adds a row or a feature on the map to those picked, or takes it away; Shift and a click picks every row between. The map shows them all, and the menu of a value copies them for a spreadsheet |
| Save | The Save button, the File menu, or Ctrl+S |
| See the version | Help → About Ographis, which also says when the program was built |
| See how the program is worked | With nothing selected, the panel on the right says what the program is for and which formats it reads and writes. Help → Tips and shortcuts has the rest |
| Read a GeoTIFF, PNG or JPG | Drop it on the window. Choose in the input whether the cells become points, squares or one outline, or are read as an image to be shown or written as a picture |
| Give a workbook a look | Choose Excel or LibreOffice Calc in the output. Font, bold and locked top row, and colours are set in its form |
| Send features to different outputs by value | *Filter by value*: choose the attribute, write the values or click *Use the values in the data*. Each value gets an output |
| Use the same value in many settings | Model → *Variables…*. Write `$(name)` in any setting, and the value is put in its place when the model runs |
| Copy from the log | Mark text in the Log tab with the mouse, also across lines, and copy it with Ctrl+C. *Copy log* copies all of it. A click on a line about a node selects the node |
| Pick attributes in a new tool | Connect it after a tool that has run: its attributes can be picked at once, without running again |
| Name a layer in the legend | Write the name in the row of the layer in Draw map. Empty gives the name of the stream |
| Follow a long run | The Log tab fills in while the model runs. A large file is reported every tenth read, and so is a status line at the top right |
| See what was written | Each output says in the log how many features it wrote, to which file, and the end of the log lists them all again |
| Write a file, layer or sheet for each value | Put an attribute or an expression in braces in the file name or the layer of an output: `out/{municipality}.gpkg` writes a GeoPackage per municipality, the layer `{municipality}` a table or a sheet for each in one file, and `stops_{now('%Y-%m-%d')}.xlsx` the date of the run in the name. Characters a file name cannot have become `_`, and an empty value is called `empty` |
| Add to a table, a sheet or a file that is there | Set *If it is already there* in the output to *Add the features to it* |
| Keep, rename, remove or order attributes | *Manage attributes* lists every attribute that comes in, with Keep, Rename or Remove, its type, and arrows for the order. Choose another type, and the values are made into it; those that cannot be are left empty. New attributes are added under the list, with a value, or empty, of a type you choose |

Attribute lists and the checking of expressions rely on the latest run. So run the
model once before you set up nodes further down the chain.

The map shows OpenStreetMap as a background when the server can be reached, and otherwise a
grid with coordinates in the model's own system. Other background maps are added in
Settings → *Background maps*: XYZ tiles, WMS or WMTS, with a user and password or a key
when the service needs one (kept in Windows' Credential Manager, as database passwords
are), and chosen above the map. *Labels* above the map writes the values of an attribute
by the features. Right-click the map to copy the coordinates of the point, in the system of
the data or as latitude and longitude. *Measure* above the map, or the same menu, measures a
distance or an area: click the points, and Escape ends. Measures are on the ellipsoid when the data has a coordinate system.
A grid of values is shown in colours: a colour for each value when there are few whole
values, as for classes or what is seen, and otherwise a ramp from yellow to blue over grey
relief. Every feature is drawn.
When up to 40,000 features are in view they are drawn as shapes: click one to find its row
in the table. When more are in view they are drawn into an image in the background, which
takes a moment for millions, and the map says so. Zoom in to see them one by one. The tiles are cached on disk. The
system proxy and certificates are used. A tile server of your own is given with the
environment variable `OGRAPHIS_TILE_URL`, in the form `https://…/{z}/{x}/{y}.png`.

The program follows the light or dark theme of the system, and uses Segoe UI on Windows.

The program closes at once also after a run with millions of features. The memory is
handed back to the system as the program ends, rather than freed piece by piece, and the
result of an earlier run is freed beside the window when a new run starts.

## Commands

```
og run <model.json | folder> [--var name=value] [--log file] [--inspect node[:port]] [--rows N] [--sample N]
       [--keep-logs N] [--no-history] [--no-lock] [--lock-hours H]
                                       runs a model, or every model in a folder, with the log as it goes
og history <model> [--last N] [--unlock]
                                       the latest runs of a model, and its lock
og info <file>                         layers, counts, coordinate system, attributes
og nodes [--json]                      node types, parameters and expression functions; --json for programs
og check <model | folder>              checks a model without running it: types, ports, settings, expressions
og schema                              a JSON Schema of the model format
og connections                         the database connections of the settings, without passwords
og picture <model> <file.png|.jpg|.pdf>
                                       the model drawn as on the canvas, for documentation
og formats [--html file]               the formats read and written, and whether GDAL or Ographis handles them
og doctor                              tests GDAL and PROJ in the installation
```

File → *Save the model as a picture…* draws the open model to PNG, JPEG or PDF in the
same way: its nodes with how they are set up, the connections, group boxes, comments and
notes. `docs/FORMATS.html`, and `FORMATS.html` in the packed folder, list the formats.

A model made in the window runs the same way from the command line, without the window,
for example from a script or the Task Scheduler of Windows. Model → *Copy command line*
puts the command for the open model on the clipboard, with its variables:

```
"C:\Ographis\og.cmd" run "C:\GIS\models\health centres.json" --var year=2026 --log C:\GIS\logs\runs.log
```

- `--var name=value` gives a variable of the model another value for this run.
- `--sample N` is a trial run: every input reads only its first N features.
- `--log file` adds every line of the log, with the date and time, to the end of the file.
  The folder is made if it is not there.
- A folder runs every model (`.json`) in it, in the order of their names, and ends with a
  line for each: OK, WARNINGS or ERRORS, and the time it took.
- The exit code says how it went: 0 all went well, 3 the run finished with warnings, 1 a
  node failed or the model could not be read, 4 the model was running already, 2 the
  command is written wrong. For a folder it is the worst of the models. A script or a
  scheduled task can act on it, for example send an e-mail when it is not 0.

### Run history, logs and the lock

Every run, from the command line and from the program, is kept in a folder beside the
model, named after it: `house.ographis` keeps its runs in `house.runs`.

- `history.csv` has a line per run: when it started, how many seconds it took, how it went
  (ok, warnings, errors or stopped), the number of nodes, warnings and errors, how many
  features and files the outputs wrote, whether it ran from `og` or the program, the user
  and the computer, the name of its log and the first error or warning. Semicolons, so
  that it opens in Excel as it is.
- `logs/2026-10-22_183000.log` is the whole log of a run from the command line, line by
  line with the time. Logs older than 30 days are removed when a model runs;
  `--keep-logs N` keeps them N days, and `--keep-logs 0` keeps them all.
- `lock` is there while the model runs, with the process, the computer, the user and when
  it started. A second run of the same model, from a scheduled task that starts again
  before the first has finished or from the program, does not start: `og run` ends with
  exit code 4 and RUNNING, and the program says who is running it. A lock whose process
  has ended on the same computer, or that is older than 24 hours (`--lock-hours`), is
  counted as left by a crash and taken over. `og history <model> --unlock` or *Remove
  lock* in the program removes one by hand. `--no-lock` runs regardless.
- `--no-history` keeps neither the line nor the log, for example in a test.

`og history <model>` shows the latest runs (`--last N`, 20 to begin with), and whether it
is running now. In the program, Model → *Run history…* shows the same for the open model,
with a button for the log of each run and one to open the folder.

## When something goes wrong

Help → *Report a problem…* writes a report with the version, the system and the errors of the
latest run, to read through and send as an issue on GitHub. Help → *Check for updates* asks
GitHub whether a newer version is out.


- **A fault in a tool** stops only that node. It gets an error that says so, and the rest of
  the model runs as usual: no work is lost, and the program goes on.
- **Every fault is written** to `crash-log.txt` in the folder for settings
  (`%APPDATA%\Ographis` on Windows), with the version, where in the code it happened and how
  it got there. Send it in with a report. The log keeps about the last megabyte.
- **The log of every run** of a saved model is written as the run goes, line by line, in
  the folder beside the model named after it: `house.ographis` has `house.runs\logs`, with
  `history.csv` beside it. Should the program stop in the middle of a run, the last lines
  tell how far it came. A program stopped by the system, such as when the memory runs out,
  has no time to write `crash-log.txt`, so the log of the run is where to look.
- **A window that stops answering** for more than 0.7 s says so in the log afterwards, with
  the part of the window that took the time, such as *data, map and log* or *the canvas*.
  Send that line along with what you did.
- **Changes that are not saved** are kept every half minute in the folder `recovery` among the
  settings. If Ographis stops without being closed, by a fault, a power cut or a restart for
  updates, it offers them the next time it starts: *Restore* opens them as a model with
  changes not saved, in the file they came from, and *Throw away* lets go of them. Several
  Ographis windows at once each keep their own.

## AI assistants

An AI assistant can write a model from a description of the task. [`AGENTS.md`](../AGENTS.md)
tells it how: the node types are looked up in [`nodes.json`](nodes.json) (or `og nodes
--json`), and the model is checked with `og check`, which names what is wrong and what would
be right, until it says OK. [`model.schema.json`](model.schema.json) lets an editor check a
model as it is written.

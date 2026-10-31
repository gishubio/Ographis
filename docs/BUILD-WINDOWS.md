# Building on Windows 11

Versions 0.2.1 to 0.5.1 were built and run on Windows 11 with these steps, against GDAL
3.13.3 from OSGeo4W. What has and has not been checked since is listed under
[What has been checked](#what-has-been-checked).

## In short

1. Install Rust. The installer offers to fetch the Visual Studio C++ tools. Accept.
2. Install OSGeo4W with the package `gdal-devel`.
3. Unpack the project, for example in `C:\Dev\ographis`.
4. Run `build.cmd` in the project folder.
5. Start with `Ographis.cmd examples\skaraborg.ographis`.

## 1. Rust and the C++ tools

Download `rustup-init.exe` from <https://rustup.rs> and run it.

- If the Visual Studio C++ tools are missing, the installer offers to fetch them. Choose
  that option. It installs Visual Studio Community with what is needed. That part needs
  administrator rights, and on a work computer you may need help from IT.
- To do it yourself instead: install *Build Tools for Visual Studio* and choose the
  workload *Desktop development with C++*.
- Then choose the standard installation of Rust (option 1).

Open a **new** PowerShell window afterwards and check:

```powershell
cargo --version
rustc -vV          # the host line should end in windows-msvc
```

## 2. GDAL through OSGeo4W

The build needs GDAL with development files. The GDAL that comes with an ordinary QGIS
installation is not enough, because the import library `gdal_i.lib` is not in it.

1. Download the *OSGeo4W network installer* from <https://trac.osgeo.org/osgeo4w/> and run it.
2. Choose **Advanced Install** and then *Install from Internet*.
3. Keep the root folder `C:\OSGeo4W`. If you already have QGIS installed through OSGeo4W in
   that folder, the package is simply added.
4. At *Select Packages*: type `gdal-devel` in the search box, open the category *Libs* and
   click the word *Skip* on the row `gdal-devel` so that it changes to a version number.
5. Click on. The installer adds `gdal`, `proj` and the rest that is needed by itself.

Check:

```powershell
Test-Path "C:\OSGeo4W\lib\gdal_i.lib"     # should answer True
```

## 3. Get the project

With Git, clone it to a short path:

```
git clone https://github.com/gishubio/Ographis.git C:\Dev\ographis
```

Or download the source zip of a release from https://github.com/gishubio/Ographis/releases and
unpack it. A zip file contains one folder. Put that folder somewhere with a
short path, so that you get for example `C:\Dev\ographis`. Avoid OneDrive and other synced
folders: the build creates tens of thousands of files in `target`.

You may want to right-click the zip file first, choose *Properties* and tick *Unblock*.
Otherwise Windows asks for permission every time a `.cmd` file in the folder is run.

## 4. Build

```powershell
cd C:\Dev\ographis
.\build.cmd
```

The script `build.ps1`, which `build.cmd` starts, does this:

1. Checks that Rust and the C++ tools are there.
2. Finds GDAL, reads its version and locates `proj.db`.
3. Builds the two programs with `cargo build --release`. The first time, all dependencies
   are downloaded and compiled, which took about four minutes on the first Windows build.
4. Writes the launchers `Ographis.cmd` and `og.cmd` in the project folder.
5. Runs the build test: `og doctor` and the example model `examples\buildtest.ographis`.

The script changes nothing in the system. If GDAL is somewhere else: `.\build.cmd -Gdal D:\GDAL`.
With `.\build.cmd -Test` all tests are run as well.

`doctor` should answer `OK` for GeoPackage, Shapefile, GeoJSON, GeoTIFF, Excel, LibreOffice
Calc, the PROJ database and the reprojection of a known point in Gothenburg. A dash at Excel
or LibreOffice Calc means that this GDAL was built without that driver. Everything else then
works, and workbooks can still be written, since the program writes them itself, but not
read. PostGIS is listed for later and is not used yet. The example model reads
`examples\subregions.gpkg`, reprojects to SWEREF 99 TM and writes
`examples\out\subregions_sweref.shp`. Open it in QGIS and check that it lands in the right place.

## 5. Start

```powershell
.\Ographis.cmd examples\skaraborg.ographis
```

Press F5 to run the model and click a node to see its data. You can also double-click
`Ographis.cmd` in Explorer. The program then starts with an empty canvas, and you can drop
data files on it.

Always start through `Ographis.cmd` and not through `ographis.exe` directly. The launcher
says where the GDAL DLLs and `proj.db` are. The command line is run the same way:

```powershell
.\og.cmd run examples\skaraborg.ographis --inspect out_towns
.\og.cmd info examples\routes.shp
```

The program draws with DirectX 12 through wgpu and needs nothing beyond what comes with
Windows 11.

## Updating to a new version

Unpack the new zip file over the old folder, or next to it, and run `.\build.cmd` again.
The `target` folder can be kept: only what has changed is compiled again.

## The GDAL version

The Rust binding (`gdal-sys` 0.12) has ready-made bindings for GDAL 3.4 to 3.12. In October
2026 OSGeo4W delivers GDAL 3.13. The script then builds with the bindings for 3.12 against
the newer library. That works because the C interface of GDAL only grows between versions
within the 3 series. The program only uses the vector part, where the interface is old and
stable.

`og version` and `og doctor` show the version of the library that is actually used.

## Building by hand

If the script is not allowed to run, or if you want to see what happens. In PowerShell, in
the same window that you then build in:

```powershell
$env:GDAL_HOME    = "C:\OSGeo4W"
$env:GDAL_VERSION = "3.12.0"
$env:PATH         = "$env:GDAL_HOME\bin;$env:PATH"
$env:PROJ_DATA    = "$env:GDAL_HOME\share\proj"
$env:GDAL_DATA    = "$env:GDAL_HOME\apps\gdal\share\gdal"

cargo build --release
.\target\release\og.exe doctor
.\target\release\ographis.exe examples\skaraborg.ographis
```

What the variables do:

- `GDAL_HOME` – the build script looks for `lib\gdal_i.lib` here.
- `GDAL_VERSION` – on Windows there is no `pkg-config`, so the version has to be given.
  Write the installed version if it is 3.4 to 3.12 (`gdalinfo --version` shows it),
  otherwise `3.12.0`.
- `PATH` – the programs need to find `gdal*.dll` when they start.
- `PROJ_DATA` – PROJ needs to find `proj.db`.
- `GDAL_DATA` – GDAL's own data files.

The variables only apply in that window. The programs therefore only start from there.

## A folder for others to try

```powershell
.\build.cmd -Package
```

builds as usual and then makes `dist\Ographis-<version>-windows` and a zip file of it beside
the folder. Give the zip file to those who are to try the program: they unpack it anywhere
and double-click `ographis.exe`. Nothing needs to be installed, neither OSGeo4W nor Rust.

What the folder holds, and how it is found:

```
ographis.exe, og.exe
*.dll                     those the programs ask for, and what those ask for in turn
proj\                     proj.db and the small files of PROJ, not the grids of other countries
gdal-data\                GDAL's data folder
curl-ca-bundle.crt        the certificates GDAL checks web addresses against, if OSGeo4W has it
examples\                 the examples, without what earlier runs wrote
READ-ME-FIRST.txt         how to start, for the one who gets the folder
licences-of-gdal-and-its-libraries\, THIRD-PARTY-LICENSES.md, LICENSE-*
```

- **The DLLs** are found by reading the import table of each program and DLL, also the
  delay-loaded ones, and taking them from GDAL's bin folder. So only what is used comes
  along, not the Qt and Python of a QGIS in the same OSGeo4W. DLLs of Windows stay out. The
  runtime of Visual C++ (`vcruntime140.dll`, `msvcp140.dll`) is copied from System32, as
  Microsoft allows, so that the folder runs where it is not installed.
- **Plugins of GDAL** are left out, since none of the formats Ographis uses needs them in
  OSGeo4W. `-Package -WithPlugins` takes them along, with what they need.
- **Variables of the computer** do not disturb: when `proj\proj.db` is next to the program,
  its own PROJ and GDAL data are used even if PostGIS or QGIS have set `PROJ_LIB`,
  `GDAL_DATA` or `GDAL_DRIVER_PATH` for the whole computer.
- **The script tries the folder** before it makes the zip file: `og doctor` and the build
  test from inside the folder, with a PATH that has nothing but Windows and none of the
  variables that point to OSGeo4W. A DLL that is missing then stops it, and the zip file is
  not made.

For the one who gets it:

- Unpack the whole zip file into a folder of its own first. Started from inside the zip
  file, Windows does not find the DLLs.
- The program is not signed. Windows SmartScreen may say that it protected the computer:
  *More info* and *Run anyway*. Where IT only lets signed programs or programs in certain
  folders run (AppLocker), ask IT where it may lie.
- Settings are kept in `%APPDATA%\Ographis`, as for a built program.

This has been used on Windows 11 with OSGeo4W's GDAL 3.13.3 since version 0.86. The reading
of the import tables has also been checked against pefile on Windows DLLs.

## If something goes wrong

| Message | Cause | What to do |
| --- | --- | --- |
| `.\build.cmd is not recognized` | The window is not in the project folder | `cd` to the folder that holds `build.cmd`. The zip file contains a folder of its own, so it may be one level down |
| `No GDAL was found` | OSGeo4W is not installed, or is in another folder | Do step 2, or point out the folder with `-Gdal` |
| `build.ps1 cannot be loaded because running scripts is disabled` | A group policy stops scripts | Follow [Building by hand](#building-by-hand) |
| `failed to remove file …\target\release\ographis.exe`, *Access is denied* (`os error 5`) | Ographis is running, and Windows keeps the file it runs from | Close Ographis (and `og` if it runs) and build again. `build.cmd` now says so before it builds |
| `cargo` is not found | The window was opened before the installation | Open a new PowerShell window |
| `link.exe` is not found, or `LNK1181` | The C++ tools are missing | Install *Desktop development with C++* |
| The download of dependencies stops at `CRYPT_E_NO_REVOCATION_CHECK` or a certificate error | A proxy that inspects the traffic | `$env:CARGO_HTTP_CHECK_REVOKE = "false"` and run again. Set `HTTPS_PROXY` if a proxy has to be given |
| `windows-msvc requires gdal_i.lib to be present` | `GDAL_HOME` points to the wrong place, or `gdal-devel` is missing | Check with `Test-Path` in step 2 |
| `No GDAL version detected` | `GDAL_VERSION` is not set | Set it, see [Building by hand](#building-by-hand) |
| `No pre-built bindings available for GDAL version` | `GDAL_VERSION` is set to something newer than 3.12 | Set `3.12.0` |
| Unresolved symbols at link time (`LNK2019`) that begin with `GDAL`, `OGR` or `OSR` | The library lacks a function that the bindings expect | Get in touch with the error output. The fallbacks are below |
| The program does not start, `gdal….dll was not found` | Started without the launcher | Start with `Ographis.cmd` |
| `doctor`: PROJ cannot find proj.db, or complains about `DATABASE.LAYOUT.VERSION` | Another `proj.db` is being used, often from PostGIS | Start with `og.cmd`, which points to the right folder |
| `Cannot open … .xlsx as vector data`, or `This GDAL build cannot write .xlsx` | GDAL was built without the Excel driver. `doctor` shows a dash at Excel | Use the GDAL from OSGeo4W, which has it |
| `Cannot save … .xlsx. Is the file open in Excel?` | Excel locks a workbook that is open | Close the workbook in Excel and run again |
| `Cannot fetch https://…` | The address cannot be reached, often because of a proxy | Open the address in a web browser on the same computer. If it works there, check the Windows proxy setting, or set `HTTPS_PROXY` |
| *Show in folder* opens the Documents folder | Explorer did not understand the path | Get in touch with the path of the file. The path is also in the navigator |
| The program opens no window | The graphics driver lacks DirectX 12, for instance in some remote desktops | Try `$env:WGPU_BACKEND = "opengl"` or `"vulkan"` before starting |
| The map shows a grid although there is a network | The tile server cannot be reached, often because of a proxy | The program reads the Windows proxy setting. Check it, or set `HTTPS_PROXY` |

## What has been checked

On Windows 11, by the first user:

- Version 0.2.1: `build.cmd` found Rust, the Visual Studio 18 Build Tools and OSGeo4W's
  GDAL 3.13.3, built both programs in about four minutes, and wrote the launchers. `doctor`
  passed on all points and the build test model ran. The program started through the
  launcher and drew its window with the system fonts.
- Versions 0.3 and 0.4 have been built and used. What was tried in them is not known in
  detail.
- Version 0.5: `build.cmd` built with three warnings from the canvas library in `vendor`,
  `doctor` answered `OK` on all points, Excel and PostGIS included, and the build test ran.
  Version 0.5.1 took away what the warnings were about. Whether the build is free from
  warnings on Windows since then has not been reported.
- Version 0.86: `build.cmd -Package` made the folder and the zip file, and the slides of
  Slides opened in PowerPoint.

Not yet checked on Windows:

- Version 0.91, all built and tested only on Linux: Model → *Schedule…* making, finding and
  removing a task with `schtasks`, and the scheduled run starting `scheduled run.cmd` (with
  OSGeo4W's GDAL found through og.cmd); the notice of Windows when a long run ends (a toast
  through PowerShell, which some organisations turn off; the taskbar flashes either way);
  Ctrl and + or − on a screen with scaling; the screen reader of Windows (Narrator) reading
  the nodes and what they are connected to; a PMTiles file written by Ographis on a web server
  in MapLibre, and opened in QGIS; glTF with groups, materials, sun and camera imported into
  Blender 4 and 5, and BlenderGIS reading where it lies; GPX from a GPS and photos from a
  phone; PostGIS *Empty it and fill it again* against a table with views on it.
- Version 0.90: that the build compiles acadrust; DWG drawings of the workplace from AutoCAD,
  compared with the same drawing saved as DXF; Overture Maps download against the real
  catalog through the proxy, and how many megabytes a small rectangle of buildings fetches;
  GeoParquet written by Ographis opened in QGIS; Check for updates against GitHub, and Report a
  problem opening the browser; the start screen in the packed folder, where the examples are
  beside the program.
- Version 0.89: Raster to areas with the GDAL of OSGeo4W; a LAZ written by Ographis opened in
  QGIS or CloudCompare, with its coordinate system; Point cloud to raster and Height above
  ground with Lantmäteriet's Laserdata NH over a whole sheet, and how long Visible area takes
  with a 1 m terrain model.
- Version 0.88: background maps against real services, such as WMS and WMTS from
  Lantmäteriet with a user and password, through the proxy; vector tiles (.mbtiles,
  .pmtiles) with the GDAL 3.13 of OSGeo4W; Ctrl+Shift+C putting the picture of the model on
  the clipboard of Windows; the double click and `/` of quick add with a real mouse; Copy
  coordinates pasted into another program.
- Version 0.87.1: the OpenTripPlanner example against the OpenTripPlanner 2.4 of the
  workplace, and the log in `otp_routes_and_departures.runs\logs` while it runs.
- Version 0.87: passwords and keys in the Windows Credential Manager (Control Panel →
  Credential Manager → Windows Credentials, one for each connection, such as
  `db:name.Ographis`), and the move of those
  kept in the settings file the first time; Input and Output to SQL Server through ODBC
  Driver 17 or 18, with a password and with the login of Windows, a primary key and the
  spatial index; OpenStreetMap download against overpass-api.de; the log when a frame of
  the window is slow, with a GeoPackage of some 400 MB; the example for OpenTripPlanner 2.4
  against a server of its own (checked here against one that answers as it does).
- Version 0.86: the page of formats written by
  `og formats --html` in the packed folder, with the GDAL from OSGeo4W; File → Save the
  model as a picture with the dialog of Windows.
- Version 0.85: how long a diorama with thousands of trees takes to render on an ordinary
  work computer, and the title in Render 3D scene with the fonts of Windows.
- Version 0.84: how long Make diorama takes with Lantmäteriet's land cover for a whole
  municipality (the rounding buffers every area three times), and the diorama glTF opened
  in Blender.
- Version 0.83: how long a large Revit IFC (Trapelo, 26 MB) takes to read in the release build,
  and how the 3D view turns with 1.5 million triangles on an ordinary work computer.
- Version 0.82: IFC files of the workplace with curved shapes from Revit (advanced breps) and
  how long they take; picking in the 3D view on a large model; Stop during Generate TIN and
  Render 3D scene on an ordinary work computer.
- Version 0.81: that the build compiles the Parquet library with its compressions (zstd and
  lz4 come as C and are built by the Visual Studio C++ tools); the lock of a running model
  checked with tasklist, and `og run` from the Task Scheduler ending with exit code 4 when
  the model is running already; the run history on a shared drive; DXF drawings of the
  workplace from AutoCAD, MicroStation and Lantmäteriet's exports, and CityGML from the
  municipalities (Göteborg's 3D city model); GDAL from OSGeo4W reading FlatGeobuf and
  SpatiaLite.
- Version 0.80: IFC files of the workplace from Revit, Archicad and Tekla: what is read and
  what is missing, how they are placed, and how long a large one takes; labels and icons
  in Draw map at large sizes for print.
- Version 0.79: a primary key written to the PostGIS of the workplace; a large PDF (A1, A0)
  from Draw map and how long it takes; glTF from Extrude opened in other programs, such as
  Blender, QGIS 3D and FME; how long a dusk picture with many lamps takes.
- Version 0.78: how long Render 3D scene takes on an ordinary work computer with clouds,
  water, reflections and three bounces; a Cloud Optimized GeoTIFF and a COPC point cloud
  read from a server through the proxy; the colour picker of the group boxes.
- Version 0.77: that the build compiles meshoptimizer, which comes as C++ with the library
  `meshopt` and is built by the Visual Studio C++ tools like the rest; LAZ files from
  Lantmäteriet's Laserdata NH, how many points are read in what time and memory; 3DBAG and
  other 3D Tiles 1.1 through the proxy; HTTP request with several requests at once against
  an OpenTripPlanner of the workplace.
- Version 0.76: 3D Tiles from a tileset on a disk of the workplace and from a server, through
  the proxy; how large a tileset of a city can be read and shown in 3D; and the program
  after its code was split into smaller files (nothing should look or act differently).
- Version 0.75: the new name. That `build.cmd` writes `Ographis.cmd` and `og.cmd`, that
  the settings, connections and latest models in `%APPDATA%\GeoBobbo` are taken over into
  `%APPDATA%\Ographis` the first time, and that `.ographis` files are offered in the open
  and save windows. Reading a points or multilinestrings layer from a whole country of
  OpenStreetMap (`sweden-latest.osm.pbf`), and how much memory it takes. Marking and copying
  text in the log.
- Version 0.74: Run model without waiting, which starts og.exe beside ographis.exe; and
  that the arrows → and ← of the log are drawn in the font of Windows.
- Version 0.73: the API examples against the real services (SCB, Eurostat, Kolada,
  Trafikverket, Open-Meteo, OGC API, ArcGIS, STAC, Nominatim) behind the proxy of the
  workplace, and an OAuth2 connection to Lantmäteriet.
- Version 0.72: the crash log and restoring changes after a stop, on Windows: that
  %APPDATA%\Ographis\crash-log.txt gets the file and line, which needs the .pdb file beside
  the .exe, and that the offer comes after ending Ographis in the Task Manager.
- Version 0.71: Render 3D scene with a camera distance and terrain colours on a large
  terrain model, and models moved up and down.
- Version 0.70: the full path of the model in the title bar, and a View with a model and
  a large terrain together, in the map and in 3D.
- Version 0.69: glTF models from web shops and design programs, with turned and scaled
  textures.
- Version 0.68: glTF from the 3D programs in use at the workplace, and large .glb files.
- Version 0.67: how fast Render 3D scene is on the cores of a work computer, and the
  Swedish time of the sun against a known shadow.
- Version 0.66: the 3D view and dragging bend points with the mouse in Windows, and
  reading NVDB within a rectangle.
- Version 0.65: Shortest route on all of NVDB for a county, and Run SQL through the
  network of the workplace.
- Version 0.64 as a whole: dragging everything with the group boxes at 125 % and 150 % in
  Windows, group boxes pasted into another window, POST to a real web service through the
  proxy of the workplace, and the legend and the turned names in Inter next to Segoe UI.
- Version 0.63 as a whole: the settings window and settings.json in %APPDATA%\Ographis,
  the JSON from the open data of Västra Götalandsregionen fetched through the proxy of the
  workplace, links in comments opened in the default browser, the build date in About, and
  the larger hit area of the ports with a real mouse and a touchpad.
- Version 0.62 as a whole: the PostGIS window listing tables from a real server while the
  window stays responsive, Output to PostGIS with a schema, a geometry type and a coordinate
  system, Draw chart with the built-in Inter font next to Segoe UI, the format list of Output
  switching to GeoSpatial PDF and PostGIS, and Render 3D view as a block after Reproject.
- Version 0.61 as a whole: PostGIS input against a real server, OpenStreetMap from an
  extract (`.osm.pbf`), Check geometry with repair (needs GEOS in the GDAL of OSGeo4W),
  contour lines, Reproject of grids and pictures, Merge rasters, and PDF with layers opened
  in Acrobat or another reader that shows layers.
- Version 0.60 as a whole: rasters read as grids, also a folder of tiles, Shade terrain and
  Render 3D view, how long they take on the cores of the machine, and the pictures written
  as PNG, GeoTIFF and PDF. A 3D view as PNG has no world file.
- Version 0.59 as a whole: Show in folder on the canvas, copy and paste between two
  windows, files dropped where the pointer is (on Windows the place is asked of the
  system, since Windows sends no pointer movements while files are dragged), the
  statistics, the title of a PDF placed left, centre or right, and the new tools.
- Version 0.58 as a whole: the menus by pointer and keyboard (Alt+letter, F10, the
  arrows), the cache marks, the new tools, and `og run` with a folder, `--log` and the exit
  codes, also from the Task Scheduler. *Copy command line* looks for `og.cmd` a few
  folders above the program, which is where `build.cmd` writes it.
- Version 0.57 as a whole: Pause and F6, the cache with Run from here and Run to here,
  Scale, Move by x and y, Geometry to 2D, PDF output, and closing at once after a large
  run. `og doctor` checks the PDF driver too. OSGeo4W's GDAL has it, and writing PDF needs
  no extra library.
- Version 0.56 as a whole: Stop and Shift+F5, comment boxes, group boxes resized from every
  side, Create points from x and y, Draw map, and PNG, JPG and GeoTIFF written as pictures.
  `og doctor` now also checks that the PNG and JPEG drivers are in the GDAL build;
  OSGeo4W's GDAL has both.
- Version 0.55 as a whole: the map with millions of features, the log while a run goes on,
  adding to files that are there, variables, HTTP request and the other new tools. The
  web requests go through the proxy and certificates of Windows, as fetching files does.
- Version 0.54 as a whole: the icons and menus, Open recent with its file under
  `%APPDATA%\Ographis`, the menu of the canvas, ports that move, Aggregate, Deaggregate,
  Set coordinate system and Generalize that keeps neighbours together.
- Version 0.53. What is new in it has only been run on Linux: Input and Output,
  the text about the program when nothing is selected, LibreOffice Calc, styled workbooks,
  GeoTIFF, *Round numbers*, *Split attribute*, *Filter by value* and the list in *Manage
  attributes*. The two new libraries that write workbooks are pure Rust and need nothing
  from the system, but have not been built with the Visual Studio tools.
- Version 0.52 has not been reported on.
- *Show in folder*. It starts `explorer.exe /select,"…"`, which could not be run here.
- Fetching from the web through the proxy of an organisation. The program uses the
  proxy setting and the certificates of Windows, as it does for the map background.
- The OpenStreetMap background, and `build.cmd -Test`.

On Linux:

- All tests pass against both GDAL 3.8.4 and a self-built GDAL 3.13.3, the latter with the
  bindings for 3.12 as on Windows.
- The C declarations the program uses were compared between GDAL 3.12 and 3.13. None of
  the changes affect how the functions are called.

## Fallbacks

Both are untried:

- **Build GDAL from source inside Cargo.** `gdal-sys` has the feature `bundled`, which
  pulls in `gdal-src` with GDAL 3.12. CMake and a C++ compiler are then needed, but no
  installed GDAL. Which formats are included is decided by feature flags that have to be chosen.
- **Replace the backend.** The engine (`og-engine`) knows nothing about GDAL. Everything
  goes through the interface `Backend` in `crates/og-engine/src/io.rs`: read, write,
  reproject coordinates. A backend in pure Rust for just GeoPackage, Shapefile and the
  SWEREF zones can replace `og-gdal` without changing the tools.

In the meantime everything works in WSL: `sudo apt install libgdal-dev` and then `cargo build`.

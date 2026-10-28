# Builds Ographis on Windows 11.
#
#   .\build.cmd                     finds GDAL, builds and runs the build test
#   .\build.cmd -Gdal D:\GDAL       points out the GDAL folder yourself
#   .\build.cmd -Test               also runs all tests
#   .\build.cmd -Package            also makes a folder and a zip file to give to others,
#                                   with the program, the DLLs it needs and its data, in dist
#   .\build.cmd -Package -WithPlugins  with GDAL's plugins too, such as Parquet
#
# The script changes nothing in the system. The environment variables only apply while it
# runs, and the only files written outside the target folder are the launchers
# Ographis.cmd and og.cmd.

param(
    [string]$Gdal = "",
    [switch]$Test,
    [switch]$Package,
    [switch]$WithPlugins
)

$ErrorActionPreference = "Stop"
$root = $PSScriptRoot
$onWindows = ($env:OS -eq "Windows_NT")
$exe = ""
if ($onWindows) { $exe = ".exe" }

# The Rust binding (gdal-sys 0.12) has ready-made bindings for GDAL 3.4 to 3.12.
$oldestMinor = 4
$newestMinor = 12

function Step($text) { Write-Host ""; Write-Host "== $text" -ForegroundColor Cyan }
function Note($text) { Write-Host "   $text" }
function Warn($text) { Write-Host "   NOTE: $text" -ForegroundColor Yellow }
function Fail($text) {
    Write-Host ""
    Write-Host "STOP: $text" -ForegroundColor Red
    exit 1
}
function Sub($a, $b) { Join-Path $a $b }
function FirstExisting($paths) {
    foreach ($p in $paths) { if ($p -and (Test-Path $p)) { return $p } }
    return $null
}

# ---------------------------------------------------------------- 1. Rust
Step "1. Rust"
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Fail "cargo was not found. Install Rust from https://rustup.rs and then open a new window."
}
$rustHost = (& rustc -vV | Where-Object { $_ -like "host:*" }) -replace "^host:\s*", ""
Note "rustc for $rustHost"
if ($onWindows -and ($rustHost -notlike "*msvc*")) {
    Warn "This is not the MSVC flavour of Rust. Run: rustup default stable-x86_64-pc-windows-msvc"
}
if ($onWindows) {
    $vswhere = Sub ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    $vc = $null
    if (Test-Path $vswhere) {
        $vc = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    }
    if ($vc) { Note "The C++ tools are in $vc" }
    else { Warn "The Visual Studio C++ tools were not found. Without them the build stops at link.exe." }
}

# ---------------------------------------------------------------- 2. GDAL
Step "2. GDAL"
$candidates = @($Gdal, $env:GDAL_HOME, $env:OSGEO4W_ROOT, "C:\OSGeo4W", "C:\OSGeo4W64")
$gdalHome = $null
$withoutDevel = $null
foreach ($c in $candidates) {
    if (-not $c -or -not (Test-Path $c)) { continue }
    if (Test-Path (Sub (Sub $c "lib") "gdal_i.lib")) { $gdalHome = (Resolve-Path $c).Path; break }
    if (-not $withoutDevel) { $withoutDevel = $c }
}
if (-not $gdalHome) {
    if ($Gdal -and -not (Test-Path $Gdal)) { Fail "The folder $Gdal does not exist." }
    if ($withoutDevel) {
        Fail ("GDAL is in $withoutDevel, but the development files are missing (lib\gdal_i.lib). " +
            "Run the OSGeo4W installer again, choose Advanced Install and add the package gdal-devel.")
    }
    Fail ("No GDAL was found. Install OSGeo4W with the package gdal-devel (see docs\BUILD-WINDOWS.md), " +
        "or point out the folder: .\build.cmd -Gdal <folder>")
}
Note "GDAL folder: $gdalHome"
$gdalBin = Sub $gdalHome "bin"

# The GDAL DLLs must be found, both by gdalinfo below and by the finished programs.
$env:PATH = $gdalBin + [IO.Path]::PathSeparator + $env:PATH

$version = $null
$gdalinfo = FirstExisting @((Sub $gdalBin "gdalinfo.exe"), (Sub $gdalBin "gdal\apps\gdalinfo.exe"))
if ($gdalinfo) {
    # Windows PowerShell turns anything a program writes to stderr into errors when it is
    # redirected, and with "Stop" the script would end. So errors are let through just here.
    $ErrorActionPreference = "Continue"
    try { $text = (& $gdalinfo --version 2>$null | Out-String) } catch { $text = "" }
    $ErrorActionPreference = "Stop"
    if ($text -match "GDAL (\d+)\.(\d+)\.(\d+)") { $version = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3]) }
}
if (-not $version) {
    # OSGeo4W names the DLL after the version: gdal313.dll is 3.13.
    $dll = Get-ChildItem -Path $gdalBin -Filter "gdal*.dll" -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match "^gdal(\d)(\d\d)\.dll$" } | Select-Object -First 1
    if ($dll -and ($dll.Name -match "^gdal(\d)(\d\d)\.dll$")) { $version = @([int]$Matches[1], [int]$Matches[2], 0) }
}
if (-not $version) { Fail "The GDAL version could not be read in $gdalBin. Neither gdalinfo.exe nor gdalNNN.dll gave an answer." }
$installed = "{0}.{1}.{2}" -f $version[0], $version[1], $version[2]
Note "Installed version: $installed"

if ($version[0] -ne 3 -or $version[1] -lt $oldestMinor) {
    Fail "GDAL $installed is too old. 3.$oldestMinor or newer is needed."
}
$bindings = $installed
if ($version[1] -gt $newestMinor) {
    # The C interface of GDAL grows between versions, but what is there stays, so the
    # bindings for 3.12 can be used against a newer library. Tried against 3.13.
    $bindings = "3.$newestMinor.0"
    Warn "GDAL $installed is newer than the bindings support. Building with the bindings for 3.$newestMinor."
}

$projDir = FirstExisting @((Sub $gdalHome "share\proj"), (Sub $gdalBin "proj9\share"), (Sub $gdalBin "proj\share"))
if ($projDir -and -not (Test-Path (Sub $projDir "proj.db"))) { $projDir = $null }
if (-not $projDir) {
    $db = Get-ChildItem -Path $gdalHome -Recurse -Filter "proj.db" -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($db) { $projDir = $db.DirectoryName }
}
if (-not $projDir) { Fail "proj.db was not found under $gdalHome. Without it coordinates cannot be reprojected." }
Note "PROJ data: $projDir"

$dataDir = FirstExisting @((Sub $gdalHome "apps\gdal\share\gdal"), (Sub $gdalHome "share\gdal"), (Sub $gdalBin "gdal-data"))
if ($dataDir) { Note "GDAL data: $dataDir" } else { Warn "The GDAL data folder was not found. Most things work anyway." }
$pluginDir = FirstExisting @((Sub $gdalHome "apps\gdal\lib\gdalplugins"), (Sub $gdalBin "gdal\plugins"))

# Other programs, PostGIS for one, like to set these variables for the whole computer.
# GDAL then reads the wrong proj.db. So all of them are set, even when they have a value.
$env:GDAL_HOME = $gdalHome
$env:GDAL_VERSION = $bindings
$env:PROJ_DATA = $projDir
$env:PROJ_LIB = $projDir
if ($dataDir) { $env:GDAL_DATA = $dataDir } else { Remove-Item Env:GDAL_DATA -ErrorAction SilentlyContinue }
if ($pluginDir) { $env:GDAL_DRIVER_PATH = $pluginDir } else { Remove-Item Env:GDAL_DRIVER_PATH -ErrorAction SilentlyContinue }

# ---------------------------------------------------------------- 3. Build
Step "3. Building (the first time takes a while)"
# A program that is running from target\release cannot be built over: Windows keeps the file.
$built = Sub (Sub $root "target") "release"
$open = Get-Process -Name ographis, og -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($built, [System.StringComparison]::OrdinalIgnoreCase) }
if ($open) {
    Fail "Ographis is running from this folder ($(($open | ForEach-Object { $_.ProcessName + '.exe' }) -join ', ')). Close it, and run build.cmd again."
}
Push-Location $root
try {
    & cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail "The build did not go through. The error is above. See the table at the end of docs\BUILD-WINDOWS.md." }
    if ($Test) {
        Step "Tests"
        & cargo test --release
        if ($LASTEXITCODE -ne 0) { Fail "At least one test did not pass." }
    }
}
finally { Pop-Location }

# ---------------------------------------------------------------- 4. Launchers
Step "4. Launchers"
$release = Sub (Sub $root "target") "release"
$lines = @(
    "@echo off",
    "rem Written by build.ps1. Run build.cmd again if GDAL is moved or updated.",
    "setlocal",
    "set `"PATH=$gdalBin;%PATH%`"",
    "set `"PROJ_DATA=$projDir`"",
    "set `"PROJ_LIB=$projDir`"",
    "set `"GDAL_DATA=$dataDir`"",
    "set `"GDAL_DRIVER_PATH=$pluginDir`""
)
# Paths with letters outside ASCII need cmd to read the file as UTF-8.
if (($lines -join "") -match "[^\x00-\x7F]") { $lines = @($lines[0], "chcp 65001 >nul") + $lines[1..($lines.Count - 1)] }
$utf8 = New-Object System.Text.UTF8Encoding($false)
$gui = $lines + @("start `"`" `"%~dp0target\release\ographis.exe`" %*")
$cli = $lines + @("`"%~dp0target\release\og.exe`" %*", "exit /b %ERRORLEVEL%")
[IO.File]::WriteAllText((Sub $root "Ographis.cmd"), ($gui -join "`r`n") + "`r`n", $utf8)
[IO.File]::WriteAllText((Sub $root "og.cmd"), ($cli -join "`r`n") + "`r`n", $utf8)
Note "Ographis.cmd  starts the program"
Note "og.cmd        runs the command line"

# ---------------------------------------------------------------- 5. Build test
Step "5. Build test"
$og = Sub $release ("og" + $exe)
& $og doctor
if ($LASTEXITCODE -ne 0) { Fail "The program was built, but the build test did not pass. The lines above show what is missing." }
Push-Location $root
try {
    & $og run (Sub "examples" "buildtest.ographis")
    if ($LASTEXITCODE -ne 0) { Fail "The example model could not be run." }
}
finally { Pop-Location }

# ---------------------------------------------------------------- 6. Package
# A folder that runs on a computer without OSGeo4W: the programs, the DLLs they use found by
# reading the imports of each file, PROJ and GDAL data, the examples and the licences.

# The DLLs a program or DLL asks for, from its import table and its delay-load table.
function Get-DllImports([string]$path) {
    $b = [IO.File]::ReadAllBytes($path)
    if ($b.Length -lt 64 -or $b[0] -ne 0x4D -or $b[1] -ne 0x5A) { return @() }
    $pe = [BitConverter]::ToInt32($b, 0x3C)
    if ([BitConverter]::ToUInt32($b, $pe) -ne 0x4550) { return @() }
    $coff = $pe + 4
    $nSections = [BitConverter]::ToUInt16($b, $coff + 2)
    $optSize = [BitConverter]::ToUInt16($b, $coff + 16)
    $opt = $coff + 20
    $wide = ([BitConverter]::ToUInt16($b, $opt) -eq 0x20B)
    if ($wide) { $dirs = $opt + 112; $imageBase = [BitConverter]::ToUInt64($b, $opt + 24) }
    else { $dirs = $opt + 96; $imageBase = [uint64][BitConverter]::ToUInt32($b, $opt + 28) }
    $sections = $opt + $optSize
    $toOffset = {
        param([uint64]$rva)
        for ($i = 0; $i -lt $nSections; $i++) {
            $sec = $sections + $i * 40
            $va = [uint64][BitConverter]::ToUInt32($b, $sec + 12)
            $size = [uint64][Math]::Max([BitConverter]::ToUInt32($b, $sec + 8), [BitConverter]::ToUInt32($b, $sec + 16))
            if ($rva -ge $va -and $rva -lt $va + $size) { return [int64]($rva - $va + [BitConverter]::ToUInt32($b, $sec + 20)) }
        }
        return [int64]-1
    }
    $text = {
        param([int64]$at)
        $end = $at
        while ($end -lt $b.Length -and $b[$end] -ne 0) { $end++ }
        return [Text.Encoding]::ASCII.GetString($b, $at, $end - $at)
    }
    $names = @()
    # Ordinary imports: 20 bytes each, the name at 12.
    $rva = [BitConverter]::ToUInt32($b, $dirs + 8)
    if ($rva -ne 0) {
        $at = & $toOffset $rva
        while ($at -ge 0 -and $at + 20 -le $b.Length) {
            $nameRva = [BitConverter]::ToUInt32($b, $at + 12)
            if ($nameRva -eq 0) { break }
            $o = & $toOffset $nameRva
            if ($o -ge 0) { $names += (& $text $o) }
            $at += 20
        }
    }
    # Delay-loaded: 32 bytes each, the name at 4, as an address in old files.
    $rva = [BitConverter]::ToUInt32($b, $dirs + 13 * 8)
    if ($rva -ne 0) {
        $at = & $toOffset $rva
        while ($at -ge 0 -and $at + 32 -le $b.Length) {
            $attr = [BitConverter]::ToUInt32($b, $at)
            $nameRva = [uint64][BitConverter]::ToUInt32($b, $at + 4)
            if ($nameRva -eq 0) { break }
            if (($attr -band 1) -eq 0) { $nameRva = $nameRva - $imageBase }
            $o = & $toOffset $nameRva
            if ($o -ge 0) { $names += (& $text $o) }
            $at += 32
        }
    }
    return $names
}

if ($Package) {
    Step "6. Package"
    $appVersion = "0.0.0"
    $cargoText = Get-Content (Sub $root "Cargo.toml") -Raw
    if ($cargoText -match '(?m)^version\s*=\s*"([^"]+)"') { $appVersion = $Matches[1] }
    $name = "Ographis-$appVersion-windows"
    $distRoot = Sub $root "dist"
    $dist = Sub $distRoot $name
    if (Test-Path $dist) { Remove-Item $dist -Recurse -Force }
    New-Item -ItemType Directory -Path $dist | Out-Null
    foreach ($p in @("ographis.exe", "og.exe")) { Copy-Item (Sub $release $p) $dist }

    # The DLLs: those in GDAL's bin folder are copied, and what they ask for in turn. Those
    # of Windows are left. The runtime of Visual C++ comes along, as Microsoft allows, so
    # that the folder also runs where it is not installed.
    $system = Sub $env:windir "System32"
    $runtime = '^(vcruntime140.*|msvcp140.*|concrt140|vcomp140)\.dll$'
    $seen = @{}
    $missing = @()
    $queue = New-Object System.Collections.Generic.Queue[string]
    foreach ($p in @("ographis.exe", "og.exe")) { $queue.Enqueue((Sub $dist $p)) }
    if ($WithPlugins -and $pluginDir) {
        $plugins = Sub $dist "gdalplugins"
        New-Item -ItemType Directory -Path $plugins | Out-Null
        Get-ChildItem $pluginDir -Filter "*.dll" | ForEach-Object {
            Copy-Item $_.FullName $plugins
            $queue.Enqueue((Sub $plugins $_.Name))
        }
    }
    while ($queue.Count -gt 0) {
        $file = $queue.Dequeue()
        foreach ($dll in (Get-DllImports $file)) {
            $key = $dll.ToLowerInvariant()
            if ($seen.ContainsKey($key)) { continue }
            $seen[$key] = $true
            if ($key -like "api-ms-*" -or $key -like "ext-ms-*") { continue }
            $fromGdal = Sub $gdalBin $dll
            if (Test-Path $fromGdal) {
                Copy-Item $fromGdal $dist
                $queue.Enqueue((Sub $dist $dll))
            } elseif ($key -match $runtime -and (Test-Path (Sub $system $dll))) {
                Copy-Item (Sub $system $dll) $dist
            } elseif (-not (Test-Path (Sub $system $dll))) {
                $missing += "$dll (asked for by $(Split-Path $file -Leaf))"
            }
        }
    }
    $dllCount = (Get-ChildItem $dist -Filter "*.dll").Count
    Note "$dllCount DLLs copied"
    foreach ($m in $missing) { Warn "Not found: $m" }

    # PROJ: the database and the small files, not the grids of other countries.
    $proj = Sub $dist "proj"
    New-Item -ItemType Directory -Path $proj | Out-Null
    Get-ChildItem $projDir -File | Where-Object { $_.Extension -notin @(".tif", ".gtx", ".gsb", ".byn") -and $_.Length -lt 50MB } |
        ForEach-Object { Copy-Item $_.FullName $proj }
    if ($dataDir) { Copy-Item $dataDir (Sub $dist "gdal-data") -Recurse }
    $certs = FirstExisting @((Sub $gdalBin "curl-ca-bundle.crt"), (Sub $gdalHome "share\curl\curl-ca-bundle.crt"), (Sub $gdalHome "bin\curl-ca-bundle.crt"))
    if ($certs) { Copy-Item $certs (Sub $dist "curl-ca-bundle.crt") }

    # The examples, without what earlier runs wrote, and what to read.
    Copy-Item (Sub $root "examples") (Sub $dist "examples") -Recurse
    Get-ChildItem (Sub $dist "examples") -Directory -Recurse | Where-Object { $_.Name -in @("out", "http-cache", "answers") } |
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue
    foreach ($f in @("README.md", "LICENSE-MIT", "LICENSE-APACHE", "THIRD-PARTY-LICENSES.md")) { Copy-Item (Sub $root $f) $dist }
    $licences = Sub $dist "licences-of-gdal-and-its-libraries"
    New-Item -ItemType Directory -Path $licences | Out-Null
    # OSGeo4W keeps them with each package. Python, Qt, GRASS and QGIS are not in the folder.
    $docs = @((Sub $gdalHome "share\doc"), (Sub $gdalHome "apps\gdal"), (Sub $gdalHome "doc")) | Where-Object { Test-Path $_ }
    Get-ChildItem $docs -Recurse -File -Include "LICENSE*", "COPYING*" -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch "\\(python|qt|grass|qgis)" } | Select-Object -First 400 |
        ForEach-Object {
            $rel = $_.FullName.Substring($gdalHome.Length).TrimStart("\").Replace("\", "_")
            Copy-Item $_.FullName (Sub $licences $rel)
        }
    $readme = @(
        "Ographis $appVersion, to try out",
        "",
        "Start: double-click ographis.exe. The examples are in the folder examples:",
        "File > Open, for example examples\skaraborg.ographis, and press F5 to run it.",
        "",
        "Unpack the whole zip file first, into a folder of its own. Started from inside the zip",
        "file it does not find its DLLs. If Windows says it protected the computer, click",
        "More info and Run anyway: the program is not signed.",
        "",
        "The command line: og.exe doctor checks that everything is in place, and",
        "og.exe run examples\buildtest.ographis runs a model.",
        "",
        "FORMATS.html lists the formats Ographis reads and writes, and what handles each.",
        "",
        "Nothing is installed. To remove it, delete the folder. Settings are kept in",
        "%APPDATA%\Ographis.",
        "",
        "Built with GDAL $installed from OSGeo4W. Their licences are in the folder",
        "licences-of-gdal-and-its-libraries, those of the Rust libraries in THIRD-PARTY-LICENSES.md."
    )
    [IO.File]::WriteAllText((Sub $dist "READ-ME-FIRST.txt"), ($readme -join "`r`n") + "`r`n", $utf8)

    # The test: the folder alone, with none of the variables that point to OSGeo4W, and a
    # PATH with nothing but Windows.
    Note "Trying the folder on its own"
    $saved = @{}
    foreach ($v in @("PATH", "PROJ_DATA", "PROJ_LIB", "GDAL_DATA", "GDAL_DRIVER_PATH", "GDAL_HOME")) { $saved[$v] = [Environment]::GetEnvironmentVariable($v, "Process") }
    try {
        $env:PATH = "$system;$env:windir"
        foreach ($v in @("PROJ_DATA", "PROJ_LIB", "GDAL_DATA", "GDAL_DRIVER_PATH", "GDAL_HOME")) { Remove-Item "Env:$v" -ErrorAction SilentlyContinue }
        & (Sub $dist "og.exe") doctor
        if ($LASTEXITCODE -ne 0) { Fail "The folder in dist does not run on its own. The lines above show what is missing." }
        # The page of formats, from the program itself, with the GDAL it is packed with.
        & (Sub $dist "og.exe") formats --html (Sub $dist "FORMATS.html")
        if ($LASTEXITCODE -ne 0) { Fail "The page of formats could not be written." }
        Push-Location $dist
        try {
            & (Sub $dist "og.exe") run (Sub "examples" "buildtest.ographis")
            if ($LASTEXITCODE -ne 0) { Fail "The folder in dist could not run the build test." }
        }
        finally { Pop-Location }
        Remove-Item (Sub (Sub $dist "examples") "out") -Recurse -Force -ErrorAction SilentlyContinue
    }
    finally {
        foreach ($v in $saved.Keys) { [Environment]::SetEnvironmentVariable($v, $saved[$v], "Process") }
    }

    $zip = Sub $distRoot "$name.zip"
    if (Test-Path $zip) { Remove-Item $zip -Force }
    Compress-Archive -Path $dist -DestinationPath $zip
    $mb = [Math]::Round((Get-Item $zip).Length / 1MB)
    Note "Folder: $dist"
    Note "Zip file: $zip ($mb MB)"
}

Write-Host ""
Write-Host "Done. Start the program with:" -ForegroundColor Green
Write-Host "   .\Ographis.cmd examples\skaraborg.ographis"
if ($Package) { Write-Host "   or give dist\$name.zip to someone: they unpack it and start ographis.exe" }

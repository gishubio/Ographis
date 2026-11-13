# Performance

How long large data takes, and how much memory, in four fixed cases. The data is made up by
the test itself, the same every time, so that the numbers can be compared between versions:
a version that is slower, or takes more memory, than the one before shows here.

```text
cargo test --release -p og-gdal --test performance -- --ignored --nocapture
```

Each case runs in a process of its own, so that its peak of memory is its own. The data is
made before the clock starts; the time is that of the run of the model, reading and writing
included. The results are printed as the table below and written to
`target/performance.csv`. `OG_PERF_ONLY=raster_slope,cloud_raster` runs only those.

Peak memory is read from Linux (VmHWM). On Windows the time is measured but not the memory,
which is shown as –. A peak written as `<1114` means that making the data took more than the
run, so the run's own is not known, only that it was lower.

| Case | What |
| --- | --- |
| points_join | 1 000 000 points from CSV joined to the 100 squares of 10 × 10 km they lie in, to GeoPackage |
| polygons_buffer | 200 000 small areas from CSV with WKT, their area and a buffer of 2 m, to GeoPackage |
| raster_slope | The slope of 4000 × 4000 heights (16 million cells), GeoTIFF to GeoTIFF |
| cloud_raster | 5 000 000 points from LAZ made a terrain model of 1 m cells, the mean height |
| dissolve_groups | 60 000 overlapping areas of 40 corners, dissolved in 400 blocks by a group attribute (0.96.2) |
| dissolve_districts | The same 60 000 areas dissolved in 120 districts of some 500 areas each (0.96.2) |
| dissolve_one | The same 60 000 areas dissolved into one (0.96.2) |
| dissolve_cover | 24 000 areas of land cover of eight kinds that cover the ground with shared, wavy edges, dissolved by kind (0.96.3) |

## Results

On Linux, two cores of an Intel Xeon at 2.1 GHz and 8 GB of memory, release build.

| Version | points_join | polygons_buffer | raster_slope | cloud_raster |
| --- | --- | --- | --- | --- |
| 0.94 | 6.1 s, 1 114 MB | 3.6 s, 820 MB | 2.5 s, 303 MB | 2.7 s, 208 MB |
| 0.95 | 6.4 s, 678 MB | 3.8 s, 490 MB | 2.8 s, 244 MB | 2.6 s, 197 MB |

In 0.96.2 the heavy vector tools work on every core. On the same two cores:

| Version | points_join | polygons_buffer | dissolve_groups | dissolve_one | raster_slope | cloud_raster |
| --- | --- | --- | --- | --- | --- | --- |
| 0.96.1 | 5.9 s, 723 MB | 3.2 s, 488 MB | 4.6 s, 352 MB | 4.2 s, 1 199 MB | 2.2 s | 2.3 s |
| 0.96.2 | 3.2 s, 750 MB | 2.0 s, 526 MB | 2.2 s, 364 MB | 2.1 s, 390 MB | 2.1 s, 303 MB | 2.2 s, 246 MB |

In 0.96.3 Dissolve finds first which areas touch, and merges clusters that touch rather
than groups. On the same two cores, from a run of all the cases:

| Version | points_join | polygons_buffer | dissolve_groups | dissolve_districts | dissolve_one | dissolve_cover | raster_slope | cloud_raster |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 0.96.3 | 3.8 s, 786 MB | 1.8 s, 526 MB | 2.8 s, 368 MB | 2.5 s, 379 MB | 3.1 s, 394 MB | 2.8 s, 579 MB | 1.9 s, 303 MB | 2.2 s, 246 MB |

Land cover is what gained: the step of Dissolve in dissolve_cover takes 1.6 s, from 2.4 s
with the way of 0.96.2, and the rest of the case is reading the areas from CSV and
writing 18 039 parts. There every area touches its neighbours, so a group of a kind is
neither one cluster nor many apart, and finding the clusters on every core is what lets the
merging be shared. The other cases are within some tenths of a second of 0.96.2, which is
how much the times vary between runs on this computer; dissolve_one may be a little slower,
as finding the clusters is work that a single group does not need. A case run by itself with
`OG_PERF_CASE=dissolve_cover` and `OG_PERF_LOG=1` prints the time of each step;
`OG_PERF_KEEP=1` keeps the folder of the data.

Dissolving into one area now merges pieces of neighbours and then the pieces two by two,
rather than everything at once, and so holds a third of the memory.

Dissolving into a few groups of a few hundred areas each, as by municipality, first went
on one core whatever the computer: a run of groups was handed out whole to one core, and a
group of up to 1024 areas was merged in one go. dissolve_districts, 120 such groups, took
4.0 s with one core busy. Now the runs are as many as the cores can take, the largest
groups go first, and a group is merged in pieces by as many cores; the same case takes
2.7 s on the two cores, and should gain more with more cores. With more cores the
vector cases should go faster still; the raster and point cloud cases did not change.

What they say: a point cloud is kept compactly, about 30 bytes a point, and a raster as the
values of its cells. Features take more: a point with four attributes some 200 bytes, a
small area more. Up to 0.94 each step also kept what it made to the end of the run, and
reading a CSV and writing a GeoPackage each held a copy of the data besides. In 0.95 the
run lets go of what a step made once the steps after it have used it, as `og run` does (the
program keeps it, to show it), CSV is read and written without the copies, and GeoPackage is
written without one: the memory of the two cases of features fell by 40 %. Most of the time of
points_join is GDAL writing the GeoPackage and its spatial index, as long as ogr2ogr takes
for the same.

From 0.95 the cases run as `og run` does, letting go of what each step has made once it is
used. The times vary by some tenths of a second from run to run.

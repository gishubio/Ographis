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

## Results

On Linux, two cores of an Intel Xeon at 2.1 GHz and 8 GB of memory, release build.

| Version | points_join | polygons_buffer | raster_slope | cloud_raster |
| --- | --- | --- | --- | --- |
| 0.94 | 6.1 s, 1 114 MB | 3.6 s, 820 MB | 2.5 s, 303 MB | 2.7 s, 208 MB |

What they say: a point cloud is kept compactly, about 30 bytes a point, and a raster as the
values of its cells. Features are not: a point with four attributes takes about 1 kB, and a
small area some 4 kB, through the run, with copies between the steps. That is where large
vector data runs out of memory first, and what reading large vector files in parts, planned
for 0.95, is to bring down. These numbers are what it will be measured against.

# Test data for 3D Tiles 1.1

- `house_metadata.glb`: the example house of Ographis as a tile of 3D Tiles 1.1, made with
  `3d-tiles-tools upgrade --targetVersion 1.1` from `examples/tiles`, with its name and year
  in a property table (EXT_structural_metadata).
- `house_meshopt.glb`: the same tile packed by `gltfpack -cc`, with EXT_meshopt_compression
  and KHR_mesh_quantization, as tilers such as tyler pack 3D Tiles.
- `quadtree/`: SparseImplicitQuadtree and `complex/`: ComplexTypes, from the samples of 3D
  Tiles by Cesium, https://github.com/CesiumGS/3d-tiles-samples, Apache License 2.0.

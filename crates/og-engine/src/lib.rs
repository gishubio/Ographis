//! Ographis – the engine.
//!
//! The data model, the expression language, the tools and the execution live here.
//! The engine knows nothing about GDAL: file formats and coordinate systems are reached
//! through [`io::Backend`]. CSV and txt are read and written by the engine itself.

pub mod chart;
pub mod citymodel;
pub mod crash;
pub mod csv_io;
pub mod expr;
pub mod formats;
pub mod geom;
pub mod geomtext;
pub mod gltf_io;
pub mod history;
pub mod icons;
pub mod ifc_io;
pub mod io;
pub mod json;
pub mod jsonstat;
pub mod mapdraw;
pub mod mesh;
pub mod mesh_write;
pub mod model;
pub mod modelpic;
pub mod network;
pub mod nodes;
pub mod obj_io;
pub mod parquet_io;
pub mod pointcloud;
pub mod render3d;
pub mod run;
pub mod settings;
pub mod slides;
pub mod sun;
pub mod tanaka;
pub mod terrain;
pub mod thematic;
pub mod tiles3d;
pub mod web;

pub use io::{Backend, Notes};
pub use model::{Feature, Field, FieldType, GbError, Result, Table, Value};
pub use nodes::{node_type, NodeType, NODE_TYPES};
pub use run::{
    downstream_of, node_keys, run, run_opts, run_stoppable, run_with, upstream_of, Edge, GroupBox, Level, Node, NodeResult, Note, Progress, RunOptions,
    RunResult, Status, Variable, Workspace,
};

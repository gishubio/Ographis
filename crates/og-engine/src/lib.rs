//! Ographis – the engine.
//!
//! The data model, the expression language, the tools and the execution live here.
//! The engine knows nothing about GDAL: file formats and coordinate systems are reached
//! through [`io::Backend`]. CSV and txt are read and written by the engine itself.

pub mod basemap;
pub mod bugreport;
pub mod chart;
pub mod citygml_write;
pub mod citymodel;
pub mod clip3d;
pub mod cloud;
pub mod crash;
pub mod csv_io;
pub mod dwg;
mod dwg_curved;
pub mod expr;
pub mod formats;
pub mod geom;
pub mod geomtext;
pub mod gltf_io;
pub mod gsheets;
pub mod history;
pub mod icons;
pub mod ids;
pub mod ifc_io;
pub mod io;
pub mod json;
pub mod jsonstat;
pub mod landxml;
pub mod mapdraw;
pub mod mesh;
pub mod mesh_write;
pub mod model;
pub mod modelpic;
pub mod network;
pub mod nodes;
pub mod obj_io;
pub mod overture;
pub mod parquet_io;
pub mod photos;
pub mod pmtiles_write;
pub mod pointcloud;
pub mod portkinds;
pub mod qgis_project;
pub mod rasterlook;
pub mod reference;
pub mod render3d;
pub mod report;
pub mod road_rules;
pub mod run;
pub mod schedule;
pub mod settings;
pub mod slides;
pub mod splats;
pub mod sun;
pub mod tanaka;
pub mod terrain;
pub mod thematic;
pub mod tiles3d;
pub mod tiles3d_write;
pub mod toolmaking;
pub mod typedefs;
pub mod usd_write;
pub mod vector;
pub mod web;
pub mod xml;

pub use io::{Backend, Notes};
pub use model::{Feature, Field, FieldType, GbError, Result, Table, Value};
pub use nodes::{node_type, NodeType, NODE_TYPES};
pub use run::{
    downstream_of, node_keys, run, run_opts, run_stoppable, run_with, upstream_of, Edge, GroupBox, Level, Node, NodeResult, Note, Progress, RunOptions,
    RunResult, Status, Variable, Workspace,
};

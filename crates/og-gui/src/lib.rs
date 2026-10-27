//! Ographis – graphical user interface. Everything that is drawn lives in the library, so
//! that the tests can run the interface without a window.

pub mod app;
pub mod dbdialog;
pub mod graph;
pub mod icons;
pub mod links;
pub mod map;
pub mod mapdata;
pub mod params;
pub mod recent;
pub mod recovery;
pub mod settings;
pub mod sources;
pub mod stats;
pub mod theme;
pub mod tiles;
pub mod view3d;

pub use app::App;

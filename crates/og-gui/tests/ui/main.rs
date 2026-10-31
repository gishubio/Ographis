//! Runs the interface without a window: loads the example workspaces, clicks, drags and
//! saves screenshots in target/screenshots. Needs GDAL and a graphics driver (software
//! rendering will do).
//!
//! The tests are in a file per area of the program; common.rs has what they share.

mod common;

mod canvas;
mod files;
mod forms_and_menus;
mod map_and_data;
mod release090;
mod release091;
mod running;

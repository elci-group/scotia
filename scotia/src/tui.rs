//! Interactive harness-selection TUI.
//!
//! Split into focused submodules so no single file is a god object:
//! [`app`] holds state, [`render`] draws the frame, [`input`] runs the event
//! loop, and [`detect`] discovers installed agent harnesses.

mod app;
pub mod detect;
mod input;
mod render;

pub use detect::{Harness, detect_harnesses, detect_harnesses_with_path};
pub use input::run_tui;

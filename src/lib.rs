//! Hotwire — the C35 × Blender bridge.
//!
//! Converts Blender-exported glTF binary (`.glb`) into the C35
//! engine's native `.c35m` model format, enforcing the house era
//! law (triangle budgets, flat facets, named materials, honest
//! scale) at the border. See SPEC.md for the full contract.
//!
//! All conversion, validation, and manifest logic in this crate
//! is our own code; the `gltf` crate is used only to parse the
//! interchange file.

pub mod config;
pub mod convert;
pub mod model;
pub mod watch;

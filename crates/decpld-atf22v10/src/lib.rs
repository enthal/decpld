//! The ATF22V10C device model. SPEC.md §4.7.
//!
//! **Every numeric constant in this crate cites the experiment that
//! measured it.** The citations name files in
//! `targets/experiments/atf22v10/`, and the reasoning is written up in
//! `targets/evidence/atf22v10-fuse-map.md`. A number here without a
//! citation is a bug, not a shortcut — a wrong fuse produces a chip that
//! misbehaves in a circuit and a user who debugs their hardware for a
//! week.
//!
//! Nothing in this crate knows about JEDEC syntax. It describes a
//! device; `decpld-jedec` serialises fuse states.

mod geometry;
mod macrocells;
mod matrix;
mod packages;
mod regions;

pub use geometry::{
    ASYNCHRONOUS_RESET_ROW, ArrayCell, Atf22v10Geometry, COLUMNS, MacrocellIndex, ROWS, RowBlock,
    SYNCHRONOUS_PRESET_ROW, SignalSource, SourceKind,
};
pub use macrocells::{MacrocellError, macrocells};
pub use matrix::{and_matrix, bool_input_of_source, macrocell_id, product_term_of_row};
pub use packages::{DIP24, GLOBAL_CLOCK, dip24};
pub use regions::{Footprint, FootprintError, regions_for};

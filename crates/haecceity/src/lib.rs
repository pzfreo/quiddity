//! Haecceity: an exact B-rep geometry kernel in pure Rust, the kernel under quiddity. STEP
//! import, the B-rep model, exact geometry,
//! faces in parameter space, rays and point classification, shared volumes and face coverage,
//! and analytic surfaces recovered from freeform ones.
//!
//! *Haecceity* is "thisness": the particular thing itself, as *quiddity* is "whatness". The
//! kernel holds the exact geometry of this part; quiddity says what kind of features it has.
//!
//! A malformed or unsupported file is refused with a typed error, never a panic: library code
//! may not `unwrap`, `expect`, `panic!` or reach `unreachable!` except at a site with a targeted
//! `allow` stating the invariant that makes it unreachable. Tests may.

#![cfg_attr(
    not(test),
    warn(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unreachable
    )
)]

pub mod anchor;
pub mod brep;
pub mod brep_write;
pub mod classify;
pub mod cloud;
pub mod cover;
pub mod express;
#[cfg(feature = "f3d")]
pub mod f3d;
pub mod geom;
pub mod hlr;
pub mod mass;
pub mod mesh;
pub mod nurbs;
mod orientation;
pub mod overlap;
pub mod p21;
pub mod pmi;
pub mod poly;
pub mod py;
pub mod rays;
pub mod recover;
pub mod sampling;
pub mod step;
pub mod sweep;
pub mod uv;
pub mod validity;
pub mod volume;

pub use brep::Part;
pub use step::{read_step, read_step_file};
pub mod express_rules;
pub mod removal;

/// Shared PMI notation and local callout geometry.
#[cfg(feature = "presentation")]
pub mod presentation;

mod stroke_font;

/// Versioned semantic PMI JSON and its presentation companion.
#[cfg(feature = "json")]
pub mod json;

#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods, clippy::disallowed_types)]

//! Engine-free, deterministic semantic contracts for VEYRA artifacts.

pub mod body;
pub mod canon;
mod capability;
pub mod geometry;
pub mod ids;
pub mod io;
pub mod path;
pub mod sample;
pub mod spatial;
pub mod time;
pub mod views;

/// Version-one feature identifiers understood by this core build.
pub mod features {
    /// Canonical JSON hashed sections.
    pub const JCS_V1: &str = "veyra.canon.jcs/1";
    /// Version-one body roots.
    pub const BODY_V1: &str = "veyra.body/1";
    /// Version-one cube-direction topology.
    pub const DIR_CUBE_V1: &str = "veyra.topo.dir_cube/1";
    /// Version-one radial topology.
    pub const RADIAL_1D_V1: &str = "veyra.topo.radial_1d/1";
    /// Version-one Zstandard and byte-shuffle codec.
    pub const ZSTD_SHUFFLE2_V1: &str = "veyra.codec.zstd-shuffle2/1";
}

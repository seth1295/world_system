//! Core-owned surface geometry derived from the body's declared figure.

use crate::io::Body;
use crate::sample::{LevelSel, Position, SampleError, SampleQuery, TimeSel};
use crate::spatial::{DirCube, TileKey, Topology};
use crate::time::DecimalString;

/// A deterministic, body-fixed surface patch in metres.
#[derive(Clone, Debug, PartialEq)]
pub struct Geometry {
    /// Domain and tile used to produce the patch.
    pub domain: String,
    /// Canonical direction-derived vertices in body-fixed metres.
    pub vertices_m: Vec<[f64; 3]>,
    /// Triangle indices with outward-facing winding.
    pub triangles: Vec<[u32; 3]>,
    /// Side count of the sampled grid.
    pub grid_n: u16,
}

impl Body {
    /// Builds a sphere or star-convex patch using canonical figure authority.
    pub fn domain_geometry(
        &self,
        domain_id: &str,
        tile: TileKey,
        grid_n: u16,
    ) -> Result<Geometry, SampleError> {
        if grid_n == 0 || grid_n > 256 {
            return Err(SampleError::UnsupportedSelection);
        }
        let domain = self
            .domains()
            .iter()
            .find(|domain| domain.id == domain_id)
            .ok_or(SampleError::UnknownDomain)?;
        if domain.topology != "veyra.topo.dir_cube/1" {
            return Err(SampleError::UnsupportedField);
        }
        DirCube.validate_tile_key(tile, domain.tile_log2)?;
        let layout = DirCube.tile_layout_for(tile, domain.tile_log2)?;
        let radius_field = if self.figure().kind == "star_convex_radial" {
            let name = self
                .figure()
                .parameters
                .get("radius_field")
                .and_then(serde_json::Value::as_str)
                .ok_or(SampleError::UnsupportedField)?;
            Some(
                self.fields()
                    .iter()
                    .find(|field| field.name == name && field.domain == domain_id)
                    .ok_or(SampleError::UnsupportedField)?,
            )
        } else {
            None
        };
        if self.figure().kind != "sphere" && self.figure().kind != "star_convex_radial" {
            return Err(SampleError::UnsupportedField);
        }
        let sphere_radius = if self.figure().kind == "sphere" {
            let radius = self
                .figure()
                .parameters
                .get("radius_m")
                .and_then(serde_json::Value::as_str)
                .ok_or(SampleError::UnsupportedField)?;
            Some(
                DecimalString::parse(radius)
                    .and_then(|value| value.to_f64())
                    .map_err(|_| SampleError::InvalidScale)?,
            )
        } else {
            None
        };
        let count = usize::from(grid_n) + 1;
        let mut vertices = Vec::with_capacity(count * count);
        let level_count = (1_u64 << tile.level) as f64;
        for j in 0..=usize::from(grid_n) {
            for i in 0..=usize::from(grid_n) {
                let local_i = i as f64 / f64::from(grid_n);
                let local_j = j as f64 / f64::from(grid_n);
                let s = (layout.i_start as f64 + local_i * layout.edge as f64) / level_count;
                let t = (layout.j_start as f64 + local_j * layout.edge as f64) / level_count;
                let direction = DirCube.direction_at_face_st(layout.face, s, t)?;
                let radius = if let Some(radius) = sphere_radius {
                    radius
                } else {
                    let field = radius_field.ok_or(SampleError::UnsupportedField)?;
                    let sample = self.sample(&SampleQuery {
                        field: field.id,
                        pos: Position::Direction(direction),
                        level: LevelSel::Exact(tile.level),
                        time: TimeSel::Static,
                    })?;
                    sample.value.ok_or(SampleError::InvalidRaster)?
                };
                if !radius.is_finite() || radius <= 0.0 {
                    return Err(SampleError::InvalidRaster);
                }
                vertices.push([
                    direction.x() * radius,
                    direction.y() * radius,
                    direction.z() * radius,
                ]);
            }
        }
        let mut triangles = Vec::with_capacity(usize::from(grid_n) * usize::from(grid_n) * 2);
        for j in 0..usize::from(grid_n) {
            for i in 0..usize::from(grid_n) {
                let a = u32::try_from(j * count + i).map_err(|_| SampleError::InvalidRaster)?;
                let b = a + 1;
                let c = a + u32::try_from(count).map_err(|_| SampleError::InvalidRaster)?;
                let d = c + 1;
                push_outward_triangle(&vertices, &mut triangles, [a, b, c]);
                push_outward_triangle(&vertices, &mut triangles, [b, d, c]);
            }
        }
        Ok(Geometry { domain: domain_id.to_owned(), vertices_m: vertices, triangles, grid_n })
    }
}

fn push_outward_triangle(vertices: &[[f64; 3]], triangles: &mut Vec<[u32; 3]>, triangle: [u32; 3]) {
    let a = vertices[triangle[0] as usize];
    let b = vertices[triangle[1] as usize];
    let c = vertices[triangle[2] as usize];
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let normal = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    if normal[0] * (a[0] + b[0] + c[0])
        + normal[1] * (a[1] + b[1] + c[1])
        + normal[2] * (a[2] + b[2] + c[2])
        >= 0.0
    {
        triangles.push(triangle);
    } else {
        triangles.push([triangle[0], triangle[2], triangle[1]]);
    }
}

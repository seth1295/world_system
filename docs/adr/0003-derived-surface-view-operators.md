# ADR 0003: Derived surface view operators

**Status:** proposed for the Stage 4 implementation

## Context

The canonical architecture names `core.slope/1` and `core.threshold_partition/1`, identifies their capability dependencies, and requires core-owned deterministic views. It does not define the numeric stencil, slope unit, or reference-surface comparison rule.

## Decisions

- `core.slope/1` samples the four cardinal neighbors at the selected stored level. It computes centered U and V gradients using great-circle neighbor-center distances scaled by the mean of the two endpoint radii, then returns `atan(sqrt(du² + dv²))` in radians. Sphere radius is constant; star-convex radius comes from its registered radius field. Nodata at the center or any required neighbor yields nodata.
- `core.threshold_partition/1` resolves the declared field reference surface and the capability's `level_surface`. A scalar value is classified as the positive side when the field's reference-surface radius plus the sampled value is greater than or equal to the level-surface radius; otherwise it is the negative side. The output codes are 1 and 0. Sphere, figure-surface, and bounded `offset_of` references are supported.
- Derived tile samples report `Derived` as their source. Capability-derived views are exposed only when their declared dependency closure and required fields resolve in one compatible domain.

## Consequences

These operators keep their authority in the core and use only declared fields, references, and topology neighbors. The slope numeric unit and threshold equality rule are now deterministic. The choices do not change stored fields, body identity, or frozen topology data.

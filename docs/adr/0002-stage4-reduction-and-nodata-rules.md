# ADR 0002: Stage 4 reduction and nodata rules

**Status:** proposed for the Stage 4 implementation

## Context

The canonical architecture names the integer downsample operators, defines the four-child rounded mean and lowest-value mode tie break, reserves a raw nodata sentinel, and defines periodic phase intervals. It does not fully define RMS rounding, reductions over incomplete nodata groups, interpolation around nodata, or the numeric range of `TimeSel::Phase`.

## Decisions

- Integer `mean` uses round half up, implemented with integer arithmetic. Four-child groups use `floor((sum + 2) / 4)`; radial pairs use the corresponding `floor((sum + 1) / 2)` rule.
- `rms` is `floor(sqrt(floor(sum(raw²) / valid_count)))`, computed with integer square root. Mode ties choose the lowest raw value.
- Downsample operators ignore nodata children. If every child is nodata, the result is the declared nodata sentinel; a group with no valid children and no sentinel is invalid.
- Spatial interpolation and periodic reductions ignore nodata contributors and renormalize the remaining weights. A result with no valid contributors is nodata.
- `TimeSel::Phase(f64)` is a normalized cycle count. Finite values wrap modulo one before selecting the half-open periodic slice interval.
- The architecture permits `f32` raster storage but requires downsample operators to use integer arithmetic and gives no `f32` conversion rule. `PyramidBuilder` therefore refuses to generate `f32` parent tiles; stored `f32` levels remain sampleable. This remains an owner decision rather than an invented conversion.

## Consequences

These rules make supported integer pyramid and sampler outputs deterministic across the supported targets. They do not change canonical blob identity, stored raw values, or the frozen topology formulas. The `f32` pyramid limitation remains unresolved until a normative conversion rule is selected. The choices should be reviewed with the Stage 4 patch and can be superseded only through an explicit format or API decision.

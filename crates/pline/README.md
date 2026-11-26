# nodalia-pline

Nodalia's own closed polylines with arc segments, and boolean operations on them. Torno reads the
exact dark area of a Gerber layer (copper, solder mask, silkscreen) with it (`crates/io`,
`gerber_area.rs`).

Standalone: std only, no `unsafe`, no panics on input. `cargo xtask layers` lists it as such.

## What it does

| | |
|---|---|
| `Pline` | A closed chain of vertices; each vertex's bulge describes the segment leaving it (`0` line, `tan(sweep/4)` arc, positive counter-clockwise) |
| `area`, `extents`, `winding_number` | Signed area (arcs included), bounds (arc extremes included), winding number of a point |
| `boolean(other, Or / And / Not)` | Union, intersection and difference of two closed polylines, either direction. The result says how the two relate (`BooleanInfo`: apart, one inside the other, the same outline, crossing, or invalid input) and gives material loops (`pos`, counter-clockwise) and holes (`neg`, clockwise) |

## How the boolean works

Both outlines are cut where they meet (line/line, line/arc and arc/arc, including the stretches
they share), each piece is classified against the other outline by its midpoint (inside, outside,
or along it in the same or the opposite direction), the pieces the operation keeps are chained into
loops, and loops are told apart by their direction. Where loops touch at a point, the most
left-turning continuation is taken so they stay apart.

## Limits, honestly

- One tolerance for everything, `1e-9` of the larger of the combined size and the largest
  coordinate. Shapes closer than that to touching are treated as touching.
- The pair search is quadratic in the number of segments (capped at 400 million pairs, then the
  answer is `InvalidInput`). There is no spatial index yet, so very large outlines are slow.
- `Xor` and offsetting are not implemented. Nothing in Torno needs them yet.
- Inputs must be simple (not crossing themselves). A self-crossing outline gives an unspecified
  result, though never a panic.

## Tests

Analytic cases (rectangles, the lens of two circles, a circle cut by a line, rings with holes,
shapes that share an edge, nested and identical outlines) and two properties checked on thousands of
generated pairs: for any two shapes, `area(A or B) + area(A and B) = area(A) + area(B)` and
`area(A not B) = area(A) - area(A and B)`. One set of pairs is random; the other sits on a coarse
grid so that edges, corners and whole circles are shared, which is what drawn layouts do all the
time. Hostile input (NaN, infinity, empty, flat, absurd magnitudes) is answered with
`InvalidInput`.

## Clean room

Written from textbooks and the geometry of lines and circles; no other polyline library's source
was read (it replaces the third-party `cavalier_contours` crate in Torno).

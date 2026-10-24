//! How a lid tile is laid on its block when seen from above: which texel corner lands on each map
//! corner (rotation and flip), and which part of the block the lid covers (diagonals and partial
//! blocks). Map corners are NW, NE, SE, SW with north up (y decreasing); texel corners are (u, v) in
//! {0, 1}, v = 0 being the first row of the tile.

/// Texel corner (u, v) at the map corners NW, NE, SE, SW.
pub type Corners = [[u8; 2]; 4];

/// GTA1 lid rotation (CityScape 2.1 type_map bits 14-15). The direction comes from OpenGTA
/// @ 1ae34ae3 gl_cityview.cpp `drawBlock`: the flat lid's corners (slope1_data.h) SW, SE, NE, NW take
/// the texture corners (0,1) (1,1) (1,0) (0,0) starting `rotation` corners later, so each step turns
/// the tile a quarter turn clockwise.
pub fn gta1_corners(rotation: u8) -> Corners {
    match rotation & 3 {
        0 => [[0, 0], [1, 0], [1, 1], [0, 1]],
        1 => [[0, 1], [0, 0], [1, 0], [1, 1]],
        2 => [[1, 1], [0, 1], [0, 0], [1, 0]],
        _ => [[1, 0], [1, 1], [0, 1], [0, 0]],
    }
}

/// GTA2 lid rotation and flip (GBH Map 3, lid bits 13-15). gta2_re @ edb484d3 MapRenderer.cpp:
/// `kLidFlags_620FA4[lid >> 13]` is {0, 8, 0x20, 0x28, 0x40, 0x48, 0x60, 0x68}, and `draw_4F3FB0`
/// turns those flags into the texel corner of each lid vertex (vertex 0 = NW, then NE, SE, SW).
pub fn gta2_corners(rotation: u8, flip: bool) -> Corners {
    match (rotation & 3, flip) {
        (0, false) => [[0, 0], [1, 0], [1, 1], [0, 1]],
        (0, true) => [[1, 0], [0, 0], [0, 1], [1, 1]],
        (1, false) => [[0, 1], [0, 0], [1, 0], [1, 1]],
        (1, true) => [[0, 0], [0, 1], [1, 1], [1, 0]],
        (2, false) => [[1, 1], [0, 1], [0, 0], [1, 0]],
        (2, true) => [[0, 1], [1, 1], [1, 0], [0, 0]],
        (3, false) => [[1, 0], [1, 1], [0, 1], [0, 0]],
        _ => [[1, 1], [1, 0], [0, 0], [0, 1]],
    }
}

/// The texel (0..n-1) at pixel (px, py) of an n-pixel block, interpolating the corners at the pixel
/// centre.
pub fn texel(c: &Corners, px: usize, py: usize, n: usize) -> (usize, usize) {
    let (fx, fy) = ((px as f64 + 0.5) / n as f64, (py as f64 + 0.5) / n as f64);
    let at = |i: usize| {
        let v = c[0][i] as f64 * (1.0 - fx) * (1.0 - fy)
            + c[1][i] as f64 * fx * (1.0 - fy)
            + c[2][i] as f64 * fx * fy
            + c[3][i] as f64 * (1.0 - fx) * fy;
        ((v * n as f64) as usize).min(n - 1)
    };
    (at(0), at(1))
}

/// The part of a block a lid covers, in 64ths of the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Square,
    /// x0, y0, x1, y1 (end exclusive).
    Rect(u8, u8, u8, u8),
    /// The triangle NE-SE-SW (lower right).
    LowerRight,
    /// NW-SE-SW (lower left).
    LowerLeft,
    /// SW-NW-NE (upper left).
    UpperLeft,
    /// NW-NE-SE (upper right).
    UpperRight,
}

impl Shape {
    /// Whether the centre of pixel (px, py) of an n-pixel block is inside (edges included).
    pub fn covers(self, px: usize, py: usize, n: usize) -> bool {
        // Centre co-ordinates doubled, to stay in integers: 2px+1 against 2n.
        let (x, y, m) = (2 * px + 1, 2 * py + 1, 2 * n);
        match self {
            Shape::Square => true,
            Shape::Rect(x0, y0, x1, y1) => {
                let s = |v: u8| v as usize * m / 64;
                x >= s(x0) && x < s(x1) && y >= s(y0) && y < s(y1)
            }
            Shape::LowerRight => x + y >= m,
            Shape::LowerLeft => y >= x,
            Shape::UpperLeft => x + y <= m,
            Shape::UpperRight => x >= y,
        }
    }
}

/// The lid shape of a GTA2 slope code (GBH Map 3), from gta2_re @ edb484d3 MapRenderer.cpp:
/// * diagonals 45-48 and 4-sided diagonal slopes 49-52 keep one triangle of the lid quad
///   (`draw_lid_4EE130`, `gDiagonalLidType_620F20` 0 up-left, 1 up-right, 3 down-left, 2 down-right;
///   their flag tables keep each corner's texel, so the texture is the full lid's, masked);
/// * partial blocks 53-61 draw the lid over 3/8 or 5/8 of the block (`DrawPartialBlock*`,
///   `kFpThreeEighths_6F6428`, `kFpFiveEighths_6F6430`: 24 and 40 of 64 pixels).
///
/// 3-sided diagonal slopes (lid tile 1023) draw no lid; that is handled by the caller.
pub fn gta2_shape(slope: u8) -> Shape {
    match slope {
        45 | 49 => Shape::LowerRight,
        46 | 50 => Shape::LowerLeft,
        47 | 51 => Shape::UpperRight,
        48 | 52 => Shape::UpperLeft,
        53 => Shape::Rect(0, 0, 24, 64),
        54 => Shape::Rect(40, 0, 64, 64),
        55 => Shape::Rect(0, 0, 64, 24),
        56 => Shape::Rect(0, 40, 64, 64),
        57 => Shape::Rect(0, 0, 24, 24),
        58 => Shape::Rect(40, 0, 64, 24),
        59 => Shape::Rect(40, 40, 64, 64),
        60 => Shape::Rect(0, 40, 24, 64),
        61 => Shape::Rect(24, 24, 40, 40),
        _ => Shape::Square,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotations_turn_clockwise_and_flip_mirrors() {
        // A quarter turn clockwise puts the tile's top-left texel at the NE corner.
        assert_eq!(texel(&gta1_corners(1), 63, 0, 64), (0, 0));
        assert_eq!(texel(&gta2_corners(1, false), 63, 0, 64), (0, 0));
        assert_eq!(texel(&gta2_corners(0, true), 0, 5, 64), (63, 5));
        for r in 0..4 {
            assert_eq!(gta1_corners(r), gta2_corners(r, false));
            // Every pixel maps to a distinct texel.
            let mut seen = std::collections::HashSet::new();
            for y in 0..64 {
                for x in 0..64 {
                    assert!(seen.insert(texel(&gta2_corners(r, r % 2 == 0), x, y, 64)));
                }
            }
        }
    }

    #[test]
    fn shapes() {
        assert!(Shape::LowerRight.covers(63, 0, 64) && !Shape::LowerRight.covers(0, 0, 64));
        assert!(Shape::LowerRight.covers(32, 31, 64) && !Shape::LowerRight.covers(31, 31, 64));
        assert!(Shape::LowerLeft.covers(5, 5, 64) && !Shape::LowerLeft.covers(6, 5, 64));
        let left = gta2_shape(53);
        assert!(left.covers(23, 10, 64) && !left.covers(24, 10, 64));
        assert!(gta2_shape(61).covers(24, 39, 64) && !gta2_shape(61).covers(40, 24, 64));
    }
}

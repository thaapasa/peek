/// Result of 2-color clustering for a cell's pixel block.
pub struct ClusterResult {
    /// First dominant color (RGB)
    pub color_a: [u8; 3],
    /// Second dominant color (RGB)
    pub color_b: [u8; 3],
    /// Bitmap: bit i = 1 means pixel i belongs to color_a (foreground),
    /// bit i = 0 means pixel i belongs to color_b (background).
    /// Packed row-major: row 0 bits 7..0 = columns 0..7, row 1 bits 15..8, etc.
    pub bitmap: u128,
}

/// Squared Euclidean distance between two RGB colors.
fn color_dist_sq(a: &[u8; 3], b: &[u8; 3]) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    (dr * dr + dg * dg + db * db) as u32
}

/// Find the two dominant colors in a pixel block using fast k-means (k=2).
///
/// `pixels` must contain exactly `CELL_W * CELL_H` (128) RGB triples,
/// stored row-major.
///
/// Returns a `ClusterResult` with the two colors and a bitmap indicating
/// which pixels belong to which cluster.
pub fn fast_2_color(pixels: &[[u8; 3]]) -> ClusterResult {
    let n = pixels.len();
    debug_assert!(n == 128, "expected 128 pixels, got {n}");

    // Compute mean color
    let (mut sum_r, mut sum_g, mut sum_b) = (0u64, 0u64, 0u64);
    for p in pixels {
        sum_r += p[0] as u64;
        sum_g += p[1] as u64;
        sum_b += p[2] as u64;
    }
    let mean = [
        (sum_r / n as u64) as u8,
        (sum_g / n as u64) as u8,
        (sum_b / n as u64) as u8,
    ];

    // Check if the cell is uniform (all pixels very close to mean)
    let mut max_dist = 0u32;
    let mut farthest_idx = 0;
    for (i, p) in pixels.iter().enumerate() {
        let d = color_dist_sq(p, &mean);
        if d > max_dist {
            max_dist = d;
            farthest_idx = i;
        }
    }

    // Uniform cell threshold: if max distance is very small, short-circuit
    if max_dist < 300 {
        // ~10 per channel
        return ClusterResult {
            color_a: mean,
            color_b: mean,
            bitmap: 0, // all pixels → background (space character)
        };
    }

    // Initialize centroids: farthest from mean, and farthest from that
    let mut centroid_b = pixels[farthest_idx];
    let mut centroid_a = pixels[0];
    let mut max_dist_from_b = 0u32;
    for (i, p) in pixels.iter().enumerate() {
        let d = color_dist_sq(p, &centroid_b);
        if d > max_dist_from_b {
            max_dist_from_b = d;
            centroid_a = pixels[i];
        }
    }

    // K-means iterations (2 rounds)
    let mut assignments = [false; 128];
    for _iter in 0..2 {
        // Assign each pixel to the closer centroid
        let (mut sa_r, mut sa_g, mut sa_b, mut ca) = (0u64, 0u64, 0u64, 0u64);
        let (mut sb_r, mut sb_g, mut sb_b, mut cb) = (0u64, 0u64, 0u64, 0u64);

        for (i, p) in pixels.iter().enumerate() {
            let da = color_dist_sq(p, &centroid_a);
            let db = color_dist_sq(p, &centroid_b);
            if da <= db {
                assignments[i] = true; // belongs to A
                sa_r += p[0] as u64;
                sa_g += p[1] as u64;
                sa_b += p[2] as u64;
                ca += 1;
            } else {
                assignments[i] = false; // belongs to B
                sb_r += p[0] as u64;
                sb_g += p[1] as u64;
                sb_b += p[2] as u64;
                cb += 1;
            }
        }

        // Recompute centroids
        if let Some(ca) = std::num::NonZeroU64::new(ca) {
            centroid_a = [
                (sa_r / ca) as u8,
                (sa_g / ca) as u8,
                (sa_b / ca) as u8,
            ];
        }
        if let Some(cb) = std::num::NonZeroU64::new(cb) {
            centroid_b = [
                (sb_r / cb) as u8,
                (sb_g / cb) as u8,
                (sb_b / cb) as u8,
            ];
        }
    }

    // Pack assignments into u128 bitmap
    // Bit layout: bit 0 = pixel at (row=0, col=0), bit 1 = (row=0, col=1), ...
    // bit 7 = (row=0, col=7), bit 8 = (row=1, col=0), etc.
    let mut bitmap: u128 = 0;
    for (i, &assigned_to_a) in assignments.iter().enumerate() {
        if assigned_to_a {
            bitmap |= 1u128 << i;
        }
    }

    ClusterResult {
        color_a: centroid_a,
        color_b: centroid_b,
        bitmap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uniform_cell() {
        let pixels = [[100u8, 150, 200]; 128];
        let result = fast_2_color(&pixels);
        // Should short-circuit: bitmap is 0 (all background)
        assert_eq!(result.bitmap, 0);
        assert_eq!(result.color_a, result.color_b);
    }

    #[test]
    fn test_two_color_split() {
        let mut pixels = [[0u8, 0, 0]; 128];
        // Top half white, bottom half black
        for p in pixels.iter_mut().take(64) {
            *p = [255, 255, 255];
        }
        let result = fast_2_color(&pixels);
        // The two colors should be clearly different
        let dist = color_dist_sq(&result.color_a, &result.color_b);
        assert!(dist > 10000, "colors should be far apart, dist={dist}");
        // Check that exactly 64 bits are set (one cluster) or 64 are clear
        let popcount = result.bitmap.count_ones();
        assert!(
            popcount == 64,
            "expected 64 bits set, got {popcount}"
        );
    }

    #[test]
    fn test_vertical_split() {
        let mut pixels = [[0u8, 0, 0]; 128];
        // Left half (cols 0-3) red, right half (cols 4-7) blue
        for row in 0..16 {
            for col in 0..8 {
                let idx = row * 8 + col;
                if col < 4 {
                    pixels[idx] = [255, 0, 0];
                } else {
                    pixels[idx] = [0, 0, 255];
                }
            }
        }
        let result = fast_2_color(&pixels);
        let dist = color_dist_sq(&result.color_a, &result.color_b);
        assert!(dist > 10000, "colors should be far apart");
        assert_eq!(result.bitmap.count_ones(), 64);
    }
}

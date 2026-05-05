use std::path::Path;

/// Piecewise-linear pH calibration loaded from a CSV file.
///
/// CSV format (header row is optional, non-numeric first column rows are skipped):
///   raw_ph,actual_ph
///   4.10,4.00
///   6.86,7.01
///
/// Correction rules:
///   0 points  → pass-through (no change)
///   1 point   → constant offset: actual = raw + (actual_0 − raw_0)
///   2+ points → piecewise linear interpolation, with linear extrapolation
///               beyond the first/last segment
pub struct PhCalibration {
    /// Sorted ascending by raw_ph.
    points: Vec<(f32, f32)>,
}

impl PhCalibration {
    pub fn identity() -> Self {
        PhCalibration { points: Vec::new() }
    }

    pub fn load_from_csv(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;

        let mut points: Vec<(f32, f32)> = Vec::new();

        for (idx, line) in content.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut cols = line.split(',');
            let raw_str    = cols.next().unwrap_or("").trim();
            let actual_str = cols.next().unwrap_or("").trim();

            // Skip header rows where the first column is not a number
            let raw: f32 = match raw_str.parse() {
                Ok(v) => v,
                Err(_) => continue,
            };
            let actual: f32 = actual_str.parse().map_err(|_| {
                format!("line {}: cannot parse actual_ph '{actual_str}'", idx + 1)
            })?;

            if !(0.0..=14.0).contains(&raw) || !(0.0..=14.0).contains(&actual) {
                return Err(format!(
                    "line {}: pH values must be 0–14 (got raw={raw}, actual={actual})",
                    idx + 1
                ));
            }

            points.push((raw, actual));
        }

        // Sort by raw pH and deduplicate exact duplicates (keep first)
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        points.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-6);

        log::info!(
            "[cal] loaded {} calibration point(s) from {}",
            points.len(),
            path.display()
        );
        for (raw, actual) in &points {
            log::info!("[cal]   raw={raw:.3}  →  actual={actual:.3}  (offset={:+.3})", actual - raw);
        }

        Ok(PhCalibration { points })
    }

    pub fn is_active(&self) -> bool {
        !self.points.is_empty()
    }

    /// Apply calibration to a raw pH reading.
    pub fn apply(&self, raw: f32) -> f32 {
        let pts = &self.points;
        match pts.len() {
            0 => raw,
            1 => raw + (pts[0].1 - pts[0].0),
            _ => interpolate(pts, raw),
        }
    }
}

fn interpolate(pts: &[(f32, f32)], x: f32) -> f32 {
    let n = pts.len();

    // Extrapolate below the first point using the first segment
    if x <= pts[0].0 {
        return lerp(pts[0], pts[1], x);
    }
    // Extrapolate above the last point using the last segment
    if x >= pts[n - 1].0 {
        return lerp(pts[n - 2], pts[n - 1], x);
    }
    // Find the segment that brackets x
    for i in 0..n - 1 {
        if x >= pts[i].0 && x <= pts[i + 1].0 {
            return lerp(pts[i], pts[i + 1], x);
        }
    }
    x // unreachable
}

#[inline]
fn lerp((x0, y0): (f32, f32), (x1, y1): (f32, f32), x: f32) -> f32 {
    let dx = x1 - x0;
    if dx.abs() < 1e-9 { return y0; }
    y0 + (x - x0) * (y1 - y0) / dx
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cal(pts: &[(f32, f32)]) -> PhCalibration {
        let mut points = pts.to_vec();
        points.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        PhCalibration { points }
    }

    #[test]
    fn identity_passthrough() {
        assert_eq!(PhCalibration::identity().apply(7.0), 7.0);
    }

    #[test]
    fn single_point_offset() {
        let c = cal(&[(6.8, 7.0)]);
        // offset = +0.2
        assert!((c.apply(6.8) - 7.0).abs() < 1e-5);
        assert!((c.apply(4.0) - 4.2).abs() < 1e-5);
    }

    #[test]
    fn two_point_linear() {
        // raw 4→4, raw 7→7.2 (slope ≠ 1)
        let c = cal(&[(4.0, 4.0), (7.0, 7.2)]);
        assert!((c.apply(4.0) - 4.0).abs() < 1e-4);
        assert!((c.apply(7.0) - 7.2).abs() < 1e-4);
        // midpoint
        let mid = c.apply(5.5);
        assert!((mid - 5.6).abs() < 1e-4);
    }

    #[test]
    fn extrapolation_below() {
        let c = cal(&[(4.0, 4.0), (7.0, 7.2)]);
        // slope = (7.2-4.0)/(7.0-4.0) = 3.2/3.0
        let expected = 4.0 + (3.0 - 4.0) * (7.2 - 4.0) / (7.0 - 4.0);
        assert!((c.apply(3.0) - expected).abs() < 1e-4);
    }
}

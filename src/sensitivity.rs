//! 厘米/360° + DPI → 鼠标灵敏度。
//!
//! 公式：转一圈需要 `dpi * (cm/360) / 2.54` 个 count，每个 count 转 `360 / 那个数` 度。
//! **不要** 再乘 `delta_time`：鼠标报告的是位移，不是速度。
//!
//! 默认 19.05cm/360 @ 800 DPI = 刚好 6000 count 一圈（常见 CS 手感）。

use crate::error::{Result, ShotError};

pub const DEFAULT_CM_PER_360: f64 = 19.05;
pub const DEFAULT_DPI: u32 = 800;

/// 转一圈需要多少鼠标 count。19.05cm @ 800DPI = 6000。
pub fn counts_per_360(cm_per_360: f64, dpi: u32) -> Result<f64> {
    if !(cm_per_360.is_finite() && cm_per_360 > 0.05 && cm_per_360 < 500.0) {
        return Err(ShotError::Sensitivity(format!(
            "cm/360 must be in (0.05, 500], got {cm_per_360}"
        )));
    }
    if !(200..=32_000).contains(&dpi) {
        return Err(ShotError::Sensitivity(format!(
            "dpi must be in 200..=32000, got {dpi}"
        )));
    }
    let inches_per_360 = cm_per_360 / 2.54;
    Ok(f64::from(dpi) * inches_per_360)
}

/// 每个鼠标 count 转多少度。Bevy 每帧用这个乘 `MouseMotion.delta`。
pub fn deg_per_count(cm_per_360: f64, dpi: u32) -> Result<f64> {
    Ok(360.0 / counts_per_360(cm_per_360, dpi)?)
}

/// 把一次鼠标位移加到视角上。pitch 钳在 ±89.9°，避免万向锁。
pub fn apply_look(
    yaw_deg: f32,
    pitch_deg: f32,
    dx: f32,
    dy: f32,
    deg_per_count: f64,
) -> (f32, f32) {
    let scale = deg_per_count as f32;
    let mut yaw = yaw_deg + dx * scale;
    if yaw > 180.0 {
        yaw -= 360.0;
    } else if yaw < -180.0 {
        yaw += 360.0;
    }
    let pitch = (pitch_deg - dy * scale).clamp(-89.9, 89.9);
    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_CM_PER_360, DEFAULT_DPI, apply_look, counts_per_360, deg_per_count};

    #[test]
    fn counts_per_360_should_be_6000_for_default_sens_and_800_dpi() {
        let counts = counts_per_360(DEFAULT_CM_PER_360, DEFAULT_DPI).unwrap();
        assert!((counts - 6000.0).abs() < 1e-9);
    }

    #[test]
    fn yaw_should_wrap_to_360_after_6000_counts() {
        let step = deg_per_count(DEFAULT_CM_PER_360, DEFAULT_DPI).unwrap();
        let mut yaw = 0.0_f32;
        let mut pitch = 0.0_f32;
        for _ in 0..6000 {
            (yaw, pitch) = apply_look(yaw, pitch, 1.0, 0.0, step);
        }
        let wrapped = ((yaw as f64) + 3600.0) % 360.0;
        let err = wrapped.min(360.0 - wrapped);
        assert!(err < 0.02, "yaw={yaw} wrapped={wrapped}");
        assert!(pitch.abs() < 1e-6);
    }

    #[test]
    fn pitch_should_not_use_frame_delta_scaling() {
        let step = deg_per_count(DEFAULT_CM_PER_360, DEFAULT_DPI).unwrap();
        let (_, pitch) = apply_look(0.0, 0.0, 0.0, 100.0, step);
        assert!((pitch + 6.0).abs() < 1e-4);
    }
}

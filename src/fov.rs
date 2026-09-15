//! FOV 换算。Bevy 的透视相机要的是 **垂直** 弧度，游戏设置里玩家填的是水平或垂直度数。
//!
//! [`FovKind::HorizontalRes`]（默认）：和 CS 一样，水平 FOV 相对 **渲染分辨率** 的宽高比，
//! 不是窗口拉伸后的比例。所以 4:3 渲染 + 无边框拉伸时，准星手感仍按 4:3 算。

use serde::{Deserialize, Serialize};

/// 玩家在设置里选的 FOV 含义。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FovKind {
    /// 水平 FOV，相对渲染分辨率宽高比（CS 默认）。
    #[default]
    HorizontalRes,
    /// 先按 4:3 水平解释，再映射到当前宽高比。
    Horizontal4x3,
    /// 数字本身就是垂直 FOV。
    Vertical,
}

/// 换成 Bevy `PerspectiveProjection.fov`（垂直、弧度）。
pub fn vertical_fov_radians(fov_deg: f32, kind: FovKind, aspect: f32) -> f32 {
    let fov = fov_deg.to_radians();
    match kind {
        FovKind::Vertical => fov,
        FovKind::HorizontalRes => 2.0 * (fov * 0.5).tan().atan2(aspect),
        FovKind::Horizontal4x3 => {
            let v = 2.0 * (fov * 0.5).tan().atan2(4.0 / 3.0);
            2.0 * ((v * 0.5).tan() * aspect).atan()
        }
    }
}

pub fn stretch_scales(window_w: f32, window_h: f32, render_w: f32, render_h: f32) -> (f32, f32) {
    (window_w / render_w, window_h / render_h)
}

#[cfg(test)]
mod tests {
    use super::{FovKind, stretch_scales, vertical_fov_radians};

    #[test]
    fn horizontal_fov_should_use_render_aspect_not_window() {
        let aspect_4_3 = 1280.0 / 960.0;
        let vfov = vertical_fov_radians(90.0, FovKind::HorizontalRes, aspect_4_3);
        let expected = 2.0 * (45.0_f32.to_radians().tan() / aspect_4_3).atan();
        assert!((vfov - expected).abs() < 1e-6);
    }

    #[test]
    fn stretch_scales_should_be_independent_on_x_and_y() {
        let (sx, sy) = stretch_scales(1920.0, 1080.0, 1280.0, 960.0);
        assert!((sx - 1.5).abs() < 1e-6);
        assert!((sy - 1.125).abs() < 1e-6);
    }
}

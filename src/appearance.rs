//! 房间外观、准星裁剪、回放轨迹网格。不依赖 Bevy 类型，viewport 再转成 Image/Mesh。
//!
//! - [`marble_texture`] / [`brick_texture`]：程序化贴图，颜色乘主题 tint
//! - [`load_crosshair`]：读 PNG 后裁掉空白，只留「有墨」的像素
//! - [`trail_screen_points`]：把一段时间的准星扫墙投影成屏幕折线
//! - [`polyline_mesh`] / [`disk_mesh`]：折线和圆点变成三角形，给 2D HUD 用
//!
//! 房间尺寸常量必须和 [`crate::scenario::WALL_Z`] 对齐，否则球会嵌进墙里或浮空。

use std::path::Path;

use crate::paths::AppPaths;
use crate::scenario::{PLAYER_EYE, WALL_Z};
use crate::session::SessionFile;
use crate::vec3::Vec3;

/// 房间半宽（米）。左右墙在 ± 这个值。
pub const ROOM_HALF_X: f32 = 5.35;
pub const ROOM_HEIGHT: f32 = 4.55;
pub const ROOM_BACK: f32 = 1.6;
pub const WALL_THICK: f32 = 0.12;
pub const MARBLE_TILE_M: f32 = 1.52;
pub const PAVER_TILE_M: f32 = 1.0;
/// 回放轨迹只画当前时刻往前这么多微秒（约 0.55 秒）。
pub const TRAIL_WINDOW_US: u64 = 550_000;
pub const TRAIL_WIDTH_PX: f32 = 5.0;

pub fn aim_wall_z() -> f32 {
    WALL_Z
}

pub fn room_front() -> f32 {
    WALL_Z
}

/// CPU 上的 RGBA 图。viewport 再上传成 Bevy `Image`。
#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let i = ((y * self.width + x) * 4) as usize;
        [
            self.pixels[i],
            self.pixels[i + 1],
            self.pixels[i + 2],
            self.pixels[i + 3],
        ]
    }
}

/// 带石纹的墙/天花板贴图。`tint` 来自主题的 wall 颜色。
pub fn marble_texture(tint: [f32; 3], size: u32) -> RgbaImage {
    let size = size.max(32);
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / size as f32;
            let v = y as f32 / size as f32;
            let grout = u < 0.016 || v < 0.016 || u > 0.984 || v > 0.984;
            let n = hash2(u * 17.3, v * 9.1);
            let warp =
                (u * 5.4 + v * 2.1 + n * 0.4).sin() * 0.035 + (u * 2.7 - v * 6.8).sin() * 0.02;
            let vein_a = (u * 7.2 + v * 11.6 + warp * 8.0).sin().abs().powf(0.42);
            let vein_b = (u * 13.0 - v * 8.4 + warp * 5.0 + n).sin().abs().powf(0.65);
            let vein = (vein_a * 0.55 + vein_b * 0.22).min(1.0);
            let shade = if grout {
                0.64 + n * 0.02
            } else {
                (0.745 - vein * 0.13 + n * 0.015).clamp(0.60, 0.82)
            };
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = (tint[0] * shade * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 1] = (tint[1] * shade * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 2] = (tint[2] * (shade * 1.04).min(1.0) * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 3] = 255;
        }
    }
    RgbaImage {
        width: size,
        height: size,
        pixels,
    }
}

fn hash2(x: f32, y: f32) -> f32 {
    ((x * 12.9898 + y * 78.233).sin() * 43758.5453).fract()
}

/// 地面地砖。颜色偏冷灰，再乘 floor tint。
pub fn brick_texture(tint: [f32; 3], size: u32) -> RgbaImage {
    let size = size.max(32);
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    let bricks_x = 8.0;
    let bricks_y = 4.0;
    for y in 0..size {
        for x in 0..size {
            let u = x as f32 / size as f32 * bricks_x;
            let v = y as f32 / size as f32 * bricks_y;
            let row = v.floor();
            let offset = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
            let local_x = (u + offset).fract();
            let local_y = v.fract();
            let mortar = local_x < 0.08 || local_y < 0.12 || local_x > 0.96;
            let n = hash2(x as f32, y as f32);
            let (r, g, b) = if mortar {
                (0.50, 0.495, 0.49)
            } else {
                (0.585 + n * 0.025, 0.580 + n * 0.02, 0.572 + n * 0.02)
            };
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = (r * tint[0] * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 1] = (g * tint[1] * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 2] = (b * tint[2] * 255.0).clamp(0.0, 255.0) as u8;
            pixels[i + 3] = 255;
        }
    }
    RgbaImage {
        width: size,
        height: size,
        pixels,
    }
}

/// 读准星图并裁到有内容的包围盒。失败返回 None，调用方用 [`fallback_crosshair`]。
pub fn load_crosshair(paths: &AppPaths, relative: &str) -> Option<RgbaImage> {
    let path = paths.resolve_res(relative);
    if !path.exists() {
        return None;
    }
    crop_crosshair_file(&path)
}

fn crop_crosshair_file(path: &Path) -> Option<RgbaImage> {
    let img = image::open(path).ok()?.into_rgba8();
    Some(crop_ink(&img))
}

fn crop_ink(img: &image::RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    let mut found = false;
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x, y).0;
            if is_ink(p) {
                found = true;
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if !found {
        return fallback_box();
    }
    let pad = 1u32;
    let min_x = min_x.saturating_sub(pad);
    let min_y = min_y.saturating_sub(pad);
    let max_x = (max_x + pad).min(w - 1);
    let max_y = (max_y + pad).min(h - 1);
    let cw = (max_x - min_x + 1).max(1);
    let ch = (max_y - min_y + 1).max(1);
    let mut pixels = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        for x in 0..cw {
            let src = img.get_pixel(min_x + x, min_y + y).0;
            let dst = ((y * cw + x) * 4) as usize;
            if is_ink(src) {
                pixels[dst] = src[0];
                pixels[dst + 1] = src[1];
                pixels[dst + 2] = src[2];
                pixels[dst + 3] = 255;
            } else {
                pixels[dst + 3] = 0;
            }
        }
    }
    RgbaImage {
        width: cw,
        height: ch,
        pixels,
    }
}

fn is_ink(p: [u8; 4]) -> bool {
    p[3] > 24 && (p[0] as u16 + p[1] as u16 + p[2] as u16) < 420
}

fn fallback_box() -> RgbaImage {
    fallback_crosshair()
}

pub fn fallback_crosshair() -> RgbaImage {
    let size = 6u32;
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for px in pixels.chunks_exact_mut(4) {
        px.copy_from_slice(&[12, 12, 12, 255]);
    }
    RgbaImage {
        width: size,
        height: size,
        pixels,
    }
}

/// 视线与瞄准墙的交点。回放轨迹用这个当「准星打在墙上的位置」。
pub fn wall_hit_at(yaw_deg: f32, pitch_deg: f32, wall_z: f32) -> Option<Vec3> {
    let dir = crate::scenario::look_direction(yaw_deg, pitch_deg);
    if dir.z.abs() < 1e-4 {
        return None;
    }
    let t = (wall_z - PLAYER_EYE.z) / dir.z;
    if t <= 0.05 {
        return None;
    }
    Some(PLAYER_EYE.add(dir.scale(t)))
}

pub fn wall_hit(yaw_deg: f32, pitch_deg: f32) -> Option<Vec3> {
    wall_hit_at(yaw_deg, pitch_deg, WALL_Z)
}

pub fn trail_screen_points(
    session: &SessionFile,
    t_us: u64,
    vfov: f32,
    aspect: f32,
    render_w: f32,
    render_h: f32,
) -> Vec<(f32, f32)> {
    let wall_z = aim_wall_z();
    let (cam_yaw, cam_pitch) = session.look_at(t_us);
    let start = t_us.saturating_sub(TRAIL_WINDOW_US);
    let mut points = Vec::new();
    let mut t = start;
    loop {
        let (yaw, pitch) = session.look_at(t);
        if let Some(hit) = wall_hit_at(yaw, pitch, wall_z) {
            if let Some((sx, sy, _)) = crate::project::project_world(
                hit, PLAYER_EYE, cam_yaw, cam_pitch, vfov, aspect, render_w, render_h,
            ) {
                if points
                    .last()
                    .is_none_or(|p: &(f32, f32)| (p.0 - sx).abs() + (p.1 - sy).abs() > 0.6)
                {
                    points.push((sx, sy));
                }
            }
        }
        if t >= t_us {
            break;
        }
        t = t.saturating_add(6_000).min(t_us);
    }
    points
}

/// 一段折线加宽成四边形条带。`width` 是 HUD 像素。
pub fn polyline_mesh(
    points: &[(f32, f32)],
    width: f32,
) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
    let mut positions = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();
    if points.len() < 2 || width <= 0.0 {
        return (positions, uvs, indices);
    }
    let half = width * 0.5;
    for window in points.windows(2) {
        let a = window[0];
        let b = window[1];
        let dx = b.0 - a.0;
        let dy = b.1 - a.1;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let nx = -dy / len * half;
        let ny = dx / len * half;
        let base = positions.len() as u32;
        positions.extend([
            [a.0 + nx, a.1 + ny, 0.0],
            [a.0 - nx, a.1 - ny, 0.0],
            [b.0 - nx, b.1 - ny, 0.0],
            [b.0 + nx, b.1 + ny, 0.0],
        ]);
        uvs.extend([[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    (positions, uvs, indices)
}

pub fn disk_mesh(cx: f32, cy: f32, radius: f32) -> (Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>) {
    let mut positions = vec![[cx, cy, 0.0]];
    let mut uvs = vec![[0.5, 0.5]];
    let mut indices = Vec::new();
    const SIDES: u32 = 12;
    for i in 0..SIDES {
        let a = i as f32 / SIDES as f32 * std::f32::consts::TAU;
        positions.push([cx + a.cos() * radius, cy + a.sin() * radius, 0.0]);
        uvs.push([0.5 + a.cos() * 0.5, 0.5 + a.sin() * 0.5]);
    }
    for i in 0..SIDES {
        let next = if i + 1 == SIDES { 1 } else { i + 2 };
        indices.extend([0, i + 1, next]);
    }
    (positions, uvs, indices)
}

/// Bevy 2D 的原点在屏幕中心，Y 向上；投影函数给的是左上角原点、Y 向下。
pub fn hud_from_screen(sx: f32, sy: f32, render_w: f32, render_h: f32) -> (f32, f32) {
    (sx - render_w * 0.5, render_h * 0.5 - sy)
}

#[cfg(test)]
mod tests {
    use super::{
        TRAIL_WINDOW_US, brick_texture, crop_ink, is_ink, load_crosshair, marble_texture,
        trail_screen_points,
    };
    use crate::paths::AppPaths;
    use crate::session::{MouseSample, SessionFile, SettingsSnapshot};
    use crate::settings::Settings;

    #[test]
    fn marble_texture_should_fill_rgba() {
        let tex = marble_texture([1.0, 1.0, 1.0], 32);
        assert_eq!(tex.pixels.len(), 32 * 32 * 4);
        assert_eq!(tex.pixels[3], 255);
        let grout = tex.pixel(0, 16);
        let grout_luma = grout[0] as u16 + grout[1] as u16 + grout[2] as u16;
        let mut slab_luma = 0u16;
        for y in 8..24 {
            for x in 8..24 {
                let p = tex.pixel(x, y);
                slab_luma = slab_luma.max(p[0] as u16 + p[1] as u16 + p[2] as u16);
            }
        }
        assert!(slab_luma > grout_luma, "slabs should be lighter than grout");
        let slab = tex.pixel(16, 16);
        assert!((slab[0] as i16 - slab[1] as i16).abs() < 18);
        assert!(slab[2] >= slab[0]);
    }

    #[test]
    fn brick_texture_should_be_cool_gray_pavers() {
        let tex = brick_texture([1.0, 1.0, 1.0], 32);
        let p = tex.pixel(16, 20);
        assert!(
            (p[0] as i16 - p[1] as i16).abs() < 16,
            "pavers must not be red clay"
        );
        assert!(p[0] > 110 && p[0] < 180);
    }

    #[test]
    fn polyline_mesh_should_emit_a_quad_per_segment() {
        let (pos, _, idx) = super::polyline_mesh(&[(0.0, 0.0), (10.0, 0.0)], 2.0);
        assert_eq!(pos.len(), 4);
        assert_eq!(idx.len(), 6);
    }

    #[test]
    fn crop_ink_should_keep_the_dark_square() {
        let mut img = image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 255, 255, 255]));
        for y in 20..24 {
            for x in 30..34 {
                img.put_pixel(x, y, image::Rgba([8, 8, 8, 255]));
            }
        }
        let cropped = crop_ink(&img);
        assert!(cropped.width <= 8 && cropped.height <= 8);
        assert!(cropped.width >= 4 && cropped.height >= 4);
        assert!(is_ink(cropped.pixel(cropped.width / 2, cropped.height / 2)));
    }

    #[test]
    fn bundled_crosshair_should_crop_to_ink() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let sprite =
            load_crosshair(&paths, crate::assets::DEFAULT_CROSSHAIR).expect("crosshair png");
        assert!(sprite.width <= 48 && sprite.height <= 48);
        assert!(sprite.width >= 2 && sprite.height >= 2);
    }

    #[test]
    fn trail_points_should_leave_center_when_look_changes() {
        let snap = SettingsSnapshot::from_settings(&Settings::default(), 1920, 1080, 60);
        let mut session = SessionFile::new("t".into(), "Sixshot Ultimate", 1, snap);
        session.mouse.push(MouseSample {
            t_us: 0,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: -8.0,
            pitch_deg: 0.0,
        });
        session.mouse.push(MouseSample {
            t_us: 10_000,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
        });
        let pts = trail_screen_points(&session, 10_000, 1.2, 16.0 / 9.0, 1920.0, 1080.0);
        assert!(pts.len() >= 2);
        let dx = (pts[0].0 - 960.0).abs();
        assert!(
            dx > 20.0,
            "old look should not project to screen center, dx={dx}"
        );
        let last = pts.last().expect("points");
        assert!((last.0 - 960.0).abs() < 8.0);
        assert!(TRAIL_WINDOW_US > 0);
    }
}

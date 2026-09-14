use crate::fov::vertical_fov_radians;
use crate::project::project_world;
use crate::scenario::{look_direction, PLAYER_EYE, WALL_X, WALL_Y_MAX, WALL_Y_MIN, WALL_Z};
use crate::session::{SessionFile, WorldKind};
use crate::vec3::Vec3;

pub fn raster_frame(session: &SessionFile, width: u32, height: u32, t_us: u64, out: &mut [u8]) {
    let w = width as usize;
    let h = height as usize;
    let sky = [210u8, 220, 230, 255];
    for px in out.chunks_exact_mut(4) {
        px.copy_from_slice(&sky);
    }

    let (yaw, pitch) = session.look_at(t_us);
    let settings = &session.header.settings;
    let aspect = settings.render_width.max(1) as f32 / settings.render_height.max(1) as f32;
    let vfov = vertical_fov_radians(settings.fov_deg, settings.fov_kind, aspect);

    fill_wall(out, w, h, yaw, pitch, vfov, aspect);
    let targets = targets_at(session, t_us);
    for target in &targets {
        if let Some((sx, sy, depth)) = project_world(
            target.pos,
            PLAYER_EYE,
            yaw,
            pitch,
            vfov,
            aspect,
            width as f32,
            height as f32,
        ) {
            let radius = (target.radius / depth) * (height as f32 * 0.55);
            fill_circle(out, w, h, sx, sy, radius.max(3.0), [18, 18, 18, 255]);
        }
    }

    let trail_start = t_us.saturating_sub(400_000);
    let mut prev: Option<(f32, f32)> = None;
    for sample in session
        .mouse
        .iter()
        .filter(|s| s.t_us >= trail_start && s.t_us <= t_us)
    {
        let dir = look_direction(sample.yaw_deg, sample.pitch_deg);
        if dir.z.abs() < 1e-4 {
            continue;
        }
        let t = (WALL_Z - PLAYER_EYE.z) / dir.z;
        if t <= 0.0 {
            continue;
        }
        let hit = PLAYER_EYE.add(dir.scale(t));
        if let Some((sx, sy, _)) = project_world(
            hit,
            PLAYER_EYE,
            yaw,
            pitch,
            vfov,
            aspect,
            width as f32,
            height as f32,
        ) {
            if let Some((px, py)) = prev {
                draw_line(out, w, h, px, py, sx, sy, [255, 80, 60, 255]);
            }
            prev = Some((sx, sy));
        }
    }

    let cx = width as f32 * 0.5;
    let cy = height as f32 * 0.5;
    draw_crosshair(out, w, h, cx, cy);

    for shot in session.shots.iter().filter(|s| s.t_us + 80_000 >= t_us && s.t_us <= t_us) {
        let dir = look_direction(shot.yaw_deg, shot.pitch_deg);
        if dir.z.abs() < 1e-4 {
            continue;
        }
        let t = (WALL_Z - PLAYER_EYE.z) / dir.z;
        if t <= 0.0 {
            continue;
        }
        let hitp = PLAYER_EYE.add(dir.scale(t));
        if let Some((sx, sy, _)) = project_world(
            hitp,
            PLAYER_EYE,
            yaw,
            pitch,
            vfov,
            aspect,
            width as f32,
            height as f32,
        ) {
            let color = if shot.hit {
                [80u8, 220, 90, 255]
            } else {
                [240, 70, 70, 255]
            };
            fill_circle(out, w, h, sx, sy, 5.0, color);
        }
    }
}

#[derive(Clone, Copy)]
struct Target {
    pos: Vec3,
    radius: f32,
}

fn targets_at(session: &SessionFile, t_us: u64) -> Vec<Target> {
    let mut live = std::collections::HashMap::new();
    for event in session.world.iter().filter(|e| e.t_us <= t_us) {
        match event.kind {
            WorldKind::Spawn => {
                live.insert(
                    event.id,
                    Target {
                        pos: event.pos,
                        radius: event.radius,
                    },
                );
            }
            WorldKind::Despawn => {
                live.remove(&event.id);
            }
        }
    }
    live.into_values().collect()
}

fn fill_wall(
    out: &mut [u8],
    w: usize,
    h: usize,
    yaw: f32,
    pitch: f32,
    vfov: f32,
    aspect: f32,
) {
    let corners = [
        Vec3::new(-WALL_X, WALL_Y_MIN, WALL_Z),
        Vec3::new(WALL_X, WALL_Y_MIN, WALL_Z),
        Vec3::new(WALL_X, WALL_Y_MAX, WALL_Z),
        Vec3::new(-WALL_X, WALL_Y_MAX, WALL_Z),
    ];
    let mut pts = Vec::new();
    for c in corners {
        if let Some((x, y, _)) = project_world(
            c,
            PLAYER_EYE,
            yaw,
            pitch,
            vfov,
            aspect,
            w as f32,
            h as f32,
        ) {
            pts.push((x, y));
        }
    }
    if pts.len() == 4 {
        fill_quad(out, w, h, &pts, [210, 208, 204, 255]);
    }
}

fn fill_quad(out: &mut [u8], w: usize, h: usize, pts: &[(f32, f32)], color: [u8; 4]) {
    let min_x = pts.iter().map(|p| p.0).fold(f32::MAX, f32::min).max(0.0) as usize;
    let max_x = pts.iter().map(|p| p.0).fold(0.0, f32::max).min(w as f32 - 1.0) as usize;
    let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).max(0.0) as usize;
    let max_y = pts.iter().map(|p| p.1).fold(0.0, f32::max).min(h as f32 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            if point_in_quad(x as f32, y as f32, pts) {
                put(out, w, h, x, y, color);
            }
        }
    }
}

fn point_in_quad(x: f32, y: f32, pts: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let n = pts.len();
    let mut j = n - 1;
    for i in 0..n {
        let pi = pts[i];
        let pj = pts[j];
        if ((pi.1 > y) != (pj.1 > y))
            && (x < (pj.0 - pi.0) * (y - pi.1) / (pj.1 - pi.1 + 1e-6) + pi.0)
        {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn fill_circle(out: &mut [u8], w: usize, h: usize, cx: f32, cy: f32, r: f32, color: [u8; 4]) {
    let r2 = r * r;
    let min_x = (cx - r).max(0.0) as usize;
    let max_x = (cx + r).min(w as f32 - 1.0) as usize;
    let min_y = (cy - r).max(0.0) as usize;
    let max_y = (cy + r).min(h as f32 - 1.0) as usize;
    for y in min_y..=max_y {
        for x in min_x..=max_x {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r2 {
                put(out, w, h, x, y, color);
            }
        }
    }
}

fn draw_line(out: &mut [u8], w: usize, h: usize, x0: f32, y0: f32, x1: f32, y1: f32, color: [u8; 4]) {
    let steps = ((x1 - x0).abs().max((y1 - y0).abs()) as usize).max(1);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let x = (x0 + (x1 - x0) * t).round() as isize;
        let y = (y0 + (y1 - y0) * t).round() as isize;
        if x >= 0 && y >= 0 {
            put(out, w, h, x as usize, y as usize, color);
        }
    }
}

fn draw_crosshair(out: &mut [u8], w: usize, h: usize, cx: f32, cy: f32) {
    let color = [20u8, 20, 20, 255];
    for d in -10i32..=10 {
        if d.abs() < 3 {
            continue;
        }
        put(
            out,
            w,
            h,
            (cx as i32 + d) as usize,
            cy as usize,
            color,
        );
        put(
            out,
            w,
            h,
            cx as usize,
            (cy as i32 + d) as usize,
            color,
        );
    }
}

fn put(out: &mut [u8], w: usize, h: usize, x: usize, y: usize, color: [u8; 4]) {
    if x >= w || y >= h {
        return;
    }
    let i = (y * w + x) * 4;
    out[i..i + 4].copy_from_slice(&color);
}

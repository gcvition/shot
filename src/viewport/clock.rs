//! 对局时钟、结束条件和轨迹资源。
//!
//! - [`MatchClock`]：开局倒计时 → 计时 → 到点结束。
//! - [`EndCondition`]：怎么算「打完」。M0 只有按时间；M3 起加击杀数/伤害/回合。
//! - [`LiveTrace`]：边打边攒的 `.shot`，只有打满时长才写盘。

use bevy::prelude::*;

use crate::paths::AppPaths;
use crate::session::SessionFile;
use crate::settings::Settings;
use crate::stats::{SessionRecord, StatsDb};

use crate::sce::EndCondition;

use super::{AppPathsRes, ScoreState, SettingsRes, ViewportLaunch};

/// 单帧时钟最多走这么多秒。防止卡顿 1 秒就把 60 秒训练直接判完。
pub(crate) const MAX_CLOCK_DELTA: f32 = 0.25;

/// 一局的时间状态。
///
/// `countdown` 是开局倒计时（设置里 0..=30 秒）。倒计时没走完时
/// `elapsed` 不加，也不能开火。
#[derive(Resource)]
pub(crate) struct MatchClock {
    pub countdown: f32,
    pub elapsed: f32,
    pub duration: f32,
    /// M3 起由 `ScenarioSpec.end` 驱动提前结束。
    #[allow(dead_code)]
    pub end: EndCondition,
    pub finished: bool,
}

/// Play 模式边打边写的轨迹。结束时存成 `cache/sessions/<id>.shot`。
#[derive(Resource)]
pub(crate) struct LiveTrace {
    pub file: SessionFile,
}

/// 单帧 dt 钳在 0.25s，避免卡顿一帧把 60 秒直接判完。
pub(crate) fn clock_step(delta_secs: f32) -> f32 {
    delta_secs.clamp(0.0, MAX_CLOCK_DELTA)
}

/// 倒计时走完才开始 `elapsed`。到点后写 `.shot`、插入 SQLite、通知菜单读最后一局。
#[allow(clippy::too_many_arguments)]
pub(crate) fn tick_clock(
    time: Res<Time>,
    mut clock: ResMut<MatchClock>,
    mut trace: ResMut<LiveTrace>,
    mut score: ResMut<ScoreState>,
    launch: Res<ViewportLaunch>,
    paths: Res<AppPathsRes>,
    settings: Res<SettingsRes>,
    mut exit: MessageWriter<AppExit>,
) {
    if clock.finished {
        return;
    }
    let dt = clock_step(time.delta_secs());
    if clock.countdown > 0.0 {
        clock.countdown = (clock.countdown - dt).max(0.0);
        return;
    }
    clock.elapsed += dt;
    score.total_ticks += 1;
    if clock.elapsed >= clock.duration {
        clock.finished = true;
        finish_play(&mut trace, &score, &launch, &paths.0, &settings.0);
        exit.write(AppExit::Success);
    }
}

/// 打满时长才调用：写 `.shot`、插入成绩库、更新 last_session。
/// Esc 中途退出走 `handle_escape`，不要经过这里。
fn finish_play(
    trace: &mut LiveTrace,
    score: &ScoreState,
    launch: &ViewportLaunch,
    paths: &AppPaths,
    settings: &Settings,
) {
    let duration_ms = (launch.scenario.duration_secs * 1000.0) as u32;
    trace.file.header.duration_ms = duration_ms;
    let _ = trace.file.save(paths);
    if let Ok(db) = StatsDb::open(paths) {
        let rec = SessionRecord {
            id: launch.session_id.clone(),
            scenario: launch.scenario.id.clone(),
            started_at: trace.file.header.started_at_unix_ms,
            duration_ms,
            score: score.hits,
            hits: score.hits,
            misses: score.misses,
            cm_per_360: settings.cm_per_360,
            dpi: settings.dpi,
            fov_deg: settings.fov_deg,
            render_width: settings.render_width,
            render_height: settings.render_height,
            trace_path: paths
                .session_trace(&launch.session_id)
                .display()
                .to_string(),
            video_path: None,
            rng_seed: trace.file.header.rng_seed as i64,
            score_f: score.score,
            scoring_version: 0,
            damage: score.damage,
            kills: score.kills,
            headshots: score.headshots,
            difficulty: None,
            playlist_id: None,
            category: launch
                .scenario
                .category
                .map(|cat| cat.as_str().to_string()),
            schema_version: i64::from(crate::session::SCHEMA_VERSION),
        };
        let _ = db.insert(&rec);
    }
    let _ = crate::launch::write_last_session(paths, &launch.session_id);
}

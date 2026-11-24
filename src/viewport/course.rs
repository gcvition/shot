//! 课程连播（M7）：`Intermission` 状态的两个系统。
//!
//! - [`round_reset`]（`OnEnter(Intermission)`）：上一关的 session 已经由
//!   `tick_clock` → `finish_play` 写盘；这里清掉所有靶子/弹道，
//!   换 `PlanRun::next_spec` 当新 spec，重建时钟/计分/武器/血量/导演/轨迹。
//!   不清资源句柄——网格/材质缓存跨关复用。
//! - [`intermission_tick`]（Update, `in_state(Intermission)`）：结果卡 6s 或
//!   按 Enter 提前开下一关（进 Countdown）。

use bevy::prelude::*;
use rand::SeedableRng;

use crate::playlist::PlanRun;
use crate::session::{SessionFile, SettingsSnapshot};

use super::combat::{PlayerHp, Projectile, WeaponState};
use super::director::{self, SpawnDirector};
use super::player::Look;
use super::targets::{MeshCache, NextTargetId, RngRes, Target, TargetMaterials};
use super::{
    LiveTrace, MatchClock, PlayerBody, ScoreState, SettingsRes, ViewportLaunch, ViewportMode,
};

/// 上一关打完进 Intermission：换 spec，重置整局资源，重生首批目标。
#[allow(clippy::too_many_arguments)]
pub(crate) fn round_reset(
    mut commands: Commands,
    targets: Query<Entity, Or<(With<Target>, With<Projectile>)>>,
    mut launch: ResMut<ViewportLaunch>,
    mut plan: ResMut<PlanRun>,
    mut clock: ResMut<MatchClock>,
    mut score: ResMut<ScoreState>,
    mut php: ResMut<PlayerHp>,
    mut body: ResMut<PlayerBody>,
    mut look: ResMut<Look>,
    mut rng: ResMut<RngRes>,
    mut next_id: ResMut<NextTargetId>,
    mut trace: ResMut<LiveTrace>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut cache: ResMut<MeshCache>,
    mats: Res<TargetMaterials>,
    settings: Res<SettingsRes>,
) {
    let Some(spec) = plan.next_spec.take() else {
        return;
    };
    for e in targets.iter() {
        commands.entity(e).despawn();
    }
    launch.scenario = spec;
    launch.session_id = uuid::Uuid::new_v4().to_string();
    let seed: u64 = rand::random();
    rng.0 = rand_chacha::ChaCha8Rng::seed_from_u64(seed);
    // finished 保持 true：Intermission 期间所有玩法系统冻结，
    // 由 intermission_tick 决定何时放 finished=false → Countdown。
    *clock = MatchClock {
        countdown: 0.0,
        elapsed: 0.0,
        duration: launch.scenario.duration_secs,
        end: launch.scenario.end,
        finished: true,
    };
    *score = ScoreState::default();
    let hp_max = if launch.scenario.player.hp > 0.0 {
        launch.scenario.player.hp
    } else {
        100.0
    };
    *php = PlayerHp {
        hp: hp_max,
        max: hp_max,
        invuln_until: 0.0,
    };
    *body = PlayerBody {
        pos: launch.scenario.player.start_pos,
        prev_pos: launch.scenario.player.start_pos,
        ..Default::default()
    };
    *look = Look {
        yaw: launch.scenario.player.start_yaw_deg,
        pitch: 0.0,
    };
    commands.insert_resource(WeaponState::for_weapon(&launch.scenario.weapon));
    commands.insert_resource(SpawnDirector::new(
        launch.scenario.spawns.clone(),
        seed,
    ));
    let snapshot = SettingsSnapshot::from_settings(
        &settings.0,
        settings.0.render_width,
        settings.0.render_height,
        60,
    );
    *trace = LiveTrace {
        file: SessionFile::new(
            launch.session_id.clone(),
            launch.scenario.id.clone(),
            seed,
            snapshot,
        ),
    };
    // 首批目标（v2）；legacy 课程项不该出现，出现也不崩。
    if launch.mode == ViewportMode::Play && !launch.scenario.spawns.is_empty() {
        let look_dir = crate::scenario::look_direction(0.0, 0.0);
        let director = SpawnDirector::new(launch.scenario.spawns.clone(), seed);
        director::spawn_initial(
            &director,
            &mut commands,
            &mut meshes,
            &mut cache,
            &mats.normal,
            mats.no_shoot.as_ref(),
            &mut rng.0,
            &mut next_id.0,
            &mut trace.file,
            look_dir,
            crate::scenario::PLAYER_EYE,
            &launch.scenario.map.blocks,
        );
        commands.insert_resource(director);
    }
}

/// 结果卡：Enter 直接开下一关，否则 6s 自动开。
pub(crate) fn intermission_tick(
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    mut wait: Local<f32>,
    mut clock: ResMut<MatchClock>,
    settings: Res<SettingsRes>,
) {
    *wait += time.delta_secs();
    if keys.just_pressed(KeyCode::Enter) || *wait >= 6.0 {
        *wait = 0.0;
        clock.finished = false;
        clock.countdown = settings.0.countdown_secs as f32;
    }
}

/// 给 HUD 提供课间文案（`update_hud` 在 Intermission 状态调用）。
pub(crate) fn intermission_label(plan: &PlanRun) -> String {
    // idx 已指向下一关；刚打完的是 idx-1，成绩取 results 末尾（score 已重置）。
    let last = plan.results.last();
    let last_score = last.map(|(_, _, s)| *s).unwrap_or(0.0);
    let next = plan.playlist.scenarios.get(plan.idx).unwrap_or(&"-");
    format!(
        "第 {}/{} 关完成  score {:.0}   下一关：{}   [Enter]",
        plan.idx,
        plan.playlist.scenarios.len(),
        last_score,
        next
    )
}

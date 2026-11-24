//! HUD：倒计时 / 剩余时间 / 回放倍速文字。只改带 `HudText` 的那一行。

use bevy::prelude::*;

use crate::sce::{FireMode, ScoringMode, WeaponKind};

use super::combat::{PlayerHp, WeaponState};
use super::{
    MatchClock, PlayerBody, ReplayCtl, RunState, ScoreState, ViewportLaunch, setup::HudText,
};

pub(crate) fn update_hud(
    clock: Res<MatchClock>,
    score: Res<ScoreState>,
    wstate: Option<Res<WeaponState>>,
    php: Option<Res<PlayerHp>>,
    body: Option<Res<PlayerBody>>,
    replay: Option<Res<ReplayCtl>>,
    launch: Res<ViewportLaunch>,
    state: Option<Res<State<RunState>>>,
    plan: Option<Res<crate::playlist::PlanRun>>,
    mut text: Query<&mut Text2d, With<HudText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
    if let (Some(state), Some(plan)) = (state.as_ref(), plan.as_ref()) {
        if **state == RunState::Intermission {
            *text = Text2d::new(super::course::intermission_label(plan));
            return;
        }
    }
    if let Some(replay) = replay {
        let remain = (replay.file.header.duration_ms as f32 / 1000.0 - replay.time).max(0.0);
        *text = Text2d::new(format!(
            "{}:{:02}  replay x{:.2}",
            remain as u32 / 60,
            remain as u32 % 60,
            replay.speed
        ));
    } else if clock.countdown > 0.0 {
        *text = Text2d::new(format!("{}", clock.countdown.ceil() as u32));
    } else {
        let remain = (clock.duration - clock.elapsed).max(0.0);
        let spec = &launch.scenario;
        // 跟枪场景显示实时命中率；其余显示击杀。
        let stat = if matches!(spec.scoring.mode, ScoringMode::OnTargetPercent)
            || spec.weapon.kind == WeaponKind::Beam
            || spec.weapon.fire_mode == FireMode::Auto
        {
            let pct = if score.total_ticks == 0 {
                0.0
            } else {
                100.0 * score.on_target_ticks as f64 / score.total_ticks as f64
            };
            format!("{pct:.1}%")
        } else if matches!(spec.scoring.mode, ScoringMode::ReactionMs) {
            let avg = if score.reaction_count == 0 {
                0.0
            } else {
                score.reaction_ms_sum / score.reaction_count as f64
            };
            format!("{avg:.0}ms")
        } else {
            format!("{}", score.hits)
        };
        // 弹匣武器显示余弹/换弹状态。
        let ammo = match (spec.weapon.magazine, wstate) {
            (Some(_), Some(w)) if w.reload_end > 0.0 => "   RLD".to_string(),
            (Some(mag), Some(w)) => format!("   {}/{}", w.mag_left.max(0), mag),
            _ => String::new(),
        };
        // AI 对枪场景显示血量。
        let hp_txt = match php {
            Some(p) if launch.scenario.spawns.iter().any(|g| g.bot.is_some()) => {
                format!("   HP {:.0}", p.hp.max(0.0))
            }
            _ => String::new(),
        };
        // 移动场景显示水平速度。
        let speed = match body {
            Some(b) if launch.scenario.player.movement_preset != crate::sce::MovementPreset::Static => {
                let hs = (b.vel.x * b.vel.x + b.vel.z * b.vel.z).sqrt();
                format!("   {:.1}m/s", hs)
            }
            _ => String::new(),
        };
        *text = Text2d::new(format!(
            "{}:{:02}   {}{}{}{}",
            remain as u32 / 60,
            remain as u32 % 60,
            stat,
            ammo,
            speed,
            hp_txt
        ));
    }
}

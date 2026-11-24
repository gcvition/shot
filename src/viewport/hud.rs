//! HUD：倒计时 / 剩余时间 / 回放倍速文字。只改带 `HudText` 的那一行。

use bevy::prelude::*;

use crate::sce::{FireMode, ScoringMode, WeaponKind};

use super::{MatchClock, ReplayCtl, ScoreState, ViewportLaunch, setup::HudText};

pub(crate) fn update_hud(
    clock: Res<MatchClock>,
    score: Res<ScoreState>,
    replay: Option<Res<ReplayCtl>>,
    launch: Res<ViewportLaunch>,
    mut text: Query<&mut Text2d, With<HudText>>,
) {
    let Ok(mut text) = text.single_mut() else {
        return;
    };
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
        *text = Text2d::new(format!(
            "{}:{:02}   {}",
            remain as u32 / 60,
            remain as u32 % 60,
            stat
        ));
    }
}

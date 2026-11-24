//! HUD：倒计时 / 剩余时间 / 回放倍速文字。只改带 `HudText` 的那一行。

use bevy::prelude::*;

use super::{MatchClock, ReplayCtl, ScoreState, setup::HudText};

pub(crate) fn update_hud(
    clock: Res<MatchClock>,
    score: Res<ScoreState>,
    replay: Option<Res<ReplayCtl>>,
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
        *text = Text2d::new(format!(
            "{}:{:02}   {}",
            remain as u32 / 60,
            remain as u32 % 60,
            score.hits
        ));
    }
}

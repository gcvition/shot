//! 局内状态机：为回合制（M6）和课程连播（M7）准备的 Bevy States。
//!
//! M0 里它只是 [`MatchClock`] 的**镜像**：[`sync_run_state`] 每帧把时钟状态
//! 翻译进来，逻辑本身仍读 `MatchClock`（保证行为和重构前逐位一致）。
//! 之后的里程碑再把「系统跑不跑」挂到 `OnEnter / in_state` 上。
//!
//! - `Loading`：生成地图、目标、Bot（将来切场景时在 `OnEnter` 做）。
//! - `Countdown`：能转视角，不能开火，时间不计（和现在一致）。
//! - `Playing`：正常进行。
//! - `RoundEnd`：对枪一回合结束，短暂停顿后复位。
//! - `Intermission`：课程连播时，上一个场景结果卡 + 「下一个」。
//! - `Finished`：写盘并退出（单场景）或回 `Intermission`（课程）。

use bevy::prelude::*;

use super::MatchClock;

#[derive(States, Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunState {
    #[default]
    Loading,
    Countdown,
    Playing,
    /// M3 回合制才进入。
    #[allow(dead_code)]
    RoundEnd,
    /// M3 课程连播的回合间隙。
    #[allow(dead_code)]
    Intermission,
    Finished,
}

/// 把 `MatchClock` 翻成 `RunState`。放在 `PlaySet::Clock` 的 `tick_clock` 后面，
/// 读到的是本帧已经推进过的时钟。
pub(crate) fn sync_run_state(
    clock: Res<MatchClock>,
    plan: Option<Res<crate::playlist::PlanRun>>,
    mut next: ResMut<NextState<RunState>>,
) {
    let state = if clock.finished {
        // 课程还有下一关 → 课间；否则真结束。
        if plan.is_some_and(|p| p.idx < p.playlist.scenarios.len()) {
            RunState::Intermission
        } else {
            RunState::Finished
        }
    } else if clock.countdown > 0.0 {
        RunState::Countdown
    } else {
        RunState::Playing
    };
    next.set(state);
}

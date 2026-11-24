//! 和真实窗口打交道的事：拉伸 RT、锁鼠标、Esc 退出。

use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::{MatchClock, ViewportLaunch, ViewportMode, setup::PresentSprite};

/// 无边框时窗口可能比 RT 大：把 Present Sprite 拉成窗口大小（会拉伸，这是刻意的）。
pub(crate) fn stretch_present(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut sprite: Query<&mut Sprite, With<PresentSprite>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let Ok(mut sprite) = sprite.single_mut() else {
        return;
    };
    sprite.custom_size = Some(Vec2::new(window.width(), window.height()));
}

/// 训练时窗口聚焦就锁鼠标；回放要点一下左键才锁，方便拖动窗口。
pub(crate) fn grab_cursor(
    mut cursors: Query<&mut CursorOptions, With<PrimaryWindow>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mouse: Res<ButtonInput<MouseButton>>,
    launch: Res<ViewportLaunch>,
) {
    let Ok(mut cursor) = cursors.single_mut() else {
        return;
    };
    let Ok(mut window) = windows.single_mut() else {
        return;
    };
    let want_grab = match launch.mode {
        ViewportMode::Play => window.focused,
        ViewportMode::Replay => {
            mouse.just_pressed(MouseButton::Left) || cursor.grab_mode != CursorGrabMode::None
        }
    };
    if !want_grab {
        return;
    }
    if cursor.visible {
        cursor.visible = false;
    }
    if cursor.grab_mode != CursorGrabMode::Confined {
        cursor.grab_mode = CursorGrabMode::Confined;
        let center = Vec2::new(window.width() * 0.5, window.height() * 0.5);
        window.set_cursor_position(Some(center));
    }
}

/// Esc：立刻关窗口。没打满时长就不写 `.shot`、不进成绩库。
pub(crate) fn handle_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut clock: ResMut<MatchClock>,
    launch: Res<ViewportLaunch>,
    mut exit: MessageWriter<AppExit>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if launch.mode == ViewportMode::Play && !clock.finished {
        clock.finished = true;
    }
    exit.write(AppExit::Success);
}

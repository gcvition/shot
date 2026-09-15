//! 菜单窗口（GPUI Kit），按 **MVI** 拆开，方便二次开发。
//!
//! # 两个进程
//!
//! 双击 `shot.exe` 进入这里：640×480 的设置/选场景窗口。点「开始训练」后，
//! [`crate::launch`] 会再拉起 **同一个 exe**，参数是 `play <场景> <对局id>`，
//! 那次进程跑 [`crate::viewport`]（Bevy）。菜单窗口会一直等到 Bevy 退出。
//!
//! # MVI 数据流（改 UI 请顺着这条线走）
//!
//! ```text
//!   View (view.rs)          画出当前 Model，点击只发出 Intent
//!        │
//!        │  Intent（intent.rs）= 「用户想做什么」
//!        ▼
//!   Update (update.rs)      Shell::dispatch 是唯一改状态的入口
//!        │
//!        ▼
//!   Model (model.rs)        设置、历史、以及 Input/Select/Slider 实体
//!        │
//!        └── cx.notify() → GPUI 再调 View::render
//! ```
//!
//! 控件自己的事件（输入、下拉、滑块）在 `model.rs` 里订阅，同样转成 Intent，
//! 不会在 `render` 里改列表或写 Settings。
//!
//! # 想加一个按钮？
//!
//! 1. `intent.rs` 加一个 `Intent` 变体  
//! 2. `update.rs` 的 `dispatch` 里处理它  
//! 3. `view.rs` 里 `this.dispatch(Intent::..., window, cx)`

mod intent;
mod model;
mod update;
mod view;
mod widgets;

use gpui_kit::component::Root;
use gpui_kit::*;

use crate::paths::AppPaths;
use crate::settings::Settings;

use model::Shell;
use widgets::{SHELL_HEIGHT, SHELL_WIDTH};

/// 启动菜单窗口。失败通常是资源目录找不到或窗口创建失败。
pub fn run() -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    paths.ensure_dirs()?;
    let settings = Settings::load_or_default(&paths)?;
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(SHELL_WIDTH), px(SHELL_HEIGHT)), cx);
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                is_resizable: false,
                is_minimizable: true,
                window_min_size: Some(size(px(SHELL_WIDTH), px(SHELL_HEIGHT))),
                titlebar: Some(TitlebarOptions {
                    title: Some(format!("Shot {}", crate::version_label()).into()),
                    appears_transparent: false,
                    ..Default::default()
                }),
                ..Default::default()
            };
            let settings = settings.clone();
            let paths = paths.clone();
            cx.spawn(async move |cx| {
                cx.open_window(options, |window, cx| {
                    window.resize(size(px(SHELL_WIDTH), px(SHELL_HEIGHT)));
                    let view = cx.new(|cx| Shell::new(paths, settings, window, cx));
                    cx.new(|cx| Root::new(view, window, cx))
                })
                .expect("failed to open window");
            })
            .detach();
        });
    Ok(())
}

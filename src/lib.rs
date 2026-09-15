//! Shot：桌面端瞄准训练器。
//!
//! 仓库里其实是 **两个程序共用一个 exe**：
//!
//! 1. **菜单进程**（GPUI Kit）— 选场景、改灵敏度、看成绩。按 **MVI** 写在 [`shell`]。
//! 2. **训练/回放进程**（Bevy 0.19）— 真正的 3D 房间和射击。写在 [`viewport`]。
//!
//! 双击 `shot.exe` 进菜单。点「开始训练」时 [`launch`] 会再启动 **同一份 exe**，
//! 参数是 `play <场景id> <对局id>`。菜单窗口会卡住，直到 Bevy 窗口退出。
//!
//! # 二次开发从哪改
//!
//! | 你想做的事 | 去哪里 |
//! |------------|--------|
//! | 加菜单按钮 / 改设置页 | [`shell`]：`intent.rs` → `update.rs` → `view.rs` |
//! | 改开火、倒计时、目标生成 | [`viewport`] 的 Bevy systems |
//! | 加一种新场景规则 | `res/scenarios/*.sce` + [`sce`] / [`scenario`] |
//! | 改灵敏度公式 | [`sensitivity`] |
//! | 改命中判定 | [`hitscan`] |
//! | 改房间贴图、准星、回放轨迹 | [`appearance`] |
//!
//! # 磁盘上的东西
//!
//! - `res/`：主题、音效、准星、`.sce` 场景（跟仓库一起走）
//! - `cache/`：`settings.json`、`stats.db`、`sessions/*.shot`（运行时生成）
//!
//! 路径解析见 [`paths::AppPaths::discover`]：优先 exe 旁边的 `res/`，开发时回退到 crate 根目录。
//!
//! # 模块一览
//!
//! | 模块 | 职责 |
//! |------|------|
//! | [`shell`] | GPUI 菜单。**MVI**：Intent → Update → Model → View |
//! | [`viewport`] | Bevy 训练/回放。3D 画到一张图，再由 2D 相机贴到全屏 |
//! | [`launch`] | 菜单进程 spawn 自己，带 `play` / `replay` |
//! | [`sce`] | 读 `res/scenarios/*.sce`（KovaaK 风格 INI） |
//! | [`scenario`] | 墙面随机点 / 3×3 格子的生成数学 |
//! | [`session`] | 一局的鼠标、开火、出生点，存 `.shot` |
//! | [`stats`] | SQLite 成绩表 |
//! | [`settings`] | `cache/settings.json` |
//! | [`sensitivity`] | 厘米/360° + DPI → 每一鼠标 count 转多少度 |
//! | [`hitscan`] / [`project`] | 射线打球、世界坐标投到屏幕 |
//! | [`appearance`] | 房间贴图、准星裁剪、回放轨迹网格 |
//! | [`theme`] | KovaaK 主题 JSON |
//! | [`sfx`] / [`wav`] | 命中/未命中音效线程 |
//! | [`display`] | Windows 当前显示器分辨率列表 |
//! | [`fov`] | 水平/垂直 FOV 换成 Bevy 要的垂直弧度 |
//! | [`vec3`] | 序列化用的小向量（不依赖 Bevy） |

pub mod appearance;
pub mod assets;
pub mod display;
pub mod error;
pub mod fov;
pub mod hitscan;
pub mod launch;
pub mod paths;
pub mod project;
pub mod sce;
pub mod scenario;
pub mod sensitivity;
pub mod session;
pub mod settings;
pub mod sfx;
pub mod stats;
pub mod theme;
pub mod vec3;
pub mod wav;

pub mod shell;
pub mod viewport;

pub use error::{Result, ShotError};
pub use settings::Settings;

/// 与 `Cargo.toml` 里 `[package].version` 同步。改 toml 后重新编译，界面上的版本会一起变。
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// 给人看的版本，例如 `v0.1.0`。窗口标题和菜单顶栏都用它。
pub fn version_label() -> String {
    format!("v{VERSION}")
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_should_match_cargo_toml_package_version() {
        let manifest = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
        let mut in_package = false;
        let mut found = None;
        for line in manifest.lines() {
            let line = line.trim();
            if line.starts_with('[') {
                in_package = line == "[package]";
                continue;
            }
            if in_package {
                if let Some(value) = line
                    .strip_prefix("version")
                    .and_then(|rest| rest.trim().strip_prefix('='))
                {
                    found = Some(value.trim().trim_matches('"').to_string());
                    break;
                }
            }
        }
        assert_eq!(found.as_deref(), Some(crate::VERSION));
        assert_eq!(crate::version_label(), format!("v{}", crate::VERSION));
    }
}

//! Intent：用户「想做什么」的完整清单。
//!
//! MVI 里 View 和控件订阅都 **只构造 Intent**，不直接改 Model。
//! 真正改状态的代码全部在 [`crate::shell::update`] 的 `Shell::dispatch`。
//!
//! 二次开发：加一个按钮时，先在这里加一个变体，再到 `update.rs` 写处理，
//! 最后在 `view.rs` 里 `dispatch(Intent::YourThing)`。

use crate::fov::FovKind;
use crate::settings::DisplayMode;

/// 主窗口三个页面。Tab 的下标必须与这个顺序一致：0 训练 / 1 设置 / 2 记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Page {
    Train,
    Settings,
    Results,
}

impl Page {
    pub(crate) fn from_tab(index: usize) -> Self {
        match index {
            1 => Self::Settings,
            2 => Self::Results,
            _ => Self::Train,
        }
    }

    pub(crate) fn tab_index(self) -> usize {
        match self {
            Self::Train => 0,
            Self::Settings => 1,
            Self::Results => 2,
        }
    }
}

/// 设置里所有「数字框」共用这一个枚举，方便 `EditNumber` 走同一条更新路径。
#[derive(Clone, Copy, Debug)]
pub(crate) enum NumField {
    Countdown,
    Width,
    Height,
    Cm360,
    Dpi,
    Fov,
}

/// 主题 / 准星 / 音效都是「从 res/ 里挑一个文件」，用槽位区分写到 Settings 的哪一栏。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetSlot {
    Theme,
    Crosshair,
    HitSound,
    MissSound,
}

/// 下拉框数据源。打开下拉时会重新扫盘，所以用 Intent 触发刷新，而不是在 `render` 里改列表。
#[derive(Clone, Copy, Debug)]
pub(crate) enum SelectKind {
    History,
    Resolution,
    Asset(AssetSlot),
}

/// 一次用户意图。保持「动词 + 必要数据」，不要塞 GPUI 控件实体进来。
#[derive(Clone, Debug)]
pub(crate) enum Intent {
    /// 切换 Tab。
    OpenPage(Page),
    /// 启动一局训练（会再 spawn 一个 Bevy 进程，当前窗口会卡住直到那局结束）。
    StartScenario(String),
    /// 回放已有对局。
    ReplaySession(String),
    /// 弹出删除确认框（还没真正删除）。
    AskDeleteSession { id: String, name: String },
    /// 确认后删除 SQLite 行和回放文件。
    DeleteSession(String),
    /// 历史列表按场景过滤；`None` 表示全部。
    FilterHistory(Option<String>),
    /// 把内存里的 Settings 写到 `cache/settings.json`。
    SaveSettings { announce: bool },
    SetAsset { slot: AssetSlot, path: String },
    SetResolution { width: u32, height: u32 },
    SetFovKind(FovKind),
    SetDisplayMode(DisplayMode),
    /// 数字框改了。`commit` 为 true 表示失焦/回车，这时才钳制并回写格式化文本。
    EditNumber {
        field: NumField,
        raw: String,
        commit: bool,
    },
    /// 瞄准相关滑块拖动（厘米/360、DPI、FOV）。
    SlideAim { field: NumField, value: f64 },
    SlideVolume(f32),
    /// Select 打开或数据变化时，重新扫描 res/ 并同步选中项。
    RefreshSelect(SelectKind),
}

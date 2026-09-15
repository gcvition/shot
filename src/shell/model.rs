//! Model：菜单窗口真正记住的东西。
//!
//! - **领域数据**：设置、当前页、历史记录、最近一局
//! - **控件实体**：GPUI 的 Input/Slider/Select 必须跨帧活着，所以也放在这里
//! - **订阅**：控件事件 → `Intent` → `dispatch`，View 里不要再偷偷改字段
//!
//! `Shell` 同时是 GPUI `Entity`，所以它既是 Store，也是 `Render` 的 `self`。

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::select::{SelectEvent, SelectState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::IndexPath;
use gpui_kit::*;

use crate::assets;
use crate::paths::AppPaths;
use crate::settings::Settings;
use crate::stats::SessionRecord;

use super::intent::{AssetSlot, Intent, NumField, Page, SelectKind};
use super::widgets::{
    self, ChoiceSelect, DPI_MAX, DPI_MIN, FOV_MAX, FOV_MIN, CM360_MAX, CM360_MIN, SHELL_HEIGHT,
    SHELL_WIDTH, asset_choices, choice_sig, format_cm360, format_fov, history_choices, number_state,
    res_value, resolution_choices, selected_ix, slider_state,
};

/// 菜单窗口的完整 Model。字段不要改成 `pub`：外面只能通过 Intent 改它。
///
/// GPUI 的 Input/Slider/Select 是独立 Entity，必须存在 Model 里才能跨帧活着。
/// `*_sig` 是选项列表的指纹：没变就不要 `set_items`，否则 observe 会死循环。
pub(crate) struct Shell {
    pub(super) paths: AppPaths,
    pub(super) settings: Settings,
    pub(super) page: Page,
    /// 刚打完或上次退出前的那一局，给「记录」页用。
    pub(super) last: Option<SessionRecord>,
    /// 同一场景最近若干局，给得分折线图（时间正序）。
    pub(super) trend: Vec<SessionRecord>,
    /// 「训练」页历史列表（新到旧）。
    pub(super) history: Vec<SessionRecord>,
    pub(super) history_filter: Option<String>,
    pub(super) countdown: Entity<InputState>,
    pub(super) res_w: Entity<InputState>,
    pub(super) res_h: Entity<InputState>,
    pub(super) cm360_input: Entity<InputState>,
    pub(super) dpi_input: Entity<InputState>,
    pub(super) fov_input: Entity<InputState>,
    pub(super) cm360: Entity<SliderState>,
    pub(super) dpi: Entity<SliderState>,
    pub(super) fov: Entity<SliderState>,
    pub(super) volume: Entity<SliderState>,
    pub(super) history_select: ChoiceSelect,
    pub(super) resolution_select: ChoiceSelect,
    pub(super) theme_select: ChoiceSelect,
    pub(super) crosshair_select: ChoiceSelect,
    pub(super) hit_select: ChoiceSelect,
    pub(super) miss_select: ChoiceSelect,
    pub(super) history_sig: u64,
    pub(super) resolution_sig: u64,
    pub(super) theme_sig: u64,
    pub(super) crosshair_sig: u64,
    pub(super) hit_sig: u64,
    pub(super) miss_sig: u64,
    /// 必须挂在 Entity 上，否则订阅一 drop，控件就变哑巴。
    _subscriptions: Vec<Subscription>,
}

impl Shell {
    /// 创建 Model：造控件、订事件、必要时跳到「记录」页。
    pub(crate) fn new(
        paths: AppPaths,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        window.resize(size(px(SHELL_WIDTH), px(SHELL_HEIGHT)));

        let countdown = number_state(
            window,
            cx,
            settings.countdown_secs.to_string(),
            0.0,
            30.0,
            1.0,
        );
        let res_w = number_state(
            window,
            cx,
            settings.render_width.to_string(),
            640.0,
            7680.0,
            2.0,
        );
        let res_h = number_state(
            window,
            cx,
            settings.render_height.to_string(),
            480.0,
            4320.0,
            2.0,
        );
        let cm360_input = number_state(
            window,
            cx,
            format_cm360(settings.cm_per_360),
            CM360_MIN,
            CM360_MAX,
            0.05,
        );
        let dpi_input = number_state(
            window,
            cx,
            settings.dpi.to_string(),
            DPI_MIN,
            DPI_MAX,
            1.0,
        );
        let fov_input = number_state(
            window,
            cx,
            format_fov(settings.fov_deg),
            FOV_MIN,
            FOV_MAX,
            1.0,
        );
        let cm360 = cx.new(|_| {
            slider_state(
                CM360_MIN as f32,
                CM360_MAX as f32,
                0.05,
                settings.cm_per_360 as f32,
            )
        });
        let dpi = cx.new(|_| {
            slider_state(DPI_MIN as f32, DPI_MAX as f32, 50.0, settings.dpi as f32)
        });
        let fov = cx.new(|_| slider_state(FOV_MIN as f32, FOV_MAX as f32, 1.0, settings.fov_deg));
        let volume = cx.new(|_| slider_state(0.0, 1.0, 0.05, settings.master_volume));

        let history_items = history_choices(&paths, None);
        let history_sig = choice_sig(&history_items);
        let history_select =
            cx.new(|cx| SelectState::new(history_items, Some(IndexPath::new(0)), window, cx));

        let resolution_items = resolution_choices(settings.render_width, settings.render_height);
        let resolution_sig = choice_sig(&resolution_items);
        let resolution_value = res_value(settings.render_width, settings.render_height);
        let resolution_ix = selected_ix(&resolution_items, &resolution_value);
        let resolution_select = cx.new(|cx| {
            SelectState::new(resolution_items, resolution_ix, window, cx).searchable(true)
        });

        let theme_items = asset_choices(&assets::list_themes(&paths), &settings.theme_path);
        let theme_sig = choice_sig(&theme_items);
        let theme_ix = selected_ix(&theme_items, &settings.theme_path);
        let theme_select =
            cx.new(|cx| SelectState::new(theme_items, theme_ix, window, cx).searchable(true));

        let crosshair_items =
            asset_choices(&assets::list_crosshairs(&paths), &settings.crosshair_path);
        let crosshair_sig = choice_sig(&crosshair_items);
        let crosshair_ix = selected_ix(&crosshair_items, &settings.crosshair_path);
        let crosshair_select = cx
            .new(|cx| SelectState::new(crosshair_items, crosshair_ix, window, cx).searchable(true));

        let hit_items = asset_choices(&assets::list_sounds(&paths), &settings.hit_sound);
        let hit_sig = choice_sig(&hit_items);
        let hit_ix = selected_ix(&hit_items, &settings.hit_sound);
        let hit_select =
            cx.new(|cx| SelectState::new(hit_items, hit_ix, window, cx).searchable(true));

        let miss_items = asset_choices(&assets::list_sounds(&paths), &settings.miss_sound);
        let miss_sig = choice_sig(&miss_items);
        let miss_ix = selected_ix(&miss_items, &settings.miss_sound);
        let miss_select =
            cx.new(|cx| SelectState::new(miss_items, miss_ix, window, cx).searchable(true));

        let mut subscriptions = Vec::new();
        bind_numeric(&countdown, NumField::Countdown, window, cx, &mut subscriptions);
        bind_numeric(&res_w, NumField::Width, window, cx, &mut subscriptions);
        bind_numeric(&res_h, NumField::Height, window, cx, &mut subscriptions);
        bind_numeric(&cm360_input, NumField::Cm360, window, cx, &mut subscriptions);
        bind_numeric(&dpi_input, NumField::Dpi, window, cx, &mut subscriptions);
        bind_numeric(&fov_input, NumField::Fov, window, cx, &mut subscriptions);
        bind_aim_slider(&cm360, NumField::Cm360, window, cx, &mut subscriptions);
        bind_aim_slider(&dpi, NumField::Dpi, window, cx, &mut subscriptions);
        bind_aim_slider(&fov, NumField::Fov, window, cx, &mut subscriptions);
        bind_volume(&volume, window, cx, &mut subscriptions);

        bind_select_confirm(&history_select, window, cx, &mut subscriptions, |value| {
            Some(Intent::FilterHistory(if value.is_empty() {
                None
            } else {
                Some(value)
            }))
        });
        bind_select_confirm(
            &resolution_select,
            window,
            cx,
            &mut subscriptions,
            |value| {
                widgets::parse_res(&value)
                    .map(|(width, height)| Intent::SetResolution { width, height })
            },
        );
        bind_asset_select(&theme_select, AssetSlot::Theme, window, cx, &mut subscriptions);
        bind_asset_select(
            &crosshair_select,
            AssetSlot::Crosshair,
            window,
            cx,
            &mut subscriptions,
        );
        bind_asset_select(&hit_select, AssetSlot::HitSound, window, cx, &mut subscriptions);
        bind_asset_select(&miss_select, AssetSlot::MissSound, window, cx, &mut subscriptions);

        observe_select(&history_select, SelectKind::History, window, cx, &mut subscriptions);
        observe_select(
            &resolution_select,
            SelectKind::Resolution,
            window,
            cx,
            &mut subscriptions,
        );
        observe_select(
            &theme_select,
            SelectKind::Asset(AssetSlot::Theme),
            window,
            cx,
            &mut subscriptions,
        );
        observe_select(
            &crosshair_select,
            SelectKind::Asset(AssetSlot::Crosshair),
            window,
            cx,
            &mut subscriptions,
        );
        observe_select(
            &hit_select,
            SelectKind::Asset(AssetSlot::HitSound),
            window,
            cx,
            &mut subscriptions,
        );
        observe_select(
            &miss_select,
            SelectKind::Asset(AssetSlot::MissSound),
            window,
            cx,
            &mut subscriptions,
        );

        let mut shell = Self {
            paths,
            settings,
            page: Page::Train,
            last: None,
            trend: Vec::new(),
            history: Vec::new(),
            history_filter: None,
            countdown,
            res_w,
            res_h,
            cm360_input,
            dpi_input,
            fov_input,
            cm360,
            dpi,
            fov,
            volume,
            history_select,
            resolution_select,
            theme_select,
            crosshair_select,
            hit_select,
            miss_select,
            history_sig,
            resolution_sig,
            theme_sig,
            crosshair_sig,
            hit_sig,
            miss_sig,
            _subscriptions: subscriptions,
        };
        shell.reload_history();
        if let Some(id) = crate::launch::read_last_session(&shell.paths)
            .ok()
            .flatten()
        {
            shell.load_session(&id);
            if shell.last.is_some() {
                shell.page = Page::Results;
            }
        }
        shell
    }
}

/// 数字框：Change 只在范围内生效；Blur/Enter 才 clamp 并格式化。
fn bind_numeric(
    input: &Entity<InputState>,
    field: NumField,
    window: &Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
) {
    subscriptions.push(
        cx.subscribe_in(input, window, move |this, state, event, window, cx| {
            if !matches!(
                event,
                InputEvent::Change | InputEvent::Blur | InputEvent::PressEnter { .. }
            ) {
                return;
            }
            this.dispatch(
                Intent::EditNumber {
                    field,
                    raw: state.read(cx).value().to_string(),
                    commit: matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }),
                },
                window,
                cx,
            );
        }),
    );
}

/// 瞄准滑块拖动 → `SlideAim`。和数字框共用 `NumField`。
fn bind_aim_slider(
    slider: &Entity<SliderState>,
    field: NumField,
    window: &Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
) {
    subscriptions.push(cx.subscribe_in(
        slider,
        window,
        move |this, _, event, window, cx| match event {
            SliderEvent::Change(value) | SliderEvent::Release(value) => {
                this.dispatch(
                    Intent::SlideAim {
                        field,
                        value: f64::from(value.end()),
                    },
                    window,
                    cx,
                );
            }
        },
    ));
}

fn bind_volume(
    slider: &Entity<SliderState>,
    window: &Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
) {
    subscriptions.push(cx.subscribe_in(
        slider,
        window,
        move |this, _, event, window, cx| match event {
            SliderEvent::Change(value) | SliderEvent::Release(value) => {
                this.dispatch(Intent::SlideVolume(value.end()), window, cx);
            }
        },
    ));
}

/// Select 点确认才发出 Intent。解析失败（比如分辨率字符串坏了）就忽略。
fn bind_select_confirm<F>(
    select: &ChoiceSelect,
    window: &Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
    to_intent: F,
) where
    F: Fn(String) -> Option<Intent> + 'static,
{
    subscriptions.push(cx.subscribe_in(
        select,
        window,
        move |this, _, event, window, cx| {
            if let SelectEvent::Confirm(Some(value)) = event {
                if let Some(intent) = to_intent(value.to_string()) {
                    this.dispatch(intent, window, cx);
                }
            }
        },
    ));
}

fn bind_asset_select(
    select: &ChoiceSelect,
    slot: AssetSlot,
    window: &Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
) {
    bind_select_confirm(select, window, cx, subscriptions, move |path| {
        Some(Intent::SetAsset { slot, path })
    });
}

/// 打开下拉时刷新磁盘列表。真正改 items 在 `RefreshSelect` 里，不在 render。
fn observe_select(
    select: &ChoiceSelect,
    kind: SelectKind,
    window: &mut Window,
    cx: &mut Context<Shell>,
    subscriptions: &mut Vec<Subscription>,
) {
    subscriptions.push(cx.observe_in(select, window, move |this, _, window, cx| {
        this.dispatch(Intent::RefreshSelect(kind), window, cx);
    }));
}

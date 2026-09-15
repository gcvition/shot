//! Update：唯一允许改 Model 的地方。
//!
//! 新手改功能：先在 `intent.rs` 加变体，再在 `Shell::dispatch` 加一支 match。
//! 不要在 `view.rs` 里写 `self.settings.xxx = ...`。

use gpui_kit::component::button::ButtonVariant;
use gpui_kit::component::dialog::DialogButtonProps;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::*;

use crate::assets;
use crate::sce;
use crate::stats::StatsDb;

use super::intent::{AssetSlot, Intent, NumField, Page, SelectKind};
use super::model::Shell;
use super::widgets::{
    CM360_MAX, CM360_MIN, DPI_MAX, DPI_MIN, FOV_MAX, FOV_MIN, asset_choices, format_cm360,
    format_fov, history_choices, parse_aim_input, replace_choices, res_value, resolution_choices,
    sync_input_text, sync_slider, try_even_res,
};

impl Shell {
    /// MVI 的唯一入口。View 和订阅都调用它。
    pub(crate) fn dispatch(
        &mut self,
        intent: Intent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match intent {
            Intent::OpenPage(page) => self.open_page(page, window, cx),
            Intent::StartScenario(id) => self.start(id, window, cx),
            Intent::ReplaySession(id) => self.replay(&id, window, cx),
            Intent::AskDeleteSession { id, name } => self.confirm_delete(id, name, window, cx),
            Intent::DeleteSession(id) => self.delete_session(&id, window, cx),
            Intent::FilterHistory(filter) => self.set_history_filter(filter, cx),
            Intent::SaveSettings { announce } => {
                self.persist_settings(window, cx, announce);
                cx.notify();
            }
            Intent::SetAsset { slot, path } => self.set_asset(slot, path, window, cx),
            Intent::SetResolution { width, height } => {
                self.set_resolution(width, height, window, cx)
            }
            Intent::SetFovKind(kind) => {
                self.settings.fov_kind = kind;
                cx.notify();
            }
            Intent::SetDisplayMode(mode) => {
                self.settings.display_mode = mode;
                cx.notify();
            }
            Intent::EditNumber { field, raw, commit } => {
                self.apply_numeric(field, &raw, commit, window, cx);
                cx.notify();
            }
            Intent::SlideAim { field, value } => {
                self.apply_aim(field, value, true, window, cx);
                cx.notify();
            }
            Intent::SlideVolume(value) => {
                self.settings.master_volume = value.clamp(0.0, 1.0);
                cx.notify();
            }
            Intent::RefreshSelect(kind) => self.refresh_select(kind, window, cx),
        }
    }

    /// 切 Tab。设置页顺手重扫资源；训练/记录页重载历史。
    fn open_page(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        self.page = page;
        match page {
            Page::Settings => {
                self.refresh_select(SelectKind::Resolution, window, cx);
                self.refresh_select(SelectKind::Asset(AssetSlot::Theme), window, cx);
                self.refresh_select(SelectKind::Asset(AssetSlot::Crosshair), window, cx);
                self.refresh_select(SelectKind::Asset(AssetSlot::HitSound), window, cx);
                self.refresh_select(SelectKind::Asset(AssetSlot::MissSound), window, cx);
            }
            Page::Train | Page::Results => {
                self.reload_history();
                self.refresh_select(SelectKind::History, window, cx);
            }
        }
        cx.notify();
    }

    /// 数字框。瞄准三项走 `parse_aim_input`；宽高必须变成偶数才写入。
    fn apply_numeric(
        &mut self,
        field: NumField,
        raw: &str,
        commit: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match field {
            NumField::Cm360 | NumField::Dpi | NumField::Fov => {
                if let Some(value) = parse_aim_input(raw, field, commit) {
                    self.apply_aim(field, value, commit, window, cx);
                }
            }
            NumField::Countdown => {
                if let Ok(value) = raw.trim().parse::<u32>() {
                    self.settings.countdown_secs = value.min(30);
                }
            }
            NumField::Width => {
                if let Ok(value) = raw.trim().parse::<u32>() {
                    if let Some((width, _height)) =
                        try_even_res(value, self.settings.render_height)
                    {
                        self.settings.render_width = width;
                        self.refresh_select(SelectKind::Resolution, window, cx);
                    }
                }
            }
            NumField::Height => {
                if let Ok(value) = raw.trim().parse::<u32>() {
                    if let Some((_width, height)) =
                        try_even_res(self.settings.render_width, value)
                    {
                        self.settings.render_height = height;
                        self.refresh_select(SelectKind::Resolution, window, cx);
                    }
                }
            }
        }
    }

    /// 同步滑块和（可选）数字框。从滑块来时 `sync_input` 为 true，避免光标被拽走。
    fn apply_aim(
        &mut self,
        field: NumField,
        value: f64,
        sync_input: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match field {
            NumField::Cm360 => {
                self.settings.cm_per_360 = value.clamp(CM360_MIN, CM360_MAX);
                sync_slider(&self.cm360, self.settings.cm_per_360 as f32, window, cx);
                if sync_input {
                    sync_input_text(
                        &self.cm360_input,
                        format_cm360(self.settings.cm_per_360),
                        window,
                        cx,
                    );
                }
            }
            NumField::Dpi => {
                self.settings.dpi = value.round().clamp(DPI_MIN, DPI_MAX) as u32;
                sync_slider(&self.dpi, self.settings.dpi as f32, window, cx);
                if sync_input {
                    sync_input_text(&self.dpi_input, self.settings.dpi.to_string(), window, cx);
                }
            }
            NumField::Fov => {
                self.settings.fov_deg = (value as f32).clamp(FOV_MIN as f32, FOV_MAX as f32);
                sync_slider(&self.fov, self.settings.fov_deg, window, cx);
                if sync_input {
                    sync_input_text(
                        &self.fov_input,
                        format_fov(self.settings.fov_deg),
                        window,
                        cx,
                    );
                }
            }
            _ => {}
        }
    }

    pub(super) fn reload_history(&mut self) {
        if let Ok(db) = StatsDb::open(&self.paths) {
            self.history = db
                .recent_newest(self.history_filter.as_deref(), 20)
                .unwrap_or_default();
        }
    }

    pub(super) fn load_session(&mut self, id: &str) {
        if let Ok(db) = StatsDb::open(&self.paths) {
            self.last = db.get(id).ok().flatten();
            if let Some(last) = &self.last {
                self.trend = db.recent(&last.scenario, 30).unwrap_or_default();
            } else {
                self.trend.clear();
            }
        }
    }

    fn set_history_filter(&mut self, filter: Option<String>, cx: &mut Context<Self>) {
        self.history_filter = filter;
        self.reload_history();
        cx.notify();
    }

    fn confirm_delete(
        &mut self,
        id: String,
        name: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let view = cx.entity().downgrade();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let view = view.clone();
            let id = id.clone();
            alert
                .confirm()
                .title(format!("删除「{name}」？"))
                .description("删除后无法恢复。")
                .button_props(
                    DialogButtonProps::default()
                        .ok_text("删除")
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text("取消")
                        .show_cancel(true),
                )
                .on_ok(move |_, window, cx| {
                    if let Some(view) = view.upgrade() {
                        view.update(cx, |this, cx| {
                            this.dispatch(Intent::DeleteSession(id.clone()), window, cx);
                        });
                    }
                    true
                })
        });
    }

    fn delete_session(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let was_last = self.last.as_ref().is_some_and(|last| last.id == id);
        match crate::stats::delete_session(&self.paths, id) {
            Ok(()) => {
                if was_last {
                    self.last = None;
                    self.trend.clear();
                    if let Ok(db) = StatsDb::open(&self.paths) {
                        if let Ok(Some(next)) = db.latest() {
                            self.load_session(&next.id);
                        }
                    }
                } else if let Some(current) = self.last.as_ref().map(|last| last.id.clone()) {
                    self.load_session(&current);
                }
                self.reload_history();
                window.push_notification(Notification::success("记录已删除"), cx);
            }
            Err(e) => {
                window.push_notification(Notification::error(format!("删除失败: {e}")), cx);
            }
        }
        cx.notify();
    }

    /// 写 `cache/settings.json`。`announce` 为 true 时弹成功通知（点「保存」）。
    fn persist_settings(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        announce: bool,
    ) -> bool {
        match self.settings.save(&self.paths) {
            Ok(()) => {
                if announce {
                    window.push_notification(Notification::success("设置已保存"), cx);
                }
                true
            }
            Err(e) => {
                window.push_notification(Notification::error(format!("保存失败: {e}")), cx);
                false
            }
        }
    }

    fn set_asset(
        &mut self,
        slot: AssetSlot,
        path: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match slot {
            AssetSlot::Theme => self.settings.theme_path = path,
            AssetSlot::Crosshair => self.settings.crosshair_path = path,
            AssetSlot::HitSound => self.settings.hit_sound = path,
            AssetSlot::MissSound => self.settings.miss_sound = path,
        }
        self.refresh_select(SelectKind::Asset(slot), window, cx);
        cx.notify();
    }

    fn set_resolution(
        &mut self,
        width: u32,
        height: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if crate::settings::validate_resolution(width, height).is_err() {
            return;
        }
        self.settings.render_width = width;
        self.settings.render_height = height;
        self.res_w.update(cx, |state, cx| {
            state.set_value(width.to_string(), window, cx);
        });
        self.res_h.update(cx, |state, cx| {
            state.set_value(height.to_string(), window, cx);
        });
        self.refresh_select(SelectKind::Resolution, window, cx);
        cx.notify();
    }

    /// 先静默存设置，再阻塞等待 Bevy。打满才进「记录」；Esc 退出不改当前页。
    fn start(&mut self, scenario: String, window: &mut Window, cx: &mut Context<Self>) {
        if !self.persist_settings(window, cx, false) {
            return;
        }
        let id = uuid::Uuid::new_v4().to_string();
        match crate::launch::run_play(&scenario, &id) {
            Ok(_) => self.after_play(&id, window, cx),
            Err(e) => {
                window.push_notification(Notification::error(format!("启动失败: {e}")), cx);
            }
        }
        cx.notify();
    }

    fn after_play(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let recorded = StatsDb::open(&self.paths)
            .ok()
            .and_then(|db| db.get(id).ok().flatten())
            .is_some();
        if recorded {
            self.load_session(id);
            self.reload_history();
            self.page = Page::Results;
            window.push_notification(Notification::success("训练结束"), cx);
        } else {
            window.push_notification(Notification::info("已退出，本局不计入记录"), cx);
        }
    }

    fn replay(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        match crate::launch::run_replay(id) {
            Ok(_) => window.push_notification(Notification::info("回放结束"), cx),
            Err(e) => window.push_notification(Notification::error(format!("回放失败: {e}")), cx),
        }
        cx.notify();
    }

    pub(super) fn scenario_label(&self, id: &str) -> String {
        sce::list_scenarios(&self.paths)
            .into_iter()
            .find(|spec| spec.id == id)
            .map(|spec| spec.display_name)
            .unwrap_or_else(|| id.to_string())
    }

    /// 重扫磁盘并同步 Select。签名没变就跳过，避免 observe → notify → observe。
    fn refresh_select(&mut self, kind: SelectKind, window: &mut Window, cx: &mut Context<Self>) {
        match kind {
            SelectKind::History => {
                let items = history_choices(&self.paths, self.history_filter.as_ref());
                let selected = self.history_filter.clone().unwrap_or_default();
                replace_choices(
                    &self.history_select,
                    items,
                    &selected,
                    &mut self.history_sig,
                    window,
                    cx,
                );
            }
            SelectKind::Resolution => {
                let items =
                    resolution_choices(self.settings.render_width, self.settings.render_height);
                let selected =
                    res_value(self.settings.render_width, self.settings.render_height);
                replace_choices(
                    &self.resolution_select,
                    items,
                    &selected,
                    &mut self.resolution_sig,
                    window,
                    cx,
                );
            }
            SelectKind::Asset(slot) => {
                let (select, sig, current, items) = match slot {
                    AssetSlot::Theme => (
                        &self.theme_select,
                        &mut self.theme_sig,
                        self.settings.theme_path.as_str(),
                        assets::list_themes(&self.paths),
                    ),
                    AssetSlot::Crosshair => (
                        &self.crosshair_select,
                        &mut self.crosshair_sig,
                        self.settings.crosshair_path.as_str(),
                        assets::list_crosshairs(&self.paths),
                    ),
                    AssetSlot::HitSound => (
                        &self.hit_select,
                        &mut self.hit_sig,
                        self.settings.hit_sound.as_str(),
                        assets::list_sounds(&self.paths),
                    ),
                    AssetSlot::MissSound => (
                        &self.miss_select,
                        &mut self.miss_sig,
                        self.settings.miss_sound.as_str(),
                        assets::list_sounds(&self.paths),
                    ),
                };
                replace_choices(
                    select,
                    asset_choices(&items, current),
                    current,
                    sig,
                    window,
                    cx,
                );
            }
        }
    }
}

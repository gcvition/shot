//! View 用得到的无状态小组件，以及 Select / Slider 的同步工具。
//!
//! 这里 **不持有业务状态**。数字解析、下拉选项生成都可以单独测，不依赖窗口。

use std::hash::{Hash, Hasher};

use gpui_kit::component::form::Field;
use gpui_kit::component::input::{InputState, NumberInput};
use gpui_kit::component::select::{Select, SelectItem, SelectState};
use gpui_kit::component::slider::{Slider, SliderState};
use gpui_kit::component::{ActiveTheme, IndexPath, Sizable, h_flex};
use gpui_kit::*;

use crate::assets::{self, AssetItem};
use crate::fov::FovKind;
use crate::paths::AppPaths;
use crate::sce;
use crate::settings::validate_resolution;

use super::intent::NumField;
use super::model::Shell;

pub(crate) const SHELL_WIDTH: f32 = 640.0;
pub(crate) const SHELL_HEIGHT: f32 = 480.0;
/// 瞄准三项的合法区间。打字时只有落在里面才写 Model；失焦再 clamp。
pub(crate) const CM360_MIN: f64 = 1.0;
pub(crate) const CM360_MAX: f64 = 200.0;
pub(crate) const DPI_MIN: f64 = 200.0;
pub(crate) const DPI_MAX: f64 = 32_000.0;
pub(crate) const FOV_MIN: f64 = 60.0;
pub(crate) const FOV_MAX: f64 = 120.0;

/// Select 一行：内部值（路径或场景 id）+ 给人看的标题。
#[derive(Clone, PartialEq)]
pub(crate) struct Choice {
    pub value: SharedString,
    pub title: SharedString,
}

impl SelectItem for Choice {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.value
    }
}

pub(crate) type ChoiceSelect = Entity<SelectState<Vec<Choice>>>;

pub(crate) fn choice(value: impl Into<SharedString>, title: impl Into<SharedString>) -> Choice {
    Choice {
        value: value.into(),
        title: title.into(),
    }
}

pub(crate) fn section_title(cx: &App, title: &'static str) -> impl IntoElement {
    div()
        .text_sm()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

/// 顶栏右侧的版本。文案来自 [`crate::VERSION`]，不是手写的数字。
pub(crate) fn version_caption(cx: &App) -> impl IntoElement {
    div()
        .flex_shrink_0()
        .text_sm()
        .text_color(cx.theme().muted_foreground)
        .child(crate::version_label())
}

/// SliderState 默认 max=100，每个 setter 都会 clamp。必须先 `.max()` 再 `.min()`。
pub(crate) fn slider_state(min: f32, max: f32, step: f32, value: f32) -> SliderState {
    SliderState::new()
        .max(max)
        .min(min)
        .step(step)
        .default_value(value)
}

pub(crate) fn number_state(
    window: &mut Window,
    cx: &mut Context<Shell>,
    value: String,
    min: f64,
    max: f64,
    step: f64,
) -> Entity<InputState> {
    cx.new(|cx| {
        let mut state = InputState::new(window, cx).min(min).max(max).step(step);
        state.set_value(value, window, cx);
        state
    })
}

pub(crate) fn slider_with_input(
    slider: &Entity<SliderState>,
    input: &Entity<InputState>,
    input_width: Rems,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .items_center()
        .w_full()
        .child(Slider::new(slider).horizontal().flex_1().min_w_0())
        .child(NumberInput::new(input).small().w(input_width))
}

pub(crate) fn labeled_select(
    label: &'static str,
    state: &ChoiceSelect,
    placeholder: &'static str,
    empty: bool,
) -> Field {
    let select = Select::new(state)
        .small()
        .placeholder(placeholder)
        .accessibility_label(label)
        .menu_max_h(rems(12.));
    let select = if empty {
        select.empty(|_, _| "未找到文件")
    } else {
        select
    };
    Field::new().label(label).child(select)
}

pub(crate) fn sync_slider(
    slider: &Entity<SliderState>,
    value: f32,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    slider.update(cx, |state, cx| {
        if (state.value().end() - value).abs() > f32::EPSILON {
            state.set_value(value, window, cx);
        }
    });
}

pub(crate) fn sync_input_text(
    input: &Entity<InputState>,
    text: String,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    input.update(cx, |state, cx| {
        if state.value().as_ref() != text {
            state.set_value(text, window, cx);
        }
    });
}

/// 打开下拉时会 observe → 刷新列表。签名没变就跳过，避免 notify 死循环。
pub(crate) fn replace_choices(
    select: &ChoiceSelect,
    items: Vec<Choice>,
    selected: &str,
    last_sig: &mut u64,
    window: &mut Window,
    cx: &mut Context<Shell>,
) {
    let sig = choice_sig(&items);
    select.update(cx, |state, cx| {
        let current = state
            .selected_value()
            .map(|value| value.to_string())
            .unwrap_or_default();
        let items_changed = sig != *last_sig;
        let selection_changed = current != selected;
        if !items_changed && !selection_changed {
            return;
        }
        if items_changed {
            *last_sig = sig;
            state.set_items(items, window, cx);
        }
        state.set_selected_value(&SharedString::from(selected.to_string()), window, cx);
    });
}

pub(crate) fn choice_sig(items: &[Choice]) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for item in items {
        item.value.hash(&mut hasher);
        item.title.hash(&mut hasher);
    }
    hasher.finish()
}

pub(crate) fn selected_ix(items: &[Choice], value: &str) -> Option<IndexPath> {
    let current = assets::normalize(value);
    items
        .iter()
        .position(|item| assets::normalize(item.value.as_ref()) == current)
        .map(IndexPath::new)
}

pub(crate) fn history_choices(paths: &AppPaths, filter: Option<&String>) -> Vec<Choice> {
    let mut items = vec![choice("", "全部")];
    for spec in sce::list_scenarios(paths) {
        items.push(choice(spec.id.clone(), spec.display_name));
    }
    if let Some(id) = filter {
        if !items.iter().any(|item| item.value.as_ref() == id) {
            items.push(choice(id.clone(), id.clone()));
        }
    }
    items
}

pub(crate) fn resolution_choices(width: u32, height: u32) -> Vec<Choice> {
    let current = (width, height);
    let mut modes = crate::display::list_resolutions();
    if !modes.contains(&current) {
        modes.insert(0, current);
    }
    modes
        .into_iter()
        .map(|(w, h)| choice(format!("{w}x{h}"), format!("{w}×{h}")))
        .collect()
}

pub(crate) fn asset_choices(items: &[AssetItem], current: &str) -> Vec<Choice> {
    let current = assets::normalize(current);
    let label = items
        .iter()
        .find(|item| assets::normalize(&item.relative) == current)
        .map(|item| item.label.clone())
        .unwrap_or_else(|| assets::display_name(&current));
    let mut options: Vec<Choice> = items
        .iter()
        .map(|item| choice(item.relative.clone(), item.label.clone()))
        .collect();
    if !current.is_empty()
        && !options
            .iter()
            .any(|item| assets::normalize(item.value.as_ref()) == current)
    {
        options.insert(0, choice(current, label));
    }
    options
}

pub(crate) fn res_value(width: u32, height: u32) -> String {
    format!("{width}x{height}")
}

pub(crate) fn parse_res(value: &str) -> Option<(u32, u32)> {
    let (w, h) = value.split_once('x')?;
    Some((w.parse().ok()?, h.parse().ok()?))
}

/// 打字过程中：只有落在范围内才生效，避免把「8」立刻钳成 DPI 200。
/// 失焦/回车：再 clamp。
pub(crate) fn parse_aim_input(raw: &str, field: NumField, commit: bool) -> Option<f64> {
    let (min, max) = match field {
        NumField::Cm360 => (CM360_MIN, CM360_MAX),
        NumField::Dpi => (DPI_MIN, DPI_MAX),
        NumField::Fov => (FOV_MIN, FOV_MAX),
        _ => return None,
    };
    let parsed = raw.trim().parse::<f64>().ok()?;
    if !parsed.is_finite() {
        return None;
    }
    if commit {
        Some(parsed.clamp(min, max))
    } else if (min..=max).contains(&parsed) {
        Some(parsed)
    } else {
        None
    }
}

pub(crate) fn format_cm360(value: f64) -> String {
    format!("{value:.2}")
}

pub(crate) fn format_fov(value: f32) -> String {
    format!("{value:.0}")
}

pub(crate) fn fov_kind_index(kind: FovKind) -> usize {
    match kind {
        FovKind::HorizontalRes => 0,
        FovKind::Horizontal4x3 => 1,
        FovKind::Vertical => 2,
    }
}

pub(crate) fn fov_kind_from_index(index: usize) -> FovKind {
    match index {
        1 => FovKind::Horizontal4x3,
        2 => FovKind::Vertical,
        _ => FovKind::HorizontalRes,
    }
}

pub(crate) fn even_width(value: u32) -> u32 {
    let width = value & !1;
    width
}

pub(crate) fn try_even_res(width: u32, height: u32) -> Option<(u32, u32)> {
    let width = even_width(width);
    let height = even_width(height);
    validate_resolution(width, height).ok()?;
    Some((width, height))
}

#[cfg(test)]
mod tests {
    use super::{parse_aim_input, slider_state};
    use crate::shell::intent::NumField;

    #[test]
    fn slider_state_should_allow_min_above_default_max() {
        let state = slider_state(200.0, 32_000.0, 50.0, 800.0);
        assert_eq!(state.min_value(), 200.0);
        assert_eq!(state.max_value(), 32_000.0);
        assert_eq!(state.value().end(), 800.0);
    }

    #[test]
    fn parse_aim_input_should_accept_in_range_while_typing() {
        assert_eq!(
            parse_aim_input("19.05", NumField::Cm360, false),
            Some(19.05)
        );
        assert_eq!(parse_aim_input("800", NumField::Dpi, false), Some(800.0));
        assert_eq!(parse_aim_input("103", NumField::Fov, false), Some(103.0));
    }

    #[test]
    fn parse_aim_input_should_ignore_partial_out_of_range_while_typing() {
        assert_eq!(parse_aim_input("8", NumField::Dpi, false), None);
        assert_eq!(parse_aim_input("19.", NumField::Fov, false), None);
        assert_eq!(parse_aim_input("", NumField::Cm360, false), None);
    }

    #[test]
    fn parse_aim_input_should_clamp_on_commit() {
        assert_eq!(parse_aim_input("8", NumField::Dpi, true), Some(200.0));
        assert_eq!(parse_aim_input("400", NumField::Fov, true), Some(120.0));
    }
}

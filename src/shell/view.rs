//! View：只负责「现在屏幕长什么样」，点击全部变成 Intent。
//!
//! 禁止在这里写 `self.settings.xxx =`。新按钮：
//! `on_click(cx.listener(|this, _, window, cx| this.dispatch(Intent::..., window, cx)))`

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::chart::LineChart;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::form::{Field, Form};
use gpui_kit::component::input::NumberInput;
use gpui_kit::component::radio::{Radio, RadioGroup};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::select::Select;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::tab::{Tab, TabBar};
use gpui_kit::component::{ActiveTheme, Root, Sizable, h_flex, v_flex};
use gpui_kit::*;

use crate::sce::{self, MENU_LIMIT, ScenarioSpec};
use crate::settings::DisplayMode;

use super::intent::{Intent, Page};
use super::model::Shell;
use super::widgets::{
    SHELL_HEIGHT, SHELL_WIDTH, fov_kind_from_index, fov_kind_index, labeled_select, section_title,
    slider_with_input, version_caption,
};

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.resize(size(px(SHELL_WIDTH), px(SHELL_HEIGHT)));
        v_flex()
            .relative()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .overflow_hidden()
            .p_3()
            .gap_3()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .child(
                        TabBar::new("nav")
                            .segmented()
                            .small()
                            .flex_1()
                            .min_w_0()
                            .selected_index(self.page.tab_index())
                            .on_click(cx.listener(|this, ix, window, cx| {
                                this.dispatch(Intent::OpenPage(Page::from_tab(*ix)), window, cx);
                            }))
                            .child(Tab::new().label("训练"))
                            .child(Tab::new().label("设置"))
                            .child(Tab::new().label("记录")),
                    )
                    .child(version_caption(cx)),
            )
            .child(
                div()
                    .id("page")
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_y_scrollbar()
                    .child(match self.page {
                        Page::Train => self.render_train(cx).into_any_element(),
                        Page::Settings => self.render_settings(cx).into_any_element(),
                        Page::Results => self.render_results(cx).into_any_element(),
                    }),
            )
            // Root 自己不画遮罩；对话框/通知必须由内容 View 挂上，否则点了没反应。
            .children(Root::render_dialog_layer(window, cx))
            .children(Root::render_sheet_layer(window, cx))
            .children(Root::render_notification_layer(window, cx))
    }
}

impl Shell {
    /// 「训练」页：场景按钮 + 带筛选的历史列表。
    fn render_train(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let scenarios = sce::list_scenarios(&self.paths);
        let mut col = v_flex().gap_3().child(section_title(cx, "场景")).child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("选择场景开始 60 秒训练"),
        );
        if scenarios.is_empty() {
            col = col.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("把 .sce 文件放到 res/scenarios"),
            );
        } else {
            for spec in scenarios.iter().take(MENU_LIMIT) {
                let id = spec.id.clone();
                col = col.child(
                    Button::new(format!("sce-{id}"))
                        .outline()
                        .small()
                        .w_full()
                        .label(spec.id.clone())
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.dispatch(Intent::StartScenario(id.clone()), window, cx);
                        })),
                );
            }
        }
        col = col.child(section_title(cx, "最近记录")).child(
            Field::new().label("场景").child(
                Select::new(&self.history_select)
                    .small()
                    .placeholder("全部")
                    .accessibility_label("筛选场景")
                    .menu_max_h(rems(12.)),
            ),
        );
        if self.history.is_empty() {
            col = col.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(self.history_empty_text(&scenarios)),
            );
        } else {
            for row in &self.history {
                let replay_id = row.id.clone();
                let delete_id = row.id.clone();
                let delete_name = self.scenario_label(&row.scenario);
                col = col.child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().flex_1().min_w_0().text_sm().child(format!(
                            "{}  {}分  {}/{}",
                            self.scenario_label(&row.scenario),
                            row.score,
                            row.hits,
                            row.hits + row.misses
                        )))
                        .child(
                            Button::new(format!("replay-{}", row.id))
                                .outline()
                                .small()
                                .label("回放")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.dispatch(
                                        Intent::ReplaySession(replay_id.clone()),
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new(format!("del-{}", row.id))
                                .danger()
                                .small()
                                .label("删除")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.dispatch(
                                        Intent::AskDeleteSession {
                                            id: delete_id.clone(),
                                            name: delete_name.clone(),
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                );
            }
        }
        col
    }

    fn history_empty_text(&self, scenarios: &[ScenarioSpec]) -> String {
        match &self.history_filter {
            None => "完成一局后会出现在这里".into(),
            Some(id) => {
                let name = scenarios
                    .iter()
                    .find(|spec| spec.id == *id)
                    .map(|spec| spec.display_name.as_str())
                    .unwrap_or(id.as_str());
                format!("没有「{name}」记录")
            }
        }
    }

    /// 瞄准 / 画面 / 外观。控件状态在 Model 里，这里只排版。
    fn render_settings(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        v_flex()
            .gap_4()
            .child(section_title(cx, "瞄准"))
            .child(
                Form::vertical()
                    .small()
                    .child(
                        Field::new().label("厘米/360°").child(slider_with_input(
                            &self.cm360,
                            &self.cm360_input,
                            rems(5.5),
                        )),
                    )
                    .child(
                        Field::new().label("DPI").child(slider_with_input(
                            &self.dpi,
                            &self.dpi_input,
                            rems(5.5),
                        )),
                    )
                    .child(
                        Field::new().label("FOV").child(slider_with_input(
                            &self.fov,
                            &self.fov_input,
                            rems(5.5),
                        )),
                    )
                    .child(
                        Field::new().label("FOV 类型").child(
                            RadioGroup::horizontal("fov-kind")
                                .selected_index(Some(fov_kind_index(s.fov_kind)))
                                .on_click(cx.listener(|this, ix, window, cx| {
                                    this.dispatch(
                                        Intent::SetFovKind(fov_kind_from_index(*ix)),
                                        window,
                                        cx,
                                    );
                                }))
                                .child(Radio::new("fov-res").label("分辨率水平").small())
                                .child(Radio::new("fov-43").label("4:3 水平").small())
                                .child(Radio::new("fov-vert").label("垂直").small()),
                        ),
                    )
                    .child(
                        Field::new()
                            .label("开始倒计时")
                            .description("0 表示立即开始")
                            .child(NumberInput::new(&self.countdown).small().w(rems(5.))),
                    ),
            )
            .child(section_title(cx, "画面"))
            .child(
                Form::vertical()
                    .small()
                    .child(labeled_select(
                        "渲染分辨率",
                        &self.resolution_select,
                        "选择分辨率",
                        false,
                    ))
                    .child(
                        Field::new()
                            .label("自定义尺寸")
                            .description("列表中没有的分辨率")
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(NumberInput::new(&self.res_w).small().w(rems(5.)))
                                    .child(NumberInput::new(&self.res_h).small().w(rems(5.))),
                            ),
                    )
                    .child(
                        Field::new()
                            .label("显示")
                            .description("下一局训练生效")
                            .child(
                                RadioGroup::horizontal("display")
                                    .selected_index(Some(match s.display_mode {
                                        DisplayMode::BorderlessStretch => 0,
                                        DisplayMode::Windowed => 1,
                                    }))
                                    .on_click(cx.listener(|this, ix, window, cx| {
                                        let mode = if *ix == 1 {
                                            DisplayMode::Windowed
                                        } else {
                                            DisplayMode::BorderlessStretch
                                        };
                                        this.dispatch(Intent::SetDisplayMode(mode), window, cx);
                                    }))
                                    .child(Radio::new("disp-borderless").label("无边框").small())
                                    .child(Radio::new("disp-window").label("窗口").small()),
                            ),
                    ),
            )
            .child(section_title(cx, "外观与声音"))
            .child(
                Form::vertical()
                    .small()
                    .child(labeled_select(
                        "主题",
                        &self.theme_select,
                        "选择主题",
                        true,
                    ))
                    .child(labeled_select(
                        "准星",
                        &self.crosshair_select,
                        "选择准星",
                        true,
                    ))
                    .child(
                        Field::new()
                            .label(format!("音量  {:.0}%", s.master_volume * 100.0))
                            .child(Slider::new(&self.volume).horizontal().w_full()),
                    )
                    .child(labeled_select(
                        "命中",
                        &self.hit_select,
                        "选择命中音",
                        true,
                    ))
                    .child(labeled_select(
                        "未命中",
                        &self.miss_select,
                        "选择未命中音",
                        true,
                    )),
            )
            .child(
                Button::new("save")
                    .primary()
                    .small()
                    .label("保存")
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.dispatch(Intent::SaveSettings { announce: true }, window, cx);
                    })),
            )
    }

    /// 上一局的数字 + 回放/再练 + 得分折线。
    fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = v_flex().gap_3();
        if let Some(last) = &self.last {
            let replay_id = last.id.clone();
            let again_id = last.scenario.clone();
            let delete_id = last.id.clone();
            let delete_name = self.scenario_label(&last.scenario);
            col = col
                .child(section_title(cx, "本局"))
                .child(
                    DescriptionList::new()
                        .small()
                        .columns(2)
                        .item("场景", self.scenario_label(&last.scenario), 2)
                        .item("得分", last.score.to_string(), 1)
                        .item("命中", last.hits.to_string(), 1)
                        .item("未命中", last.misses.to_string(), 1)
                        .item("准确率", format!("{:.1}%", last.accuracy() * 100.0), 1)
                        .item("命中/秒", format!("{:.2}", last.hits_per_sec()), 1),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(Button::new("rep").primary().small().label("回放").on_click(
                            cx.listener(move |this, _, window, cx| {
                                this.dispatch(Intent::ReplaySession(replay_id.clone()), window, cx);
                            }),
                        ))
                        .child(
                            Button::new("again")
                                .outline()
                                .small()
                                .label("再练一局")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.dispatch(
                                        Intent::StartScenario(again_id.clone()),
                                        window,
                                        cx,
                                    );
                                })),
                        )
                        .child(
                            Button::new("del-last")
                                .danger()
                                .small()
                                .label("删除")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.dispatch(
                                        Intent::AskDeleteSession {
                                            id: delete_id.clone(),
                                            name: delete_name.clone(),
                                        },
                                        window,
                                        cx,
                                    );
                                })),
                        ),
                );
        } else {
            col = col
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("还没有训练记录"),
                )
                .child(
                    Button::new("go-train")
                        .outline()
                        .small()
                        .label("去训练")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.dispatch(Intent::OpenPage(Page::Train), window, cx);
                        })),
                );
        }
        if !self.trend.is_empty() {
            col = col.child(section_title(cx, "得分趋势")).child(
                div().h(rems(6.)).w_full().child(
                    LineChart::new(self.trend.clone())
                        .x(|r| r.id.chars().take(4).collect::<String>())
                        .y(|r| r.score as f64)
                        .dot()
                        .name("得分"),
                ),
            );
        }
        col
    }
}

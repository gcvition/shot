use gpui_kit::component::button::*;
use gpui_kit::component::chart::LineChart;
use gpui_kit::component::{h_flex, v_flex, Root};
use gpui_kit::*;

use crate::paths::AppPaths;
use crate::settings::{DisplayMode, ScenarioKind, Settings};
use crate::stats::{SessionRecord, StatsDb};

pub fn run() -> anyhow::Result<()> {
    let paths = AppPaths::discover()?;
    paths.ensure_dirs()?;
    let settings = Settings::load_or_default(&paths)?;
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        let settings = settings.clone();
        let paths = paths.clone();
        cx.spawn(async move |cx| {
            cx.open_window(WindowOptions::default(), |window, cx| {
                let view = cx.new(|cx| Shell::new(paths, settings, window, cx));
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        })
        .detach();
    });
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Menu,
    Settings,
    Results,
}

struct Shell {
    paths: AppPaths,
    settings: Settings,
    page: Page,
    status: String,
    last: Option<SessionRecord>,
    trend: Vec<SessionRecord>,
    history: Vec<SessionRecord>,
    backend: String,
}

impl Shell {
    fn new(paths: AppPaths, settings: Settings, _window: &mut Window, _cx: &mut Context<Self>) -> Self {
        let backend = crate::video::probe_backend_name().to_string();
        let mut shell = Self {
            paths,
            settings,
            page: Page::Menu,
            status: String::new(),
            last: None,
            trend: Vec::new(),
            history: Vec::new(),
            backend,
        };
        shell.reload_history();
        if let Some(id) = crate::launch::read_last_session(&shell.paths).ok().flatten() {
            shell.load_session(&id);
            if shell.last.is_some() {
                shell.page = Page::Results;
            }
        }
        shell
    }

    fn reload_history(&mut self) {
        if let Ok(db) = StatsDb::open(&self.paths) {
            self.history = db
                .recent(ScenarioKind::SixTargets, 20)
                .unwrap_or_default();
            let mut grid = db.recent(ScenarioKind::GridShot, 20).unwrap_or_default();
            self.history.append(&mut grid);
            self.history.sort_by_key(|r| std::cmp::Reverse(r.started_at));
            self.history.truncate(20);
        }
    }

    fn load_session(&mut self, id: &str) {
        if let Ok(db) = StatsDb::open(&self.paths) {
            self.last = db.get(id).ok().flatten();
            if let Some(last) = &self.last {
                self.trend = db.recent(last.scenario, 30).unwrap_or_default();
            }
        }
    }

    fn save_settings(&mut self) {
        match self.settings.save(&self.paths) {
            Ok(()) => self.status = "设置已保存".into(),
            Err(e) => self.status = format!("保存失败: {e}"),
        }
    }

    fn start(&mut self, scenario: ScenarioKind, cx: &mut Context<Self>) {
        self.save_settings();
        let id = uuid::Uuid::new_v4().to_string();
        self.status = format!("正在启动{}…", scenario.title());
        cx.notify();
        match crate::launch::run_play(scenario, &id) {
            Ok(_) => {
                self.load_session(&id);
                self.reload_history();
                self.page = Page::Results;
                self.status = format!("视频后端: {}", self.backend);
            }
            Err(e) => self.status = format!("启动失败: {e}"),
        }
        cx.notify();
    }

    fn replay(&mut self, id: &str, cx: &mut Context<Self>) {
        self.status = "回放中…".into();
        cx.notify();
        match crate::launch::run_replay(id) {
            Ok(_) => self.status = "回放结束".into(),
            Err(e) => self.status = format!("回放失败: {e}"),
        }
        cx.notify();
    }

    fn encode(&mut self, id: &str, cx: &mut Context<Self>) {
        let session = match crate::session::SessionFile::load(&self.paths, id) {
            Ok(session) => session,
            Err(e) => {
                self.status = format!("读取轨迹失败: {e}");
                cx.notify();
                return;
            }
        };
        match crate::video::encode_session(&self.paths, &session) {
            Ok(path) => {
                if let Ok(db) = StatsDb::open(&self.paths) {
                    let _ = db.set_video_path(id, &path.display().to_string());
                }
                self.status = format!("已导出 {}", path.display());
                self.load_session(id);
            }
            Err(e) => self.status = format!("导出失败: {e}"),
        }
        cx.notify();
    }
}

impl Render for Shell {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            .p_5()
            .gap_4()
            .child(
                div()
                    .text_xl()
                    .child("Shot 瞄准训练"),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(nav_btn("menu", "菜单", self.page == Page::Menu, cx.listener(|this, _, _, cx| {
                        this.page = Page::Menu;
                        cx.notify();
                    })))
                    .child(nav_btn("set", "设置", self.page == Page::Settings, cx.listener(|this, _, _, cx| {
                        this.page = Page::Settings;
                        cx.notify();
                    })))
                    .child(nav_btn("res", "结算", self.page == Page::Results, cx.listener(|this, _, _, cx| {
                        this.page = Page::Results;
                        this.reload_history();
                        cx.notify();
                    }))),
            )
            .child(div().child(self.status.clone()))
            .child(match self.page {
                Page::Menu => self.render_menu(cx).into_any_element(),
                Page::Settings => self.render_settings(cx).into_any_element(),
                Page::Results => self.render_results(cx).into_any_element(),
            })
    }
}

impl Shell {
    fn render_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .gap_3()
            .child(div().child("选择场景（每局 60 秒）"))
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Button::new("six")
                            .primary()
                            .label("六目标")
                            .on_click(cx.listener(|this, _, _, cx| this.start(ScenarioKind::SixTargets, cx))),
                    )
                    .child(
                        Button::new("grid")
                            .primary()
                            .label("网格射击")
                            .on_click(cx.listener(|this, _, _, cx| this.start(ScenarioKind::GridShot, cx))),
                    ),
            )
            .child(div().child("最近记录"))
            .children(self.history.iter().take(8).map(|row| {
                let id = row.id.clone();
                h_flex()
                    .gap_2()
                    .child(div().child(format!(
                        "{}  {}分  {}/{}",
                        row.scenario.title(),
                        row.score,
                        row.hits,
                        row.hits + row.misses
                    )))
                    .child(
                        Button::new(format!("replay-{}", row.id))
                            .label("回放")
                            .on_click(cx.listener(move |this, _, _, cx| this.replay(&id, cx))),
                    )
            }))
    }

    fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let s = &self.settings;
        v_flex()
            .gap_2()
            .child(stepper(
                "cm360",
                format!("厘米/360°  {:.2}", s.cm_per_360),
                cx.listener(|this, _, _, cx| {
                    this.settings.cm_per_360 = (this.settings.cm_per_360 - 0.05).max(1.0);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.settings.cm_per_360 = (this.settings.cm_per_360 + 0.05).min(200.0);
                    cx.notify();
                }),
            ))
            .child(stepper(
                "dpi",
                format!("DPI  {}", s.dpi),
                cx.listener(|this, _, _, cx| {
                    this.settings.dpi = this.settings.dpi.saturating_sub(50).max(200);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.settings.dpi = (this.settings.dpi + 50).min(32_000);
                    cx.notify();
                }),
            ))
            .child(stepper(
                "fov",
                format!("FOV  {:.0}° {:?}", s.fov_deg, s.fov_kind),
                cx.listener(|this, _, _, cx| {
                    this.settings.fov_deg = (this.settings.fov_deg - 1.0).max(60.0);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.settings.fov_deg = (this.settings.fov_deg + 1.0).min(120.0);
                    cx.notify();
                }),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("fovk")
                            .label("切换 FOV 类型")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.settings.fov_kind = match this.settings.fov_kind {
                                    crate::fov::FovKind::HorizontalRes => {
                                        crate::fov::FovKind::Horizontal4x3
                                    }
                                    crate::fov::FovKind::Horizontal4x3 => crate::fov::FovKind::Vertical,
                                    crate::fov::FovKind::Vertical => crate::fov::FovKind::HorizontalRes,
                                };
                                cx.notify();
                            })),
                    ),
            )
            .child(div().child(format!(
                "渲染分辨率  {}×{}",
                s.render_width, s.render_height
            )))
            .child(
                h_flex().gap_2().children(Settings::RESOLUTION_PRESETS.iter().map(|(w, h, label)| {
                    let w = *w;
                    let h = *h;
                    Button::new(*label)
                        .label(*label)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.settings.render_width = w;
                            this.settings.render_height = h;
                            cx.notify();
                        }))
                })),
            )
            .child(
                Button::new("disp")
                    .label(match s.display_mode {
                        DisplayMode::BorderlessStretch => "显示: 无边框拉伸全屏",
                        DisplayMode::Windowed => "显示: 窗口",
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.display_mode = match this.settings.display_mode {
                            DisplayMode::BorderlessStretch => DisplayMode::Windowed,
                            DisplayMode::Windowed => DisplayMode::BorderlessStretch,
                        };
                        cx.notify();
                    })),
            )
            .child(stepper(
                "vol",
                format!("音量  {:.0}%", s.master_volume * 100.0),
                cx.listener(|this, _, _, cx| {
                    this.settings.master_volume = (this.settings.master_volume - 0.05).max(0.0);
                    cx.notify();
                }),
                cx.listener(|this, _, _, cx| {
                    this.settings.master_volume = (this.settings.master_volume + 0.05).min(1.0);
                    cx.notify();
                }),
            ))
            .child(
                Button::new("video")
                    .label(if s.video_enabled {
                        "局后导出视频: 开"
                    } else {
                        "局后导出视频: 关"
                    })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.settings.video_enabled = !this.settings.video_enabled;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("save")
                    .primary()
                    .label("保存设置")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.save_settings();
                        cx.notify();
                    })),
            )
            .child(div().child(format!(
                "主题 {}  准星 {}  命中 {}  未命中 {}",
                s.theme_path, s.crosshair_path, s.hit_sound, s.miss_sound
            )))
    }

    fn render_results(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut col = v_flex().gap_3();
        if let Some(last) = &self.last {
            let id = last.id.clone();
            let id2 = last.id.clone();
            col = col
                .child(div().child(format!(
                    "{}  得分 {}  命中 {}  未命中 {}  准确率 {:.1}%  {:.2} hits/s",
                    last.scenario.title(),
                    last.score,
                    last.hits,
                    last.misses,
                    last.accuracy() * 100.0,
                    last.hits_per_sec()
                )))
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("rep")
                                .primary()
                                .label("回放本局")
                                .on_click(cx.listener(move |this, _, _, cx| this.replay(&id, cx))),
                        )
                        .child(
                            Button::new("again")
                                .label("再来一局")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if let Some(last) = this.last.clone() {
                                        this.start(last.scenario, cx);
                                    }
                                })),
                        )
                        .child(
                            Button::new("enc")
                                .label("导出视频")
                                .on_click(cx.listener(move |this, _, _, cx| this.encode(&id2, cx))),
                        )
                        .child(
                            Button::new("folder")
                                .label("打开视频目录")
                                .on_click(cx.listener(|this, _, _, _cx| {
                                    let _ = crate::launch::open_in_explorer(&this.paths.video);
                                })),
                        ),
                );
        } else {
            col = col.child(div().child("还没有训练记录"));
        }
        if !self.trend.is_empty() {
            col = col
                .child(div().child("得分趋势"))
                .child(
                    div()
                        .h(px(220.))
                        .w_full()
                        .child(
                            LineChart::new(self.trend.clone())
                                .x(|r| {
                                    let n = r.id.chars().take(4).collect::<String>();
                                    n
                                })
                                .y(|r| r.score as f64)
                                .dot()
                                .name("得分"),
                        ),
                );
        }
        col
    }
}

fn nav_btn(
    id: &'static str,
    label: &'static str,
    active: bool,
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let btn = Button::new(id).label(label).on_click(handler);
    if active {
        btn.primary()
    } else {
        btn
    }
}

fn stepper(
    id: &'static str,
    label: String,
    down: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    up: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    h_flex()
        .gap_2()
        .child(Button::new(format!("{id}-d")).label("-").on_click(down))
        .child(div().min_w(px(220.)).child(label))
        .child(Button::new(format!("{id}-u")).label("+").on_click(up))
}

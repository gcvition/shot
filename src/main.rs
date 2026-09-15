//! 进程入口：同一个 `shot.exe` 根据命令行切换身份。
//!
//! | 命令行 | 做什么 |
//! |--------|--------|
//! | （无参数，或随便什么不是 play/replay） | GPUI 菜单 [`shot::shell::run`] |
//! | `play [场景id] [对局id]` | Bevy 训练。场景缺省时用 `res/scenarios` 里第一个 `.sce` |
//! | `replay <对局id>` | Bevy 回放。从 `cache/sessions/<id>.shot` 读轨迹 |
//!
//! 菜单里点「开始训练」并不会在当前进程里嵌 Bevy，而是 [`shot::launch`] 再 spawn
//! 一份自己。这样 GPUI 和 Bevy 各管各的窗口，互不抢消息循环。

fn main() {
    if let Err(err) = real_main() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}

fn real_main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("play") => {
            let paths = shot::paths::AppPaths::discover()?;
            let requested = args.get(2).map(String::as_str).unwrap_or("");
            let spec = if requested.is_empty() {
                shot::sce::list_scenarios(&paths)
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("no scenario files in res/scenarios"))?
            } else {
                shot::sce::load_scenario(&paths, requested)?
            };
            let id = args
                .get(3)
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            shot::viewport::run(shot::viewport::ViewportMode::Play, spec, id)?;
        }
        Some("replay") => {
            let id = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("replay requires session id"))?;
            let paths = shot::paths::AppPaths::discover()?;
            let scenario_id = shot::session::SessionFile::load(&paths, &id)
                .ok()
                .map(|file| file.header.scenario)
                .unwrap_or_else(|| "Sixshot Ultimate".into());
            let spec = shot::sce::load_scenario(&paths, &scenario_id)
                .unwrap_or_else(|_| shot::sce::ScenarioSpec::fallback(&scenario_id));
            shot::viewport::run(shot::viewport::ViewportMode::Replay, spec, id)?;
        }
        _ => shot::shell::run()?,
    }
    Ok(())
}

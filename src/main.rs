//! 进程入口：同一个 `shot.exe` 根据命令行切换身份。
//!
//! | 命令行 | 做什么 |
//! |--------|--------|
//! | （无参数，或随便什么不是 play/replay） | GPUI 菜单 [`shot::shell::run`] |
//! | `play [场景id] [对局id]` | Bevy 训练。场景缺省时用 `res/scenarios` 里第一个 `.sce` |
//! | `replay <对局id>` | Bevy 回放。从 `cache/sessions/<id>.shot` 读轨迹 |
//! | `export-trace <对局id>` | 把 `.shot` 导成 JSON 打到 stdout（调试用） |
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
            if args.iter().any(|a| a == "--aimbot") {
                shot::viewport::run_aimbot(spec, id)?;
            } else {
                shot::viewport::run(shot::viewport::ViewportMode::Play, spec, id)?;
            }
        }
        Some("replay") => {
            let id = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("replay requires session id"))?;
            let paths = shot::paths::AppPaths::discover()?;
            let file = shot::session::SessionFile::load(&paths, &id)
                .map_err(|e| anyhow::anyhow!("读取回放 {id} 失败: {e}"))?;
            let scenario_id = file.header.scenario.clone();
            let spec = shot::sce::load_scenario(&paths, &scenario_id)
                .unwrap_or_else(|_| shot::sce::ScenarioSpec::fallback(&scenario_id));
            shot::viewport::run(shot::viewport::ViewportMode::Replay, spec, id)?;
        }
        Some("export-trace") => {
            // `.shot` 是二进制，调试时把轨迹导成 JSON 打到 stdout。
            let id = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("export-trace requires session id"))?;
            let paths = shot::paths::AppPaths::discover()?;
            let file = shot::session::SessionFile::load(&paths, &id)?;
            println!("{}", serde_json::to_string_pretty(&file)?);
        }
        Some("playlist") => {
            // 课程连播：`shot playlist <id> [session-id]`。
            let name = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("playlist requires a course id"))?;
            let playlist = shot::playlist::find(&name)
                .ok_or_else(|| anyhow::anyhow!("unknown playlist: {name}"))?;
            let paths = shot::paths::AppPaths::discover()?;
            let first = playlist.scenarios[0];
            let spec = shot::sce::load_scenario(&paths, first)
                .map_err(|e| anyhow::anyhow!("playlist scenario {first}: {e}"))?;
            let id = args
                .get(3)
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let plan = shot::playlist::PlanRun::new(playlist);
            shot::viewport::run_playlist(spec, id, plan)?;
        }
        Some("reanalyze") => {
            // 重算全部历史局的指标（schema 升级后补数据用）。
            let paths = shot::paths::AppPaths::discover()?;
            let db = shot::stats::StatsDb::open(&paths)?;
            let mut n = 0usize;
            for rec in db.recent_newest(None, 10_000)? {
                if let Ok(file) = shot::session::SessionFile::load(&paths, &rec.id) {
                    let m = shot::analysis::analyze(&file);
                    let mut rows = m.rows();
                    rows.push(("score", rec.score_f));
                    db.insert_metrics(&rec.id, &rows)?;
                    n += 1;
                }
            }
            println!("reanalyzed {n} sessions");
        }
        _ => shot::shell::run()?,
    }
    Ok(())
}

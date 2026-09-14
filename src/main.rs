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
            let scenario = args
                .get(2)
                .and_then(|s| shot::ScenarioKind::parse(s))
                .unwrap_or(shot::ScenarioKind::SixTargets);
            let id = args
                .get(3)
                .cloned()
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            shot::viewport::run(shot::viewport::ViewportMode::Play, scenario, id)?;
        }
        Some("replay") => {
            let id = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("replay requires session id"))?;
            let scenario = shot::paths::AppPaths::discover()
                .ok()
                .and_then(|p| shot::session::SessionFile::load(&p, &id).ok())
                .map(|s| s.header.scenario)
                .unwrap_or(shot::ScenarioKind::SixTargets);
            shot::viewport::run(shot::viewport::ViewportMode::Replay, scenario, id)?;
        }
        Some("encode") => {
            let id = args
                .get(2)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("encode requires session id"))?;
            let paths = shot::paths::AppPaths::discover()?;
            let session = shot::session::SessionFile::load(&paths, &id)?;
            let path = shot::video::encode_session(&paths, &session)?;
            if let Ok(db) = shot::stats::StatsDb::open(&paths) {
                let _ = db.set_video_path(&id, &path.display().to_string());
            }
            println!("wrote {}", path.display());
        }
        _ => shot::shell::run()?,
    }
    Ok(())
}

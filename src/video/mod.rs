mod ffmpeg;
mod mux;
mod openh264_sink;
mod preview;

use std::path::{Path, PathBuf};

use crate::error::Result;
use crate::paths::AppPaths;
use crate::session::SessionFile;

pub use ffmpeg::FfmpegSink;
pub use preview::raster_frame;

pub trait VideoSink {
    fn push_rgba(&mut self, frame: &[u8]) -> Result<()>;
    fn finish(self: Box<Self>) -> Result<PathBuf>;
}

pub fn detect_ffmpeg() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("SHOT_FFMPEG") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let names = ["ffmpeg", "ffmpeg.exe"];
    if let Ok(path_var) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_var) {
            for name in names {
                let candidate = dir.join(name);
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }
    [
        r"C:\ffmpeg\bin\ffmpeg.exe",
        r"C:\ProgramData\chocolatey\bin\ffmpeg.exe",
    ]
    .iter()
    .map(PathBuf::from)
    .find(|p| p.is_file())
}

pub fn open_sink(output: &Path, width: u32, height: u32, fps: u32) -> Result<Box<dyn VideoSink>> {
    if let Some(ffmpeg) = detect_ffmpeg() {
        return Ok(Box::new(FfmpegSink::spawn(
            &ffmpeg, output, width, height, fps,
        )?));
    }
    Ok(Box::new(openh264_sink::OpenH264Sink::new(
        output, width, height, fps,
    )?))
}

pub fn encode_session(paths: &AppPaths, session: &SessionFile) -> Result<PathBuf> {
    let w = session.header.settings.render_width.max(2) & !1;
    let h = session.header.settings.render_height.max(2) & !1;
    let fps = 60u32;
    let duration = (session.header.duration_ms as f32 / 1000.0).max(0.05);
    let frames = ((duration * fps as f32).ceil() as u32).max(1);
    let output = paths.session_video(&session.header.session_id);
    paths.ensure_dirs()?;
    let mut sink = open_sink(&output, w, h, fps)?;
    let mut buffer = vec![0u8; (w * h * 4) as usize];
    for i in 0..frames {
        let t_us = (i as u64) * 1_000_000 / u64::from(fps);
        preview::raster_frame(session, w, h, t_us, &mut buffer);
        sink.push_rgba(&buffer)?;
    }
    sink.finish()
}

pub fn probe_backend_name() -> &'static str {
    if detect_ffmpeg().is_some() {
        "ffmpeg"
    } else {
        "openh264"
    }
}

#[cfg(test)]
mod tests {
    use super::encode_session;
    use crate::paths::AppPaths;
    use crate::session::{SessionFile, SettingsSnapshot};
    use crate::settings::{ScenarioKind, Settings};

    #[test]
    fn encode_tiny_session_should_write_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        paths.ensure_dirs().unwrap();
        let mut settings = Settings::default();
        settings.render_width = 64;
        settings.render_height = 48;
        let snap = SettingsSnapshot::from_settings(&settings, 64, 48, 60);
        let mut session = SessionFile::new("tiny".into(), ScenarioKind::SixTargets, 1, snap);
        session.header.duration_ms = 80;
        session.mouse.push(crate::session::MouseSample {
            t_us: 0,
            dx: 0.0,
            dy: 0.0,
            yaw_deg: 0.0,
            pitch_deg: 0.0,
        });
        let path = encode_session(&paths, &session).unwrap();
        assert!(path.exists());
        assert!(std::fs::metadata(&path).unwrap().len() > 32);
    }
}

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use crate::error::{Result, ShotError};
use crate::video::VideoSink;

pub struct FfmpegSink {
    child: Child,
    stdin: ChildStdin,
    output: PathBuf,
}

impl FfmpegSink {
    pub fn spawn(ffmpeg: &Path, output: &Path, width: u32, height: u32, fps: u32) -> Result<Self> {
        if let Some(parent) = output.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut child = Command::new(ffmpeg)
            .args([
                "-y",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
                "-s",
                &format!("{width}x{height}"),
                "-r",
                &fps.to_string(),
                "-i",
                "-",
                "-an",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "20",
                "-pix_fmt",
                "yuv420p",
                "-movflags",
                "+faststart",
                output.to_str().ok_or_else(|| ShotError::Other("video path is not utf-8".into()))?,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ShotError::Other("ffmpeg stdin missing".into()))?;
        Ok(Self {
            child,
            stdin,
            output: output.to_path_buf(),
        })
    }
}

impl VideoSink for FfmpegSink {
    fn push_rgba(&mut self, frame: &[u8]) -> Result<()> {
        self.stdin.write_all(frame).map_err(ShotError::from)
    }

    fn finish(mut self: Box<Self>) -> Result<PathBuf> {
        drop(self.stdin);
        let status = self.child.wait()?;
        if !status.success() {
            return Err(ShotError::Other(format!("ffmpeg exited with {status}")));
        }
        Ok(self.output)
    }
}

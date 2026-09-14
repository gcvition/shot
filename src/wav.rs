use std::path::Path;

use crate::error::Result;
use crate::paths::AppPaths;

pub fn write_sine_wav(path: &Path, freq_hz: f32, duration_secs: f32, volume: f32) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let sample_rate = 22_050u32;
    let n = (duration_secs * sample_rate as f32).max(1.0) as u32;
    let mut pcm = Vec::with_capacity(n as usize * 2);
    for i in 0..n {
        let t = i as f32 / sample_rate as f32;
        let env = (1.0 - t / duration_secs).clamp(0.0, 1.0).powi(1);
        let sample = (t * freq_hz * std::f32::consts::TAU).sin() * volume * env;
        let v = (sample * i16::MAX as f32) as i16;
        pcm.extend_from_slice(&v.to_le_bytes());
    }
    let data_len = pcm.len() as u32;
    let mut bytes = Vec::with_capacity(44 + pcm.len());
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    bytes.extend_from_slice(&2u16.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    bytes.extend(pcm);
    std::fs::write(path, bytes)?;
    Ok(())
}

pub fn ensure_fallback_sounds(paths: &AppPaths) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    paths.ensure_dirs()?;
    let hit = if paths.res.join("命中.wav").exists() {
        paths.res.join("命中.wav")
    } else {
        let path = paths.cache.join("hit_fallback.wav");
        if !path.exists() {
            write_sine_wav(&path, 880.0, 0.07, 0.4)?;
        }
        path
    };
    let miss = if paths.res.join("未命中.wav").exists() {
        paths.res.join("未命中.wav")
    } else {
        let path = paths.cache.join("miss_fallback.wav");
        if !path.exists() {
            write_sine_wav(&path, 180.0, 0.08, 0.25)?;
        }
        path
    };
    Ok((hit, miss))
}

#[cfg(test)]
mod tests {
    use super::write_sine_wav;

    #[test]
    fn write_sine_wav_should_create_riff_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beep.wav");
        write_sine_wav(&path, 440.0, 0.05, 0.2).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert!(bytes.len() > 44);
    }
}

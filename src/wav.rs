//! 把 `res/sounds` 里的 ogg/wav 解成 PCM，给 [`crate::sfx`] 播。
//!
//! ogg 用 lewton 解码。文件缺失时用短正弦波顶上，保证训练不会因为音效挂掉。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::error::{Result, ShotError};
use crate::paths::AppPaths;

/// 交错的 f32 PCM。`samples` 用 Arc，命中/空枪可以在线程间共享同一份。
#[derive(Clone)]
pub struct PcmClip {
    pub samples: Arc<[f32]>,
    pub sample_rate: u32,
    pub channels: u16,
}

impl PcmClip {
    fn from_i16(samples: &[i16], sample_rate: u32, channels: u16) -> Self {
        let samples: Arc<[f32]> = samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32)
            .collect::<Vec<_>>()
            .into();
        Self {
            samples,
            sample_rate: sample_rate.max(1),
            channels: channels.max(1),
        }
    }

    fn sine(freq_hz: f32, duration_secs: f32, volume: f32) -> Self {
        let sample_rate = 22_050u32;
        let n = (duration_secs * sample_rate as f32).max(1.0) as usize;
        let mut pcm = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / sample_rate as f32;
            let env = (1.0 - t / duration_secs).clamp(0.0, 1.0);
            pcm.push((t * freq_hz * std::f32::consts::TAU).sin() * volume * env);
        }
        Self {
            samples: pcm.into(),
            sample_rate,
            channels: 1,
        }
    }
}

pub fn write_sine_wav(path: &Path, freq_hz: f32, duration_secs: f32, volume: f32) -> Result<()> {
    write_clip_wav(path, &PcmClip::sine(freq_hz, duration_secs, volume))
}

fn write_clip_wav(path: &Path, clip: &PcmClip) -> Result<()> {
    let pcm: Vec<i16> = clip
        .samples
        .iter()
        .map(|sample| (*sample * i16::MAX as f32) as i16)
        .collect();
    write_pcm_wav(path, clip.sample_rate, clip.channels, &pcm)
}

fn write_pcm_wav(path: &Path, sample_rate: u32, channels: u16, pcm: &[i16]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let channels = channels.max(1);
    let data_len = (pcm.len() * 2) as u32;
    let byte_rate = sample_rate * u32::from(channels) * 2;
    let block_align = channels * 2;
    let mut bytes = Vec::with_capacity(44 + pcm.len() * 2);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(b"fmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&channels.to_le_bytes());
    bytes.extend_from_slice(&sample_rate.to_le_bytes());
    bytes.extend_from_slice(&byte_rate.to_le_bytes());
    bytes.extend_from_slice(&block_align.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&data_len.to_le_bytes());
    for sample in pcm {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }
    std::fs::write(path, bytes)?;
    Ok(())
}

fn first_existing_sound(paths: &AppPaths, relative_or_stem: &str) -> Option<PathBuf> {
    let resolved = paths.resolve_res(relative_or_stem);
    if resolved.is_file() {
        return Some(resolved);
    }
    if resolved.extension().is_some() {
        for ext in ["ogg", "wav"] {
            let alt = resolved.with_extension(ext);
            if alt.is_file() {
                return Some(alt);
            }
        }
    }
    let stem = std::path::Path::new(relative_or_stem)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or(relative_or_stem);
    for dir in [paths.res.join(crate::assets::SOUNDS_DIR), paths.res.clone()] {
        for ext in ["ogg", "wav"] {
            let path = dir.join(format!("{stem}.{ext}"));
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

#[cfg(test)]
fn magic_is_riff(path: &Path) -> bool {
    let Ok(bytes) = std::fs::read(path) else {
        return false;
    };
    bytes.len() > 44 && bytes.starts_with(b"RIFF") && bytes[8..12] == *b"WAVE"
}

fn transcode_ogg_to_i16(src: &Path) -> Result<(Vec<i16>, u32, u16)> {
    let file = std::fs::File::open(src)?;
    let mut reader = lewton::inside_ogg::OggStreamReader::new(file)
        .map_err(|e| ShotError::Other(format!("ogg decode {src:?}: {e}")))?;
    let sample_rate = reader.ident_hdr.audio_sample_rate;
    let channels = u16::from(reader.ident_hdr.audio_channels.max(1));
    let mut pcm = Vec::new();
    loop {
        match reader.read_dec_packet_itl() {
            Ok(Some(samples)) => pcm.extend(samples),
            Ok(None) => break,
            Err(e) => return Err(ShotError::Other(format!("ogg packet {src:?}: {e}"))),
        }
    }
    if pcm.is_empty() {
        return Err(ShotError::Other(format!(
            "ogg produced no samples: {src:?}"
        )));
    }
    Ok((pcm, sample_rate, channels))
}

fn decode_file_to_clip(src: &Path) -> Result<PcmClip> {
    match src
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.to_ascii_lowercase())
        .as_deref()
    {
        Some("ogg") => {
            let (pcm, rate, channels) = transcode_ogg_to_i16(src)?;
            Ok(PcmClip::from_i16(&pcm, rate, channels))
        }
        Some("wav") => {
            let bytes = std::fs::read(src)?;
            parse_pcm16_wav(&bytes)
        }
        _ => Err(ShotError::Other(format!("unsupported sound {src:?}"))),
    }
}

fn parse_pcm16_wav(bytes: &[u8]) -> Result<PcmClip> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(ShotError::Other("not a wave file".into()));
    }
    let channels = u16::from_le_bytes([bytes[22], bytes[23]]).max(1);
    let sample_rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]).max(1);
    let bits = u16::from_le_bytes([bytes[34], bytes[35]]);
    if bits != 16 {
        return Err(ShotError::Other("only 16-bit pcm wav is supported".into()));
    }
    let data_len = u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]) as usize;
    let start = 44;
    let end = (start + data_len).min(bytes.len());
    let mut pcm = Vec::with_capacity((end - start) / 2);
    let mut idx = start;
    while idx + 1 < end {
        pcm.push(i16::from_le_bytes([bytes[idx], bytes[idx + 1]]));
        idx += 2;
    }
    Ok(PcmClip::from_i16(&pcm, sample_rate, channels))
}

fn stage_clip(paths: &AppPaths, relative: &str, fallback_hz: f32) -> PcmClip {
    if let Some(src) = first_existing_sound(paths, relative) {
        if let Ok(clip) = decode_file_to_clip(&src) {
            if !clip.samples.is_empty() {
                return clip;
            }
        }
    }
    PcmClip::sine(fallback_hz, 0.08, 0.35)
}

/// 读设置里的两条路径。找不到文件就用 880Hz / 180Hz 的短蜂鸣。
pub fn load_sfx_clips(paths: &AppPaths, hit: &str, miss: &str) -> Result<(PcmClip, PcmClip)> {
    paths.ensure_dirs()?;
    Ok((
        stage_clip(paths, hit, 880.0),
        stage_clip(paths, miss, 180.0),
    ))
}

pub fn ensure_fallback_sounds(paths: &AppPaths) -> Result<(PathBuf, PathBuf)> {
    let (hit, miss) = load_sfx_clips(
        paths,
        crate::assets::DEFAULT_HIT_SOUND,
        crate::assets::DEFAULT_MISS_SOUND,
    )?;
    let hit_path = paths.cache.join("sfx_hit.wav");
    let miss_path = paths.cache.join("sfx_miss.wav");
    write_clip_wav(&hit_path, &hit)?;
    write_clip_wav(&miss_path, &miss)?;
    Ok((hit_path, miss_path))
}

#[cfg(test)]
mod tests {
    use super::{
        ensure_fallback_sounds, first_existing_sound, load_sfx_clips, magic_is_riff, write_sine_wav,
    };
    use crate::paths::AppPaths;

    #[test]
    fn first_existing_sound_should_prefer_ogg() {
        let dir = tempfile::tempdir().unwrap();
        let paths = AppPaths::from_root(dir.path().to_path_buf());
        std::fs::create_dir_all(&paths.res).unwrap();
        std::fs::write(paths.res.join("命中.ogg"), b"ogg").unwrap();
        std::fs::write(paths.res.join("命中.wav"), b"wav").unwrap();
        let path = first_existing_sound(&paths, "命中").unwrap();
        assert_eq!(path.extension().and_then(|e| e.to_str()), Some("ogg"));
    }

    #[test]
    fn write_sine_wav_should_create_riff_header() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("beep.wav");
        write_sine_wav(&path, 440.0, 0.05, 0.2).unwrap();
        assert!(magic_is_riff(&path));
    }

    #[test]
    fn bundled_sounds_should_stage_ascii_wav() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let (hit, miss) = ensure_fallback_sounds(&paths).unwrap();
        assert_eq!(hit.file_name().unwrap(), "sfx_hit.wav");
        assert_eq!(miss.file_name().unwrap(), "sfx_miss.wav");
        assert!(magic_is_riff(&hit));
        assert!(magic_is_riff(&miss));
        let (hit_clip, miss_clip) = load_sfx_clips(
            &paths,
            crate::assets::DEFAULT_HIT_SOUND,
            crate::assets::DEFAULT_MISS_SOUND,
        )
        .unwrap();
        assert!(!hit_clip.samples.is_empty());
        assert!(!miss_clip.samples.is_empty());
    }
}

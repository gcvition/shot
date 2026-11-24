//! 音效播放：独立线程 + rodio，避免卡 Bevy 主循环。
//!
//! [`spawn_player`] 返回一个 `Sender<SfxEvent>`。训练里发 [`SfxEvent`] 即可。
//! 没有默认输出设备时线程会把消息吞掉，游戏照常跑，只是没声音。
//!
//! M0 只装了命中/空枪两条 clip；新变体目前没有对应的音频文件，收到就忽略。

use std::num::NonZero;
use std::sync::mpsc::{self, Sender};
use std::thread;

use rodio::DeviceSinkBuilder;
use rodio::buffer::SamplesBuffer;
use rodio::source::Source;

use crate::wav::PcmClip;

/// 一种要播的声音。
///
/// `pan`/`vol` 给空间音效用（M6 的脚步、AI 枪声）；
/// `pan` 0 = 正中，-1 = 全左，1 = 全右。
#[derive(Clone, Copy, Debug)]
pub enum SfxEvent {
    Hit,
    Miss,
    Headshot,
    Kill,
    Shot,
    Reload,
    Empty,
    Footstep { pan: f32, vol: f32 },
    BotShot { pan: f32 },
    Damaged,
}

/// 后台播放器。`Hit` 播命中 clip，`Miss` 播空枪，其余变体暂无音频（忽略）。
pub fn spawn_player(hit: PcmClip, miss: PcmClip, volume: f32) -> Sender<SfxEvent> {
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new()
        .name("shot-sfx".into())
        .spawn(move || {
            let Ok(mut handle) = DeviceSinkBuilder::open_default_sink() else {
                while rx.recv().is_ok() {}
                return;
            };
            handle.log_on_drop(false);
            while let Ok(event) = rx.recv() {
                let clip = match event {
                    SfxEvent::Hit => &hit,
                    SfxEvent::Miss => &miss,
                    _ => continue,
                };
                if let Some(source) = clip_to_source(clip, volume) {
                    handle.mixer().add(source);
                }
            }
            drop(handle);
        });
    tx
}

fn clip_to_source(clip: &PcmClip, volume: f32) -> Option<impl Source + Send + 'static> {
    let channels = NonZero::new(clip.channels)?;
    let sample_rate = NonZero::new(clip.sample_rate)?;
    if clip.samples.is_empty() {
        return None;
    }
    let buffer = SamplesBuffer::new(channels, sample_rate, clip.samples.to_vec());
    Some(buffer.amplify(volume.clamp(0.0, 1.0)))
}

#[cfg(test)]
mod tests {
    use super::clip_to_source;
    use crate::paths::AppPaths;
    use crate::wav::load_sfx_clips;

    #[test]
    fn bundled_clips_should_become_playable_sources() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let (hit, miss) = load_sfx_clips(
            &paths,
            crate::assets::DEFAULT_HIT_SOUND,
            crate::assets::DEFAULT_MISS_SOUND,
        )
        .unwrap();
        assert!(clip_to_source(&hit, 1.0).is_some());
        assert!(clip_to_source(&miss, 1.0).is_some());
    }
}

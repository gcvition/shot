use std::num::NonZero;
use std::sync::mpsc::{self, Sender};
use std::thread;

use rodio::buffer::SamplesBuffer;
use rodio::source::Source;
use rodio::DeviceSinkBuilder;

use crate::wav::PcmClip;

pub fn spawn_player(hit: PcmClip, miss: PcmClip, volume: f32) -> Sender<bool> {
    let (tx, rx) = mpsc::channel();
    let _ = thread::Builder::new().name("shot-sfx".into()).spawn(move || {
        let Ok(mut handle) = DeviceSinkBuilder::open_default_sink() else {
            while rx.recv().is_ok() {}
            return;
        };
        handle.log_on_drop(false);
        while let Ok(is_hit) = rx.recv() {
            let clip = if is_hit { &hit } else { &miss };
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
    use crate::wav::load_sfx_clips;
    use crate::paths::AppPaths;

    #[test]
    fn bundled_clips_should_become_playable_sources() {
        let paths = AppPaths::from_root(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let (hit, miss) = load_sfx_clips(&paths).unwrap();
        assert!(clip_to_source(&hit, 1.0).is_some());
        assert!(clip_to_source(&miss, 1.0).is_some());
    }
}

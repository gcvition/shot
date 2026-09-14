use std::path::{Path, PathBuf};

use openh264::encoder::{Encoder, EncoderConfig};
use openh264::formats::YUVBuffer;
use openh264::OpenH264API;

use crate::error::{Result, ShotError};
use crate::video::mux::{annexb_to_avcc, write_avc_mp4};
use crate::video::VideoSink;

pub struct OpenH264Sink {
    encoder: Encoder,
    width: u32,
    height: u32,
    fps: u32,
    output: PathBuf,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    samples: Vec<Vec<u8>>,
    keyframes: Vec<bool>,
}

impl OpenH264Sink {
    pub fn new(output: &Path, width: u32, height: u32, fps: u32) -> Result<Self> {
        let api = OpenH264API::from_source();
        let config = EncoderConfig::new().skip_frames(false);
        let encoder = Encoder::with_api_config(api, config)
            .map_err(|e| ShotError::Other(format!("openh264 init: {e}")))?;
        Ok(Self {
            encoder,
            width,
            height,
            fps,
            output: output.to_path_buf(),
            sps: None,
            pps: None,
            samples: Vec::new(),
            keyframes: Vec::new(),
        })
    }
}

impl VideoSink for OpenH264Sink {
    fn push_rgba(&mut self, frame: &[u8]) -> Result<()> {
        let yuv = rgba_to_i420(frame, self.width as usize, self.height as usize);
        let buffer = YUVBuffer::from_vec(yuv, self.width as usize, self.height as usize);
        let encoded = self
            .encoder
            .encode(&buffer)
            .map_err(|e| ShotError::Other(format!("openh264 encode: {e}")))?;
        let bytes = encoded.to_vec();
        if bytes.is_empty() {
            return Ok(());
        }
        let (mut samples, sps, pps, key) = annexb_to_avcc(&bytes);
        if let Some(sps) = sps {
            self.sps = Some(sps);
        }
        if let Some(pps) = pps {
            self.pps = Some(pps);
        }
        if let Some(sample) = samples.pop() {
            if !sample.is_empty() {
                self.samples.push(sample);
                self.keyframes.push(key || self.samples.len() == 1);
            }
        }
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<PathBuf> {
        let sps = self
            .sps
            .ok_or_else(|| ShotError::Other("openh264 missing SPS".into()))?;
        let pps = self
            .pps
            .ok_or_else(|| ShotError::Other("openh264 missing PPS".into()))?;
        write_avc_mp4(
            &self.output,
            self.width,
            self.height,
            self.fps,
            &sps,
            &pps,
            &self.samples,
            &self.keyframes,
        )?;
        Ok(self.output)
    }
}

fn rgba_to_i420(frame: &[u8], width: usize, height: usize) -> Vec<u8> {
    let y_size = width * height;
    let uv_size = (width / 2) * (height / 2);
    let mut out = vec![0u8; y_size + uv_size * 2];
    for y in 0..height {
        for x in 0..width {
            let i = (y * width + x) * 4;
            let r = frame[i] as f32;
            let g = frame[i + 1] as f32;
            let b = frame[i + 2] as f32;
            let yy = (0.257 * r + 0.504 * g + 0.098 * b + 16.0).clamp(0.0, 255.0) as u8;
            out[y * width + x] = yy;
            if y % 2 == 0 && x % 2 == 0 {
                let u = (-0.148 * r - 0.291 * g + 0.439 * b + 128.0).clamp(0.0, 255.0) as u8;
                let v = (0.439 * r - 0.368 * g - 0.071 * b + 128.0).clamp(0.0, 255.0) as u8;
                let uv_index = (y / 2) * (width / 2) + x / 2;
                out[y_size + uv_index] = u;
                out[y_size + uv_size + uv_index] = v;
            }
        }
    }
    out
}

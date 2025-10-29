use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream, StreamConfig};
use parking_lot::Mutex;
use tracing::{debug, error};

use shared::prelude::*;

pub struct AudioPlayer {
    queue: Arc<Mutex<VecDeque<i16>>>,
    muted: Arc<AtomicBool>,
    channels: usize,
    #[allow(dead_code)]
    sample_rate: u32,
    _stream: Stream,
}

impl AudioPlayer {
    pub fn new() -> Result<Self> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or_else(|| anyhow!("未检测到默认音频输出设备"))?;

        let supported = device
            .supported_output_configs()?
            .filter(|cfg| cfg.sample_format() == SampleFormat::I16)
            .max_by_key(|cfg| cfg.max_sample_rate().0)
            .ok_or_else(|| anyhow!("输出设备不支持 i16 格式"))?;

        let config = supported.with_max_sample_rate();
        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        let stream_config: StreamConfig = config.into();

        let queue = Arc::new(Mutex::new(VecDeque::<i16>::with_capacity(sample_rate as usize)));
        let muted = Arc::new(AtomicBool::new(false));

        let queue_cb = queue.clone();
        let muted_cb = muted.clone();

        let stream = device.build_output_stream(
            &stream_config,
            move |output: &mut [i16], _| {
                if muted_cb.load(Ordering::SeqCst) {
                    for sample in output.iter_mut() {
                        *sample = 0;
                    }
                    return;
                }

                let mut buffer = queue_cb.lock();
                for sample in output.iter_mut() {
                    *sample = buffer.pop_front().unwrap_or(0);
                }
            },
            move |err| {
                error!(?err, "音频输出流错误");
            },
            None,
        )?;

        stream.play()?;

        Ok(Self {
            queue,
            muted,
            channels,
            sample_rate,
            _stream: stream,
        })
    }

    pub fn enqueue(&self, frame: AudioFrame) {
        if frame.force_play {
            self.muted.store(false, Ordering::SeqCst);
        }

        if self.muted.load(Ordering::SeqCst) {
            return;
        }

        if frame.channels as usize != self.channels {
            debug!(
                expected = self.channels,
                received = frame.channels,
                "忽略声道数不匹配的音频帧"
            );
            return;
        }

        let mut buffer = self.queue.lock();
        for chunk in frame.data.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            buffer.push_back(sample);
        }
    }

    pub fn set_muted(&self, muted: bool) {
        self.muted.store(muted, Ordering::SeqCst);
    }

    #[allow(dead_code)]
    pub fn is_muted(&self) -> bool {
        self.muted.load(Ordering::SeqCst)
    }

    pub fn muted_handle(&self) -> Arc<AtomicBool> {
        self.muted.clone()
    }
}

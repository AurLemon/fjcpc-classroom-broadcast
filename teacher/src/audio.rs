use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, StreamConfig};
use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::{debug, error, info};

use shared::prelude::*;

use crate::server::TeacherState;

#[derive(Clone)]
pub struct AudioBroadcaster {
    state: Arc<TeacherState>,
    tasks: Arc<Mutex<Option<AudioTasks>>>,
    running: Arc<AtomicBool>,
    force_play: Arc<AtomicBool>,
    frame_counter: Arc<AtomicU64>,
}

struct AudioTasks {
    capture: JoinHandle<Result<()>>,
    dispatch: JoinHandle<()>,
}

struct AudioPacket {
    data: Vec<u8>,
    sample_rate: u32,
    channels: u16,
}

impl AudioBroadcaster {
    pub fn new(state: Arc<TeacherState>, force_play: bool) -> Self {
        Self {
            state,
            tasks: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            force_play: Arc::new(AtomicBool::new(force_play)),
            frame_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn start(&self) -> Result<()> {
        if self.tasks.lock().is_some() {
            debug!("Audio broadcaster already running");
            return Ok(());
        }

        let (tx, mut rx) = mpsc::unbounded_channel::<AudioPacket>();
        self.running.store(true, Ordering::SeqCst);

        let running_capture = self.running.clone();
        let capture_sender = tx.clone();
        let capture_handle =
            tokio::task::spawn_blocking(move || run_capture(capture_sender, running_capture));

        let state = self.state.clone();
        let running_dispatch = self.running.clone();
        let force_flag = self.force_play.clone();
        let frame_counter = self.frame_counter.clone();

        let dispatch_handle = tokio::spawn(async move {
            while let Some(packet) = rx.recv().await {
                let frame_id = frame_counter.fetch_add(1, Ordering::Relaxed) + 1;
                let timestamp_ms = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;

                let frame = AudioFrame {
                    frame_id,
                    timestamp_ms,
                    sample_rate: packet.sample_rate,
                    channels: packet.channels as u8,
                    force_play: force_flag.load(Ordering::SeqCst),
                    data: packet.data,
                };

                state.broadcast_audio(frame);
            }

            running_dispatch.store(false, Ordering::SeqCst);
        });

        *self.tasks.lock() = Some(AudioTasks {
            capture: capture_handle,
            dispatch: dispatch_handle,
        });

        info!("音频广播已启动");
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(tasks) = self.tasks.lock().take() {
            tasks.capture.abort();
            tasks.dispatch.abort();
        }
        info!("音频广播已停止");
    }

    pub fn set_force_play(&self, force: bool) {
        self.force_play.store(force, Ordering::SeqCst);
    }

    #[cfg(feature = "ui")]
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    #[cfg(feature = "ui")]
    pub fn is_force_play(&self) -> bool {
        self.force_play.load(Ordering::SeqCst)
    }
}

fn run_capture(tx: mpsc::UnboundedSender<AudioPacket>, running: Arc<AtomicBool>) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| anyhow!("未检测到可用的录音设备"))?;

    let supported_configs = device.supported_input_configs()?;
    let desired_config = supported_configs
        .filter(|cfg| cfg.sample_format() == SampleFormat::I16)
        .max_by_key(|cfg| cfg.max_sample_rate().0)
        .ok_or_else(|| anyhow!("录音设备不支持 i16 格式"))?;

    let config = desired_config.with_max_sample_rate();
    let sample_rate = config.sample_rate().0;
    let channels = config.channels();
    let stream_config: StreamConfig = config.into();

    let frame_samples_per_channel = (sample_rate as usize / 50).max(1); // ~20ms per frame
    let frame_samples = frame_samples_per_channel * channels as usize;

    let running_callback = running.clone();
    let running_error = running.clone();
    let sender_callback = tx.clone();

    let mut sample_buffer: Vec<i16> = Vec::with_capacity(frame_samples * 2);

    let stream = device.build_input_stream(
        &stream_config,
        move |data: &[i16], _| {
            sample_buffer.extend_from_slice(data);
            while sample_buffer.len() >= frame_samples {
                let frame: Vec<i16> = sample_buffer.drain(..frame_samples).collect();
                let mut bytes = Vec::with_capacity(frame.len() * 2);
                for sample in frame {
                    bytes.extend_from_slice(&sample.to_le_bytes());
                }
                if sender_callback
                    .send(AudioPacket {
                        data: bytes,
                        sample_rate,
                        channels,
                    })
                    .is_err()
                {
                    running_callback.store(false, Ordering::SeqCst);
                    break;
                }
            }
        },
        move |err| {
            error!(?err, "音频输入流发生错误");
            running_error.store(false, Ordering::SeqCst);
        },
        None,
    )?;

    stream.play()?;

    while running.load(Ordering::SeqCst) {
        std::thread::sleep(Duration::from_millis(20));
    }

    drop(stream);
    Ok(())
}

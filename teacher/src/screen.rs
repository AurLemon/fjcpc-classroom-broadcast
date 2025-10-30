use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use image::{codecs::jpeg::JpegEncoder, ColorType};
use parking_lot::Mutex;
use screenshots::Screen;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, warn};

use shared::prelude::*;

use crate::server::TeacherState;

#[derive(Clone)]
pub struct ScreenBroadcaster {
    state: Arc<TeacherState>,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl ScreenBroadcaster {
    pub fn new(state: Arc<TeacherState>) -> Self {
        Self {
            state,
            task: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn start(&self, mode: BroadcastMode) -> Result<()> {
        if self.task.lock().is_some() {
            debug!("Screen broadcaster already running");
            return Ok(());
        }

        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            if let Err(err) = capture_loop(state, mode).await {
                error!(?err, "Screen capture loop exited with error");
            } else {
                debug!("Screen capture loop terminated");
            }
        });

        *self.task.lock() = Some(handle);
        Ok(())
    }

    pub async fn stop(&self) {
        if let Some(handle) = self.task.lock().take() {
            handle.abort();
        }
    }
}

async fn capture_loop(state: Arc<TeacherState>, mode: BroadcastMode) -> Result<()> {
    let base_cfg = state.broadcast_config();
    let interval_duration = Duration::from_millis((1000.0 / base_cfg.fps as f64) as u64);
    let primary = Screen::from_point(0, 0).context("无法找到主显示器")?;
    let screen = Arc::new(primary);
    let mut ticker = time::interval(interval_duration.max(Duration::from_millis(16)));

    loop {
        ticker.tick().await;

        let frame_id = state.next_frame_id();
        let screen_clone = screen.clone();
        let cfg = base_cfg.clone();

        let result =
            tokio::task::spawn_blocking(move || capture_frame(screen_clone, frame_id, mode, &cfg))
                .await;

        match result {
            Ok(Ok(frame)) => {
                state.broadcast_video(frame);
            }
            Ok(Err(err)) => {
                warn!(?err, "屏幕捕获失败");
            }
            Err(join_err) => {
                if join_err.is_cancelled() {
                    debug!("屏幕捕获任务已取消");
                    break;
                } else if let Some(err) = join_err.try_into_panic().err() {
                    error!("屏幕捕获线程崩溃: {:?}", err);
                    break;
                }
            }
        }
    }

    Ok(())
}

impl ScreenBroadcaster {
    pub fn handle_disconnection(&self, _student_id: &str) {}
}

fn capture_frame(
    screen: Arc<Screen>,
    frame_id: u64,
    mode: BroadcastMode,
    cfg: &BroadcastConfig,
) -> Result<VideoFrame> {
    let image = screen.capture().context("执行屏幕截取失败")?;
    let width = image.width();
    let height = image.height();
    let raw = image.into_raw();

    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for pixel in raw.chunks_exact(4) {
        // BGRA -> RGB
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }

    let mut jpeg_bytes = Vec::new();
    {
        let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, cfg.jpeg_quality);
        encoder.encode(&rgb, width as u32, height as u32, ColorType::Rgb8)?;
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(VideoFrame {
        frame_id,
        timestamp_ms,
        source: BroadcastSource::Teacher,
        codec: VideoCodec::Jpeg,
        width,
        height,
        fullscreen: matches!(mode, BroadcastMode::Fullscreen),
        data: jpeg_bytes,
    })
}

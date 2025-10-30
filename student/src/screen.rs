use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use parking_lot::Mutex;
use screenshots::Screen;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{debug, error, warn};

use shared::prelude::*;

pub struct ScreenStreamer {
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    running: Arc<AtomicBool>,
    frame_counter: Arc<AtomicU64>,
}

impl ScreenStreamer {
    pub fn new() -> Self {
        Self {
            task: Arc::new(Mutex::new(None)),
            running: Arc::new(AtomicBool::new(false)),
            frame_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    pub async fn start(
        &self,
        sender: mpsc::UnboundedSender<StudentToTeacher>,
        student_id: String,
        student_name: String,
    ) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            debug!("屏幕广播已在运行");
            return Ok(());
        }

        let running = self.running.clone();
        let frame_counter = self.frame_counter.clone();
        let task_handle = tokio::spawn(async move {
            if let Err(err) =
                capture_loop(sender, running, frame_counter, student_id, student_name).await
            {
                error!(?err, "学生屏幕捕获失败");
            }
        });

        *self.task.lock() = Some(task_handle);
        Ok(())
    }

    pub async fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        if let Some(handle) = self.task.lock().take() {
            handle.abort();
        }
    }
}

async fn capture_loop(
    sender: mpsc::UnboundedSender<StudentToTeacher>,
    running: Arc<AtomicBool>,
    counter: Arc<AtomicU64>,
    student_id: String,
    student_name: String,
) -> Result<()> {
    let base_cfg = BroadcastConfig::default();
    let interval = Duration::from_millis((1000.0 / base_cfg.fps as f64) as u64);
    let primary = Screen::from_point(0, 0).context("无法访问主显示器")?;
    let screen = Arc::new(primary);
    let mut ticker = time::interval(interval.max(Duration::from_millis(16)));

    while running.load(Ordering::SeqCst) {
        ticker.tick().await;
        let frame_id = counter.fetch_add(1, Ordering::Relaxed) + 1;
        let screen_clone = screen.clone();
        let student_id_clone = student_id.clone();
        let student_name_clone = student_name.clone();
        let cfg = base_cfg.clone();

        let result = tokio::task::spawn_blocking(move || {
            capture_frame(
                screen_clone,
                frame_id,
                &student_id_clone,
                &student_name_clone,
                &cfg,
            )
        })
        .await;

        match result {
            Ok(Ok(frame)) => {
                if sender.send(StudentToTeacher::Video(frame)).is_err() {
                    warn!("发送屏幕帧失败，教师端可能已断开");
                    break;
                }
            }
            Ok(Err(err)) => {
                warn!(?err, "截屏失败");
            }
            Err(join_err) => {
                if join_err.is_cancelled() {
                    debug!("屏幕捕获任务已取消");
                    break;
                } else {
                    error!(?join_err, "屏幕捕获线程异常退出");
                    break;
                }
            }
        }
    }

    Ok(())
}

fn capture_frame(
    screen: Arc<Screen>,
    frame_id: u64,
    student_id: &str,
    student_name: &str,
    cfg: &BroadcastConfig,
) -> Result<VideoFrame> {
    let image = screen.capture().context("执行截屏失败")?;
    let width = image.width();
    let height = image.height();
    let raw = image.into_raw();

    let mut rgb = Vec::with_capacity((width * height * 3) as usize);
    for pixel in raw.chunks_exact(4) {
        rgb.push(pixel[2]);
        rgb.push(pixel[1]);
        rgb.push(pixel[0]);
    }

    let mut jpeg = Vec::new();
    {
        let mut encoder =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, cfg.jpeg_quality);
        encoder.encode(&rgb, width as u32, height as u32, image::ColorType::Rgb8)?;
    }

    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    Ok(VideoFrame {
        frame_id,
        timestamp_ms,
        source: BroadcastSource::Student {
            student_id: student_id.to_string(),
            student_name: Some(student_name.to_string()),
        },
        codec: VideoCodec::Jpeg,
        width,
        height,
        fullscreen: true,
        data: jpeg,
    })
}

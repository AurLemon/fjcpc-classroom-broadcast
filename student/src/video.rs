use std::sync::mpsc::{self, Sender};
use std::thread;

use anyhow::Result;
use minifb::{Scale, ScaleMode, Window, WindowOptions};
use tracing::{debug, error, warn};

use shared::prelude::*;

pub struct VideoRenderer {
    sender: Sender<VideoCommand>,
}

impl VideoRenderer {
    pub fn new() -> Self {
        let (tx, rx) = mpsc::channel::<VideoCommand>();
        thread::Builder::new()
            .name("student-video-renderer".into())
            .spawn(move || render_loop(rx))
            .expect("Failed to spawn video renderer thread");

        Self { sender: tx }
    }

    pub fn display_frame(&self, frame: VideoFrame, mode: BroadcastMode) {
        if let Err(err) = self.sender.send(VideoCommand::Frame { frame, mode }) {
            error!(?err, "视频渲染线程不可用");
        }
    }

    pub fn stop(&self) {
        if let Err(err) = self.sender.send(VideoCommand::Stop) {
            warn!(?err, "停止视频渲染失败");
        }
    }
}

impl Drop for VideoRenderer {
    fn drop(&mut self) {
        let _ = self.sender.send(VideoCommand::Exit);
    }
}

enum VideoCommand {
    Frame {
        frame: VideoFrame,
        mode: BroadcastMode,
    },
    Stop,
    Exit,
}

fn render_loop(receiver: mpsc::Receiver<VideoCommand>) {
    let mut window: Option<Window> = None;
    let mut current_mode = BroadcastMode::Window;

    for command in receiver {
        match command {
            VideoCommand::Frame { frame, mode } => match decode_frame(&frame) {
                Ok((buffer, width, height)) => {
                    ensure_window(&mut window, width, height, mode);
                    if let Some(win) = window.as_mut() {
                        if !win.is_open() {
                            debug!("视频窗口已关闭，重新创建");
                            window = create_window(width, height, mode).ok();
                        }

                        if let Some(win) = window.as_mut() {
                            if current_mode != mode {
                                apply_mode(win, mode);
                                current_mode = mode;
                            }

                            if let Err(err) = win.update_with_buffer(&buffer, width, height) {
                                error!(?err, "刷新视频窗口失败");
                                window = None;
                            }
                        }
                    }
                }
                Err(err) => {
                    error!(?err, "解码视频帧失败");
                }
            },
            VideoCommand::Stop => {
                if let Some(win) = window.take() {
                    drop(win);
                }
            }
            VideoCommand::Exit => break,
        }
    }
}

fn ensure_window(window: &mut Option<Window>, width: usize, height: usize, mode: BroadcastMode) {
    if let Some(existing) = window {
        if existing.get_size().0 != width || existing.get_size().1 != height {
            *window = create_window(width, height, mode).ok();
        }
    } else {
        *window = create_window(width, height, mode).ok();
    }
}

fn create_window(width: usize, height: usize, mode: BroadcastMode) -> Result<Window> {
    let mut options = WindowOptions::default();
    options.resize = false;
    options.scale = Scale::X1;
    options.scale_mode = ScaleMode::AspectRatioStretch;

    let mut window = Window::new("课堂广播", width, height, options)?;
    window.limit_update_rate(None);
    apply_mode(&mut window, mode);
    Ok(window)
}

fn apply_mode(_window: &mut Window, _mode: BroadcastMode) {
    // minifb 0.24 不支持在运行时切换全屏，这里仅保留占位逻辑。
}

fn decode_frame(frame: &VideoFrame) -> Result<(Vec<u32>, usize, usize)> {
    match frame.codec {
        VideoCodec::Jpeg => {
            let dyn_img = image::load_from_memory(&frame.data)?;
            let rgb = dyn_img.to_rgb8();
            Ok(rgb_to_u32(&rgb))
        }
        VideoCodec::Bgra => {
            let width = frame.width as usize;
            let height = frame.height as usize;
            let mut buffer = Vec::with_capacity(width * height);
            for pixel in frame.data.chunks_exact(4) {
                let r = pixel[2] as u32;
                let g = pixel[1] as u32;
                let b = pixel[0] as u32;
                buffer.push((r << 16) | (g << 8) | b);
            }
            Ok((buffer, width, height))
        }
    }
}

fn rgb_to_u32(image: &image::RgbImage) -> (Vec<u32>, usize, usize) {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let mut buffer = Vec::with_capacity(width * height);
    for pixel in image.pixels() {
        let r = pixel[0] as u32;
        let g = pixel[1] as u32;
        let b = pixel[2] as u32;
        buffer.push((r << 16) | (g << 8) | b);
    }
    (buffer, width, height)
}

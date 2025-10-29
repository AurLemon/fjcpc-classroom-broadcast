use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use parking_lot::Mutex;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::net::TcpStream;
use tokio::signal;
use tokio::sync::mpsc;
use tokio::time;
use tracing::{error, info, warn};
use uuid::Uuid;

use shared::prelude::*;

use crate::audio::AudioPlayer;
use crate::files::FileDownloadManager;
use crate::screen::ScreenStreamer;
use crate::video::VideoRenderer;

pub struct StudentApp {
    config: StudentConfig,
}

impl StudentApp {
    pub fn new(config: StudentConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<()> {
        let address = self.config.teacher_addr();
        info!(%address, "连接教师端");
        let stream = TcpStream::connect(&address)
            .await
            .with_context(|| format!("无法连接教师端 {address}"))?;
        stream.set_nodelay(true)?;

        let (mut reader, mut writer) = stream.into_split();
        let (tx, mut rx) = mpsc::unbounded_channel::<StudentToTeacher>();

        let writer_task = tokio::spawn(async move {
            while let Some(message) = rx.recv().await {
                if let Err(err) = write_message(&mut writer, &message).await {
                    error!(?err, "向教师端发送数据失败");
                    break;
                }
            }
        });

        let video = VideoRenderer::new();
        let audio = Arc::new(AudioPlayer::new()?);
        let files = Arc::new(FileDownloadManager::new(
            self.config.download_path.clone(),
            self.config.auto_open_file,
        ));
        let screen_streamer = ScreenStreamer::new();

        let running = Arc::new(AtomicBool::new(true));
        let forced_fullscreen = Arc::new(AtomicBool::new(false));
        let current_mode = Arc::new(Mutex::new(BroadcastMode::Window));

        send_hello(&self.config, &tx)?;

        spawn_heartbeat(tx.clone(), running.clone());
        let muted_handle = audio.muted_handle();
        spawn_command_loop(
            tx.clone(),
            muted_handle,
            running.clone(),
        );

        tokio::select! {
            _ = signal::ctrl_c() => {
                info!("收到 Ctrl+C, 正在退出");
            }
            _ = async {
                while running.load(Ordering::SeqCst) {
                    match read_message::<_, TeacherToStudent>(&mut reader).await {
                        Ok(message) => {
                            if let Err(err) = handle_message(
                                &self.config,
                                &video,
                                audio.clone(),
                                files.clone(),
                                &screen_streamer,
                                &tx,
                                message,
                                current_mode.clone(),
                                forced_fullscreen.clone(),
                            ).await {
                                error!(?err, "处理教师端消息失败");
                            }
                        }
                        Err(err) => {
                            warn!(?err, "读取教师端消息失败，准备断开");
                            break;
                        }
                    }
                }
            } => {}
        }

        running.store(false, Ordering::SeqCst);
        screen_streamer.stop().await;
        video.stop();
        audio.set_muted(true);
        drop(tx);
        writer_task.abort();

        info!("学生端已退出");
        Ok(())
    }
}

fn send_hello(config: &StudentConfig, tx: &mpsc::UnboundedSender<StudentToTeacher>) -> Result<()> {
    let message = StudentToTeacher::Hello(HelloMessage {
        student_id: config.student_id.clone(),
        student_name: config.student_name.clone(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: StudentCapabilities {
            receive_video: true,
            send_video: true,
            receive_audio: true,
            send_audio: false,
            file_transfer: true,
        },
    });
    tx.send(message)?;
    Ok(())
}

fn spawn_heartbeat(tx: mpsc::UnboundedSender<StudentToTeacher>, running: Arc<AtomicBool>) {
    tokio::spawn(async move {
        let mut ticker = time::interval(Duration::from_secs(5));
        while running.load(Ordering::SeqCst) {
            ticker.tick().await;
            let heartbeat = StudentToTeacher::Heartbeat(Heartbeat {
                timestamp_ms: current_millis(),
            });
            if tx.send(heartbeat).is_err() {
                break;
            }
        }
    });
}

fn spawn_command_loop(
    tx: mpsc::UnboundedSender<StudentToTeacher>,
    muted_flag: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        let stdin = tokio::io::stdin();
        let reader = BufReader::new(stdin);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let mut parts = trimmed.split_whitespace();
            match parts.next().unwrap_or("") {
                "help" => print_help(),
                "upload" => {
                    if let Some(path) = parts.next() {
                        if let Err(err) = upload_file(path.into(), &tx).await {
                            error!(?err, "上传文件失败");
                        }
                    } else {
                        warn!("用法: upload <文件路径>");
                    }
                }
                "mute" => {
                    muted_flag.store(true, Ordering::SeqCst);
                    info!("已静音");
                }
                "unmute" => {
                    muted_flag.store(false, Ordering::SeqCst);
                    info!("已取消静音");
                }
                "quit" | "exit" => {
                    running.store(false, Ordering::SeqCst);
                    break;
                }
                other => {
                    warn!(%other, "未知命令，输入 help 查看帮助");
                }
            }
        }
    });
}

fn print_help() {
    println!(
        "命令列表:\n  help               显示帮助\n  upload <路径>     向教师端上传文件\n  mute/unmute       切换音频播放\n  quit              退出学生客户端"
    );
}

async fn upload_file(path: PathBuf, tx: &mpsc::UnboundedSender<StudentToTeacher>) -> Result<()> {
    let metadata = tokio::fs::metadata(&path)
        .await
        .with_context(|| format!("无法读取文件信息: {}", path.display()))?;

    if !metadata.is_file() {
        return Err(anyhow!("{} 不是有效文件", path.display()));
    }

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| anyhow!("无法解析文件名"))?;

    let transfer_id = Uuid::new_v4();
    tx.send(StudentToTeacher::FileOffer(FileOffer {
        transfer_id,
        file_name: file_name.clone(),
        total_size: metadata.len(),
        auto_open: false,
    }))?;

    let mut file = tokio::fs::File::open(&path).await?;
    let mut buffer = vec![0u8; 64 * 1024];
    let mut offset = 0u64;

    loop {
        let read = file.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        tx.send(StudentToTeacher::FileChunk(FileChunk {
            transfer_id,
            offset,
            bytes: buffer[..read].to_vec(),
            final_chunk: false,
        }))?;
        offset += read as u64;
    }

    tx.send(StudentToTeacher::FileComplete(FileTransferComplete {
        transfer_id,
        success: true,
        message: Some(format!("{} 上传完成", file_name)),
    }))?;

    Ok(())
}

async fn handle_message(
    config: &StudentConfig,
    video: &VideoRenderer,
    audio: Arc<AudioPlayer>,
    files: Arc<FileDownloadManager>,
    screen_streamer: &ScreenStreamer,
    tx: &mpsc::UnboundedSender<StudentToTeacher>,
    message: TeacherToStudent,
    current_mode: Arc<Mutex<BroadcastMode>>,
    forced_fullscreen: Arc<AtomicBool>,
) -> Result<()> {
    match message {
        TeacherToStudent::Welcome(ack) => {
            forced_fullscreen.store(ack.force_fullscreen, Ordering::SeqCst);
            *current_mode.lock() = ack.broadcast_mode;
            info!("已连接教师端，默认模式: {:?}", ack.broadcast_mode);
        }
        TeacherToStudent::Broadcast(command) => {
            handle_broadcast_command(
                command,
                config,
                video,
                screen_streamer,
                &forced_fullscreen,
                current_mode.clone(),
                tx,
            )
            .await?;
        }
        TeacherToStudent::Video(frame) => {
            let mode = *current_mode.lock();
            video.display_frame(frame, mode);
        }
        TeacherToStudent::Audio(frame) => {
            audio.enqueue(frame);
        }
        TeacherToStudent::FileOffer(offer) => {
            let path = files.handle_offer(&offer).await?;
            info!(transfer = %offer.transfer_id, file = %offer.file_name, path = %path.display(), "收到文件传输请求");
        }
        TeacherToStudent::FileChunk(chunk) => {
            files.handle_chunk(&chunk).await?;
        }
        TeacherToStudent::FileComplete(done) => {
            if let Some(path) = files.handle_complete(&done).await? {
                if let Err(err) = open_file(&path) {
                    warn!(?err, path = %path.display(), "自动打开文件失败");
                }
            }
            let _ = tx.send(StudentToTeacher::Ack(
                done
                    .message
                    .clone()
                    .unwrap_or_else(|| "文件传输完成".into()),
            ));
        }
        TeacherToStudent::Heartbeat(_) => {
            let _ = tx.send(StudentToTeacher::Heartbeat(Heartbeat {
                timestamp_ms: current_millis(),
            }));
        }
        TeacherToStudent::Error(msg) => {
            warn!(?msg, "教师端错误提示");
        }
    }
    Ok(())
}

async fn handle_broadcast_command(
    command: BroadcastCommand,
    config: &StudentConfig,
    video: &VideoRenderer,
    screen_streamer: &ScreenStreamer,
    forced_fullscreen: &Arc<AtomicBool>,
    current_mode: Arc<Mutex<BroadcastMode>>,
    tx: &mpsc::UnboundedSender<StudentToTeacher>,
) -> Result<()> {
    match command {
        BroadcastCommand::Start { source, mode } => {
            let should_fullscreen = matches!(mode, BroadcastMode::Fullscreen)
                && (config.auto_fullscreen
                    || (forced_fullscreen.load(Ordering::SeqCst) && config.allow_forced_fullscreen));
            let actual_mode = if should_fullscreen {
                BroadcastMode::Fullscreen
            } else {
                BroadcastMode::Window
            };
            *current_mode.lock() = actual_mode;

            match source {
                BroadcastSource::Teacher => {
                    screen_streamer.stop().await;
                }
                BroadcastSource::Student { student_id, .. } => {
                    if student_id == config.student_id {
                        screen_streamer
                            .start(tx.clone(), config.student_id.clone(), config.student_name.clone())
                            .await?;
                    } else {
                        screen_streamer.stop().await;
                    }
                }
            }
        }
        BroadcastCommand::Stop => {
            screen_streamer.stop().await;
            video.stop();
            *current_mode.lock() = BroadcastMode::Window;
        }
        BroadcastCommand::RequestStudentShare { student_id } => {
            if student_id == config.student_id {
                screen_streamer
                    .start(tx.clone(), config.student_id.clone(), config.student_name.clone())
                    .await?;
            }
        }
    }
    Ok(())
}

fn open_file(path: &Path) -> Result<()> {
    Command::new("cmd")
        .args(["/C", "start", ""])
        .arg(path)
        .spawn()
        .with_context(|| format!("无法打开文件 {}", path.display()))?;
    Ok(())
}

fn current_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

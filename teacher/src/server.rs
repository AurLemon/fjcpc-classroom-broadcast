use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use anyhow::{anyhow, bail, Context, Result};
use parking_lot::{Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::signal;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tracing::{error, info, warn};
use uuid::Uuid;

use shared::prelude::*;

use crate::audio::AudioBroadcaster;
use crate::screen::ScreenBroadcaster;

#[cfg(feature = "ui")]
pub type CommandSender = mpsc::UnboundedSender<ServerCommand>;
pub type CommandReceiver = mpsc::UnboundedReceiver<ServerCommand>;

#[derive(Debug)]
pub enum ServerCommand {
    StartTeacher {
        mode: BroadcastMode,
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    StartStudent {
        student_id: String,
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    StopBroadcast {
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    SendFile {
        path: PathBuf,
        auto_open_override: bool,
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    AudioStart {
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    AudioStop {
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    AudioForce {
        force: bool,
        respond_to: Option<oneshot::Sender<Result<(), String>>>,
    },
    #[cfg(feature = "ui")]
    ListStudents {
        respond_to: oneshot::Sender<Result<Vec<StudentSummary>, String>>,
    },
    #[cfg(feature = "ui")]
    QueryStatus {
        respond_to: oneshot::Sender<ServerStatus>,
    },
    Quit,
}

#[cfg(feature = "ui")]
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub listen_addr: String,
    pub broadcast_mode: BroadcastMode,
    pub broadcast_source: Option<BroadcastSource>,
    pub audio_enabled: bool,
    pub audio_forced: bool,
    pub connected_students: usize,
}

pub struct TeacherServer {
    state: Arc<TeacherState>,
    screen: ScreenBroadcaster,
    audio: AudioBroadcaster,
    running: AtomicBool,
}

impl TeacherServer {
    pub fn new(config: TeacherConfig) -> Result<Self> {
        let config = Arc::new(config);
        let state = Arc::new(TeacherState::new(config.clone()));
        let screen = ScreenBroadcaster::new(state.clone());
        let audio = AudioBroadcaster::new(state.clone(), config.force_audio);
        Ok(Self {
            state,
            screen,
            audio,
            running: AtomicBool::new(false),
        })
    }

    pub async fn run(
        &self,
        auto_start_broadcast: bool,
        command_rx: Option<CommandReceiver>,
    ) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            bail!("Teacher server already running");
        }

        let addr = self.state.config.listen_addr();
        info!(%addr, "教师端监听启动");
        let listener = TcpListener::bind(&addr)
            .await
            .with_context(|| format!("无法监听 {addr}"))?;

        if auto_start_broadcast {
            self.start_teacher_broadcast(BroadcastMode::Fullscreen)
                .await?;
        }

        if self.state.config.enable_audio_by_default {
            if let Err(err) = self.audio.start().await {
                warn!(?err, "音频广播启动失败");
            }
        }

        let state = self.state.clone();
        let screen = self.screen.clone();
        let accept_task = tokio::spawn(async move {
            loop {
                match listener.accept().await {
                    Ok((stream, addr)) => {
                        let state = state.clone();
                        let screen = screen.clone();
                        tokio::spawn(async move {
                            if let Err(err) =
                                handle_student_connection(state, screen, stream, addr).await
                            {
                                error!(?err, %addr, "学生连接异常");
                            }
                        });
                    }
                    Err(err) => {
                        error!(?err, "监听器异常");
                        break;
                    }
                }
            }
        });

        let console_enabled = command_rx.is_none();
        if console_enabled {
            info!("输入 help 查看命令");
        }
        tokio::select! {
            result = self.command_loop(command_rx, console_enabled) => {
                if let Err(err) = result {
                    error!(?err, "命令循环异常");
                }
            }
            _ = signal::ctrl_c() => {
                info!("收到退出信号");
            }
        }

        self.running.store(false, Ordering::SeqCst);
        self.screen.stop().await;
        self.audio.stop().await;
        accept_task.abort();
        self.state.disconnect_all();
        Ok(())
    }

    async fn command_loop(
        &self,
        mut external: Option<CommandReceiver>,
        enable_console: bool,
    ) -> Result<()> {
        let mut lines = if enable_console {
            Some(BufReader::new(tokio::io::stdin()).lines())
        } else {
            None
        };

        loop {
            tokio::select! {
                maybe_cmd = async {
                    if let Some(rx) = external.as_mut() {
                        rx.recv().await
                    } else {
                        None
                    }
                }, if external.is_some() => {
                    match maybe_cmd {
                        Some(cmd) => {
                            if self.execute_command(cmd).await? {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                line = async {
                    if let Some(lines) = lines.as_mut() {
                        lines.next_line().await
                    } else {
                        Ok(None)
                    }
                }, if lines.is_some() => {
                    match line {
                        Ok(Some(content)) => {
                            if self.handle_console_command(content).await? {
                                break;
                            }
                        }
                        Ok(None) => break,
                        Err(err) => {
                            error!(?err, "读取命令失败");
                        }
                    }
                }
                else => break,
            }
        }

        Ok(())
    }

    async fn handle_console_command(&self, line: String) -> Result<bool> {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }

        let mut parts = trimmed.split_whitespace();
        match parts.next().unwrap_or("") {
            "help" => {
                self.print_help();
                Ok(false)
            }
            "students" => {
                self.print_students();
                Ok(false)
            }
            "start" => {
                let mode = match parts.next() {
                    Some("window") => BroadcastMode::Window,
                    _ => BroadcastMode::Fullscreen,
                };
                self.invoke_console_command(
                    ServerCommand::StartTeacher {
                        mode,
                        respond_to: None,
                    },
                    "开启广播失败",
                )
                .await
            }
            "stop" => {
                self.invoke_console_command(
                    ServerCommand::StopBroadcast { respond_to: None },
                    "停止广播失败",
                )
                .await
            }
            "spotlight" => {
                if let Some(student_id) = parts.next() {
                    self.invoke_console_command(
                        ServerCommand::StartStudent {
                            student_id: student_id.to_string(),
                            respond_to: None,
                        },
                        "学生屏幕广播失败",
                    )
                    .await
                } else {
                    warn!("用法: spotlight <student_id>");
                    Ok(false)
                }
            }
            "send" => {
                if let Some(path) = parts.next() {
                    let auto_open = matches!(parts.next(), Some("open"));
                    self.invoke_console_command(
                        ServerCommand::SendFile {
                            path: PathBuf::from(path),
                            auto_open_override: auto_open,
                            respond_to: None,
                        },
                        "文件分发失败",
                    )
                    .await
                } else {
                    warn!("用法: send <路径> [open]");
                    Ok(false)
                }
            }
            "audio" => match parts.next() {
                Some("on") => {
                    self.invoke_console_command(
                        ServerCommand::AudioStart { respond_to: None },
                        "音频广播启动失败",
                    )
                    .await
                }
                Some("off") => {
                    self.invoke_console_command(
                        ServerCommand::AudioStop { respond_to: None },
                        "停止音频广播失败",
                    )
                    .await
                }
                Some("force") => {
                    self.invoke_console_command(
                        ServerCommand::AudioForce {
                            force: true,
                            respond_to: None,
                        },
                        "设置音频强制播放失败",
                    )
                    .await
                }
                Some("allow") => {
                    self.invoke_console_command(
                        ServerCommand::AudioForce {
                            force: false,
                            respond_to: None,
                        },
                        "取消音频强制播放失败",
                    )
                    .await
                }
                _ => {
                    warn!("用法: audio <on|off|force|allow>");
                    Ok(false)
                }
            },
            "quit" | "exit" => self.execute_command(ServerCommand::Quit).await,
            other => {
                warn!(%other, "未知命令");
                Ok(false)
            }
        }
    }

    async fn invoke_console_command(
        &self,
        command: ServerCommand,
        error_message: &str,
    ) -> Result<bool> {
        match self.execute_command(command).await {
            Ok(should_exit) => Ok(should_exit),
            Err(err) => {
                error!(?err, "{error_message}");
                Ok(false)
            }
        }
    }

    async fn execute_command(&self, command: ServerCommand) -> Result<bool> {
        match command {
            ServerCommand::StartTeacher { mode, respond_to } => {
                let result = self.start_teacher_broadcast(mode).await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|err| format!("{:#}", err)),
                    );
                    if result.is_err() {
                        return Ok(false);
                    }
                }
                result?;
                Ok(false)
            }
            ServerCommand::StartStudent {
                student_id,
                respond_to,
            } => {
                let result = self.start_student_broadcast(&student_id).await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|err| format!("{:#}", err)),
                    );
                    if result.is_err() {
                        return Ok(false);
                    }
                }
                result?;
                Ok(false)
            }
            ServerCommand::StopBroadcast { respond_to } => {
                let result = self.stop_broadcast().await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|err| format!("{:#}", err)),
                    );
                    if result.is_err() {
                        return Ok(false);
                    }
                }
                result?;
                Ok(false)
            }
            ServerCommand::SendFile {
                path,
                auto_open_override,
                respond_to,
            } => {
                let result = self.send_file_to_all(path, auto_open_override).await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|err| format!("{:#}", err)),
                    );
                    if result.is_err() {
                        return Ok(false);
                    }
                }
                result?;
                Ok(false)
            }
            ServerCommand::AudioStart { respond_to } => {
                let result = self.audio.start().await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(
                        result
                            .as_ref()
                            .map(|_| ())
                            .map_err(|err| format!("{:#}", err)),
                    );
                    if result.is_err() {
                        return Ok(false);
                    }
                }
                result?;
                Ok(false)
            }
            ServerCommand::AudioStop { respond_to } => {
                self.audio.stop().await;
                if let Some(tx) = respond_to {
                    let _ = tx.send(Ok(()));
                }
                Ok(false)
            }
            ServerCommand::AudioForce { force, respond_to } => {
                self.audio.set_force_play(force);
                if let Some(tx) = respond_to {
                    let _ = tx.send(Ok(()));
                }
                Ok(false)
            }
            #[cfg(feature = "ui")]
            ServerCommand::ListStudents { respond_to } => {
                let list = self.state.list_students();
                let _ = respond_to.send(Ok(list));
                Ok(false)
            }
            #[cfg(feature = "ui")]
            ServerCommand::QueryStatus { respond_to } => {
                let status = self.status_snapshot();
                let _ = respond_to.send(status);
                Ok(false)
            }
            ServerCommand::Quit => Ok(true),
        }
    }

    #[cfg(feature = "ui")]
    fn status_snapshot(&self) -> ServerStatus {
        let students = self.state.list_students();
        ServerStatus {
            listen_addr: self.state.config().listen_addr(),
            broadcast_mode: self.state.broadcast_mode(),
            broadcast_source: self.state.broadcast_source(),
            audio_enabled: self.audio.is_running(),
            audio_forced: self.audio.is_force_play(),
            connected_students: students.len(),
        }
    }

    fn print_help(&self) {
        println!(
            "命令:\n  help                 显示帮助\n  students             列出在线学生\n  start [window]       开启教师屏幕广播\n  stop                 停止当前广播\n  spotlight <ID>       请求学生屏幕广播\n  send <路径> [open]   分发文件，可选参数 open 自动打开\n  audio <on|off|force|allow> 控制音频广播\n  quit                 退出程序"
        );
    }

    fn print_students(&self) {
        let entries = self.state.list_students();
        if entries.is_empty() {
            println!("暂无学生在线");
            return;
        }
        println!("在线学生:");
        for entry in entries {
            println!(
                "- {} ({}) @ {}",
                entry.display_name, entry.student_id, entry.addr
            );
        }
    }

    async fn start_teacher_broadcast(&self, mode: BroadcastMode) -> Result<()> {
        self.state
            .set_broadcast_source(Some(BroadcastSource::Teacher), mode);
        self.screen.start(mode).await?;
        self.state.broadcast_command(BroadcastCommand::Start {
            source: BroadcastSource::Teacher,
            mode,
        });
        info!(?mode, "教师屏幕广播启动");
        Ok(())
    }

    async fn start_student_broadcast(&self, student_id: &str) -> Result<()> {
        self.screen.stop().await;
        let student_name = self
            .state
            .find_student_name(student_id)
            .unwrap_or_else(|| student_id.to_string());
        let source = BroadcastSource::Student {
            student_id: student_id.to_string(),
            student_name: Some(student_name.clone()),
        };
        self.state
            .set_broadcast_source(Some(source.clone()), BroadcastMode::Fullscreen);
        self.state.broadcast_command(BroadcastCommand::Start {
            source,
            mode: BroadcastMode::Fullscreen,
        });
        info!(student_id, "已请求学生屏幕广播");
        Ok(())
    }

    async fn stop_broadcast(&self) -> Result<()> {
        self.screen.stop().await;
        self.state.set_broadcast_source(None, BroadcastMode::Window);
        self.state.broadcast_command(BroadcastCommand::Stop);
        info!("广播已停止");
        Ok(())
    }

    async fn send_file_to_all(&self, path: PathBuf, auto_open_override: bool) -> Result<()> {
        use tokio::fs::File;

        let metadata = tokio::fs::metadata(&path)
            .await
            .with_context(|| format!("无法读取文件信息: {}", path.display()))?;
        if !metadata.is_file() {
            bail!("{} 不是有效文件", path.display());
        }

        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or_else(|| anyhow!("无法解析文件名"))?;

        let transfer_id = Uuid::new_v4();
        let auto_open = auto_open_override || self.state.config.file_auto_open;

        self.state.broadcast(TeacherToStudent::FileOffer(FileOffer {
            transfer_id,
            file_name: file_name.clone(),
            total_size: metadata.len(),
            auto_open,
        }));

        let mut file = File::open(&path).await?;
        let mut buffer = vec![0u8; 64 * 1024];
        let mut offset = 0u64;
        loop {
            let read = file.read(&mut buffer).await?;
            if read == 0 {
                break;
            }
            self.state.broadcast(TeacherToStudent::FileChunk(FileChunk {
                transfer_id,
                offset,
                bytes: buffer[..read].to_vec(),
                final_chunk: offset + read as u64 >= metadata.len(),
            }));
            offset += read as u64;
        }

        self.state
            .broadcast(TeacherToStudent::FileComplete(FileTransferComplete {
                transfer_id,
                success: true,
                message: Some(format!("文件 {} 已发送", file_name)),
            }));

        info!(file = %file_name, size = metadata.len(), "文件分发完成");
        Ok(())
    }
}

async fn handle_student_connection(
    state: Arc<TeacherState>,
    screen: ScreenBroadcaster,
    stream: TcpStream,
    addr: SocketAddr,
) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    let greeting: StudentToTeacher = read_message(&mut reader).await?;
    let hello = match greeting {
        StudentToTeacher::Hello(payload) => payload,
        other => bail!("期望 Hello 消息, 收到 {:?}", other),
    };

    info!(student = %hello.student_id, %addr, "学生已连接");

    let (tx, mut rx) = mpsc::unbounded_channel::<TeacherToStudent>();
    let connection_id = Uuid::new_v4();
    let student_handle = Arc::new(StudentHandle::new(
        connection_id,
        addr,
        hello.student_id.clone(),
        hello.student_name.clone(),
        hello.capabilities.clone(),
        tx.clone(),
    ));

    state.add_student(student_handle.clone());

    let welcome = TeacherToStudent::Welcome(HelloAck {
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        force_fullscreen: matches!(state.broadcast_mode(), BroadcastMode::Fullscreen),
        broadcast_mode: state.broadcast_mode(),
    });
    student_handle.send(welcome);

    let writer_task = tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if let Err(err) = write_message(&mut writer, &message).await {
                error!(?err, "发送给学生失败");
                break;
            }
        }
    });

    let mut uploads: HashMap<Uuid, UploadSession> = HashMap::new();

    loop {
        let message = match read_message(&mut reader).await {
            Ok(msg) => msg,
            Err(err) => {
                warn!(?err, student = %hello.student_id, "学生连接断开");
                break;
            }
        };

        match message {
            StudentToTeacher::Hello(_) => {
                warn!(student = %hello.student_id, "收到重复 Hello");
            }
            StudentToTeacher::Heartbeat(_) => {
                student_handle.touch();
            }
            StudentToTeacher::Video(frame) => {
                if state.is_student_broadcasting(&hello.student_id) {
                    state.broadcast_except(
                        TeacherToStudent::Video(frame.clone()),
                        Some(connection_id),
                    );
                }
            }
            StudentToTeacher::Audio(frame) => {
                state.broadcast_except(TeacherToStudent::Audio(frame.clone()), Some(connection_id));
            }
            StudentToTeacher::FileOffer(offer) => {
                let path = state.prepare_upload_path(&hello, &offer.file_name).await?;
                let file = tokio::fs::File::create(&path)
                    .await
                    .with_context(|| format!("无法创建文件 {}", path.display()))?;
                uploads.insert(
                    offer.transfer_id,
                    UploadSession {
                        file,
                        path,
                        expected: offer.total_size,
                        received: 0,
                    },
                );
                info!(student = %hello.student_id, file = %offer.file_name, "准备接收文件");
            }
            StudentToTeacher::FileChunk(chunk) => {
                if let Some(session) = uploads.get_mut(&chunk.transfer_id) {
                    session
                        .file
                        .write_all(&chunk.bytes)
                        .await
                        .context("写入上传文件失败")?;
                    session.received += chunk.bytes.len() as u64;
                } else {
                    warn!(transfer = %chunk.transfer_id, "收到未知文件分片");
                }
            }
            StudentToTeacher::FileComplete(done) => {
                if let Some(mut session) = uploads.remove(&done.transfer_id) {
                    session.file.flush().await?;
                    if done.success {
                        info!(student = %hello.student_id, path = %session.path.display(), "学生上传完成");
                        let _ = tx.send(TeacherToStudent::FileComplete(FileTransferComplete {
                            transfer_id: done.transfer_id,
                            success: true,
                            message: Some("文件上传完成".into()),
                        }));
                    } else {
                        warn!(student = %hello.student_id, "学生上传失败");
                    }
                }
            }
            StudentToTeacher::Ack(_) => {}
            StudentToTeacher::Error(msg) => {
                warn!(student = %hello.student_id, %msg, "学生报告错误");
            }
        }
    }

    state.remove_student(connection_id);
    writer_task.abort();
    screen.handle_disconnection(&hello.student_id);
    info!(student = %hello.student_id, "学生已断开");
    Ok(())
}

struct UploadSession {
    file: tokio::fs::File,
    path: PathBuf,
    #[allow(dead_code)]
    expected: u64,
    received: u64,
}

pub struct TeacherState {
    config: Arc<TeacherConfig>,
    students: Arc<RwLock<HashMap<Uuid, Arc<StudentHandle>>>>,
    broadcast_source: Arc<RwLock<Option<BroadcastSource>>>,
    broadcast_mode: Arc<RwLock<BroadcastMode>>,
    frame_counter: AtomicU64,
}

impl TeacherState {
    fn new(config: Arc<TeacherConfig>) -> Self {
        Self {
            config,
            students: Arc::new(RwLock::new(HashMap::new())),
            broadcast_source: Arc::new(RwLock::new(None)),
            broadcast_mode: Arc::new(RwLock::new(BroadcastMode::Window)),
            frame_counter: AtomicU64::new(0),
        }
    }

    #[cfg(feature = "ui")]
    pub fn config(&self) -> Arc<TeacherConfig> {
        Arc::clone(&self.config)
    }

    pub(crate) fn broadcast_config(&self) -> BroadcastConfig {
        self.config.broadcast.clone()
    }

    pub(crate) fn next_frame_id(&self) -> u64 {
        self.frame_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn add_student(&self, student: Arc<StudentHandle>) {
        self.students.write().insert(student.connection_id, student);
    }

    fn remove_student(&self, connection_id: Uuid) {
        self.students.write().remove(&connection_id);
    }

    fn list_students(&self) -> Vec<StudentSummary> {
        self.students
            .read()
            .values()
            .map(|student| StudentSummary {
                student_id: student.student_id.clone(),
                display_name: student.student_name.clone(),
                addr: student.addr,
            })
            .collect()
    }

    fn broadcast(&self, message: TeacherToStudent) {
        self.broadcast_except(message, None);
    }

    fn broadcast_except(&self, message: TeacherToStudent, exclude: Option<Uuid>) {
        let recipients: Vec<Arc<StudentHandle>> = self
            .students
            .read()
            .iter()
            .filter(|(id, _)| exclude.map_or(true, |ex| ex != **id))
            .map(|(_, handle)| Arc::clone(handle))
            .collect();

        for student in recipients {
            student.send(message.clone());
        }
    }

    fn broadcast_command(&self, command: BroadcastCommand) {
        self.broadcast(TeacherToStudent::Broadcast(command));
    }

    pub(crate) fn broadcast_video(&self, frame: VideoFrame) {
        self.broadcast(TeacherToStudent::Video(frame));
    }

    pub(crate) fn broadcast_audio(&self, frame: AudioFrame) {
        self.broadcast(TeacherToStudent::Audio(frame));
    }

    fn set_broadcast_source(&self, source: Option<BroadcastSource>, mode: BroadcastMode) {
        *self.broadcast_source.write() = source;
        *self.broadcast_mode.write() = mode;
    }

    #[cfg(feature = "ui")]
    pub fn broadcast_source(&self) -> Option<BroadcastSource> {
        self.broadcast_source.read().clone()
    }

    pub fn broadcast_mode(&self) -> BroadcastMode {
        *self.broadcast_mode.read()
    }

    fn is_student_broadcasting(&self, student_id: &str) -> bool {
        matches!(
            &*self.broadcast_source.read(),
            Some(BroadcastSource::Student { student_id: sid, .. }) if sid == student_id
        )
    }

    async fn prepare_upload_path(&self, hello: &HelloMessage, file_name: &str) -> Result<PathBuf> {
        let student_dir = self
            .config
            .save_upload_dir
            .join(sanitize_filename(&hello.student_id));
        tokio::fs::create_dir_all(&student_dir).await?;
        let safe_name = sanitize_filename(file_name);
        Ok(student_dir.join(safe_name))
    }

    fn find_student_name(&self, student_id: &str) -> Option<String> {
        self.students
            .read()
            .values()
            .find(|student| student.student_id == student_id)
            .map(|student| student.student_name.clone())
            .or_else(|| {
                self.config
                    .expected_students
                    .iter()
                    .find(|s| s.student_id == student_id)
                    .and_then(|s| s.student_name.clone())
            })
    }

    fn disconnect_all(&self) {
        self.students.write().clear();
    }
}

struct StudentHandle {
    connection_id: Uuid,
    addr: SocketAddr,
    student_id: String,
    student_name: String,
    #[allow(dead_code)]
    capabilities: StudentCapabilities,
    sender: mpsc::UnboundedSender<TeacherToStudent>,
    last_seen: Mutex<Instant>,
}

impl StudentHandle {
    fn new(
        connection_id: Uuid,
        addr: SocketAddr,
        student_id: String,
        student_name: String,
        capabilities: StudentCapabilities,
        sender: mpsc::UnboundedSender<TeacherToStudent>,
    ) -> Self {
        Self {
            connection_id,
            addr,
            student_id,
            student_name,
            capabilities,
            sender,
            last_seen: Mutex::new(Instant::now()),
        }
    }

    fn send(&self, message: TeacherToStudent) {
        if let Err(err) = self.sender.send(message) {
            warn!(student = %self.student_id, ?err, "发送消息失败");
        }
    }

    fn touch(&self) {
        *self.last_seen.lock() = Instant::now();
    }
}

#[derive(Debug, Clone)]
pub struct StudentSummary {
    pub student_id: String,
    pub display_name: String,
    pub addr: SocketAddr,
}

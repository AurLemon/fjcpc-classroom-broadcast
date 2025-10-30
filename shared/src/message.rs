use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Broadcast display mode requested by the teacher.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BroadcastMode {
    Fullscreen,
    Window,
}

impl Default for BroadcastMode {
    fn default() -> Self {
        BroadcastMode::Fullscreen
    }
}

/// Identifies the source of a broadcast feed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum BroadcastSource {
    Teacher,
    Student {
        student_id: String,
        student_name: Option<String>,
    },
}

/// Supported codecs for video transport.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    /// JPEG encoded frame; clients should decode via libjpeg-compatible decoder.
    Jpeg,
    /// Raw BGRA pixels (mainly for diagnostics / testing).
    Bgra,
}

/// Video frame transported from teacher to student (or reverse for student spotlight).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoFrame {
    pub frame_id: u64,
    pub timestamp_ms: u64,
    pub source: BroadcastSource,
    pub codec: VideoCodec,
    pub width: u32,
    pub height: u32,
    pub fullscreen: bool,
    pub data: Vec<u8>,
}

/// Audio frame chunk transmitted alongside video.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioFrame {
    pub frame_id: u64,
    pub timestamp_ms: u64,
    pub sample_rate: u32,
    pub channels: u8,
    pub force_play: bool,
    pub data: Vec<u8>,
}

/// Metadata describing a file that will be sent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileOffer {
    pub transfer_id: Uuid,
    pub file_name: String,
    pub total_size: u64,
    pub auto_open: bool,
}

/// Data chunk for an ongoing file transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileChunk {
    pub transfer_id: Uuid,
    pub offset: u64,
    pub bytes: Vec<u8>,
    pub final_chunk: bool,
}

/// Completion notification for a file transfer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileTransferComplete {
    pub transfer_id: Uuid,
    pub success: bool,
    pub message: Option<String>,
}

/// Initial message sent by a student when connecting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    pub student_id: String,
    pub student_name: String,
    pub client_version: String,
    #[serde(default)]
    pub capabilities: StudentCapabilities,
}

/// Acknowledgement from teacher after successful registration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloAck {
    pub server_version: String,
    pub force_fullscreen: bool,
    pub broadcast_mode: BroadcastMode,
}

/// Reported capabilities of a student client.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct StudentCapabilities {
    #[serde(default)]
    pub receive_video: bool,
    #[serde(default)]
    pub send_video: bool,
    #[serde(default)]
    pub receive_audio: bool,
    #[serde(default)]
    pub send_audio: bool,
    #[serde(default)]
    pub file_transfer: bool,
}

/// Periodic heartbeat between peers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub timestamp_ms: u64,
}

/// Command to start/stop broadcasts or switch source.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum BroadcastCommand {
    Start {
        source: BroadcastSource,
        mode: BroadcastMode,
    },
    Stop,
    RequestStudentShare {
        student_id: String,
    },
}

/// Messages sent from teacher to student.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum TeacherToStudent {
    Welcome(HelloAck),
    Broadcast(BroadcastCommand),
    Video(VideoFrame),
    Audio(AudioFrame),
    FileOffer(FileOffer),
    FileChunk(FileChunk),
    FileComplete(FileTransferComplete),
    Heartbeat(Heartbeat),
    Error(String),
}

/// Messages sent from student to teacher.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum StudentToTeacher {
    Hello(HelloMessage),
    Heartbeat(Heartbeat),
    Ack(String),
    Video(VideoFrame),
    Audio(AudioFrame),
    FileOffer(FileOffer),
    FileChunk(FileChunk),
    FileComplete(FileTransferComplete),
    Error(String),
}

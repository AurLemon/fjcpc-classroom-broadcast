use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Configuration for the JPEG based screen broadcast pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BroadcastConfig {
    /// Target frames per second for screen capture.
    pub fps: u32,
    /// JPEG quality (1-100) when encoding captured frames.
    pub jpeg_quality: u8,
    /// Optional maximum width for captured frames. When set, frames will be scaled down.
    pub max_width: Option<u32>,
    /// Optional maximum height for captured frames. When set, frames will be scaled down.
    pub max_height: Option<u32>,
}

impl Default for BroadcastConfig {
    fn default() -> Self {
        Self {
            fps: 12,
            jpeg_quality: 75,
            max_width: None,
            max_height: None,
        }
    }
}

/// Configuration loaded by the teacher binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TeacherConfig {
    /// Host/interface to bind for incoming student connections.
    pub listen_host: String,
    /// TCP port to bind for student connections.
    pub listen_port: u16,
    /// Whether audio streaming starts enabled.
    pub enable_audio_by_default: bool,
    /// Whether students should be forced out of mute when audio starts.
    pub force_audio: bool,
    /// Directory where uploaded files from students will be stored.
    pub save_upload_dir: PathBuf,
    /// Whether distributed files should request auto open on student side by default.
    pub file_auto_open: bool,
    /// Screen broadcast quality parameters.
    pub broadcast: BroadcastConfig,
    /// Optional list of expected students used for display purposes.
    pub expected_students: Vec<StudentRegistration>,
    /// Interval (seconds) at which teacher expects heartbeat pings.
    pub heartbeat_interval_secs: u64,
    /// Idle timeout (seconds) before a connection is considered dead.
    pub idle_timeout_secs: u64,
}

impl TeacherConfig {
    /// Load configuration from a TOML file.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create teacher config directory {}", parent.display())
                })?;
            }
        }
        let content = fs::read_to_string(path_ref).with_context(|| {
            format!("Failed to read teacher config from {}", path_ref.display())
        })?;
        let mut config: Self = toml::from_str(&content).with_context(|| {
            format!("Failed to parse teacher config {}", path_ref.display())
        })?;
        config.finalize(path_ref)?;
        Ok(config)
    }

    fn finalize(&mut self, path: &Path) -> Result<()> {
        self.broadcast.fps = self.broadcast.fps.clamp(1, 60);
        self.broadcast.jpeg_quality = self.broadcast.jpeg_quality.clamp(1, 100);

        if self.save_upload_dir.is_relative() {
            let base = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            self.save_upload_dir = base.join(&self.save_upload_dir);
        }

        fs::create_dir_all(&self.save_upload_dir).with_context(|| {
            format!(
                "Failed to create upload directory {}",
                self.save_upload_dir.display()
            )
        })?;

        Ok(())
    }

    /// Return the socket address string used for binding (`host:port`).
    pub fn listen_addr(&self) -> String {
        format!("{}:{}", self.listen_host, self.listen_port)
    }
}

impl Default for TeacherConfig {
    fn default() -> Self {
        Self {
            listen_host: "0.0.0.0".to_string(),
            listen_port: 5000,
            enable_audio_by_default: false,
            force_audio: false,
            save_upload_dir: PathBuf::from("uploads"),
            file_auto_open: false,
            broadcast: BroadcastConfig::default(),
            expected_students: Vec::new(),
            heartbeat_interval_secs: 10,
            idle_timeout_secs: 30,
        }
    }
}

/// Information about a student expected to join the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StudentRegistration {
    pub student_id: String,
    pub student_name: Option<String>,
    pub seat: Option<String>,
    pub allow_uploads: bool,
}

impl Default for StudentRegistration {
    fn default() -> Self {
        Self {
            student_id: String::new(),
            student_name: None,
            seat: None,
            allow_uploads: true,
        }
    }
}

/// Configuration loaded by the student binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StudentConfig {
    pub teacher_ip: String,
    pub teacher_port: u16,
    pub student_name: String,
    pub student_id: String,
    pub auto_fullscreen: bool,
    pub download_path: PathBuf,
    pub auto_open_file: bool,
    /// When true, teacher directives can override the `auto_fullscreen` flag.
    pub allow_forced_fullscreen: bool,
}

impl StudentConfig {
    /// Load configuration from a JSON file.
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path_ref = path.as_ref();
        if let Some(parent) = path_ref.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent).with_context(|| {
                    format!("Failed to create student config directory {}", parent.display())
                })?;
            }
        }
        let content = fs::read_to_string(path_ref).with_context(|| {
            format!("Failed to read student config from {}", path_ref.display())
        })?;
        let mut config: Self = serde_json::from_str(&content).with_context(|| {
            format!("Failed to parse student config {}", path_ref.display())
        })?;
        config.finalize(path_ref)?;
        Ok(config)
    }

    fn finalize(&mut self, path: &Path) -> Result<()> {
        if self.download_path.is_relative() {
            let base = path
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| PathBuf::from("."));
            self.download_path = base.join(&self.download_path);
        }

        fs::create_dir_all(&self.download_path).with_context(|| {
            format!(
                "Failed to create download directory {}",
                self.download_path.display()
            )
        })?;

        Ok(())
    }

    /// Return the address of the teacher (`host:port`).
    pub fn teacher_addr(&self) -> String {
        format!("{}:{}", self.teacher_ip, self.teacher_port)
    }
}

impl Default for StudentConfig {
    fn default() -> Self {
        Self {
            teacher_ip: "127.0.0.1".to_string(),
            teacher_port: 5000,
            student_name: "Student".to_string(),
            student_id: "S00".to_string(),
            auto_fullscreen: true,
            download_path: PathBuf::from("downloads"),
            auto_open_file: false,
            allow_forced_fullscreen: true,
        }
    }
}

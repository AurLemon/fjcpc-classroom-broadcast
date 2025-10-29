//! Shared types and utilities used by both teacher and student binaries.

pub mod config;
pub mod logging;
pub mod message;
pub mod net;
pub mod util;

pub mod prelude {
    //! Common imports that are frequently used across binaries.
    pub use crate::config::{BroadcastConfig, StudentConfig, StudentRegistration, TeacherConfig};
    pub use crate::logging::init_tracing;
    pub use crate::message::{
        AudioFrame, BroadcastCommand, BroadcastMode, BroadcastSource, FileChunk, FileOffer,
        FileTransferComplete, Heartbeat, HelloAck, HelloMessage, StudentCapabilities,
        StudentToTeacher, TeacherToStudent, VideoCodec, VideoFrame,
    };
    pub use crate::net::{read_message, write_message, FramedStream};
    pub use crate::util::sanitize_filename;
}

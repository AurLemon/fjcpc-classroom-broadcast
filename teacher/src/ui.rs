#![cfg(feature = "ui")]

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use native_windows_gui as nwg;
use nwg::CheckBoxState;
use tokio::sync::oneshot;
use tracing::error;

use shared::prelude::{BroadcastMode, BroadcastSource};

use crate::server::{CommandSender, ServerCommand, ServerStatus, StudentSummary};

pub struct UiContext {
    command_tx: CommandSender,
    config_path: PathBuf,
}

impl UiContext {
    pub fn new(command_tx: CommandSender, config_path: PathBuf) -> Self {
        Self {
            command_tx,
            config_path,
        }
    }
}

pub fn run(context: UiContext) -> Result<()> {
    nwg::init()?;

    let app = Rc::new(RefCell::new(ControlPanel::new(context)));
    ControlPanel::build_ui(&app)?;
    {
        let mut panel = app.borrow_mut();
        panel.refresh_all().log_error("failed to refresh panel");
    }

    nwg::dispatch_thread_events();

    if let Some(handler) = app.borrow_mut().handler.take() {
        nwg::unbind_event_handler(&handler);
    }

    Ok(())
}

struct ControlPanel {
    ctx: UiContext,
    handler: Option<nwg::EventHandler>,
    students: Vec<StudentSummary>,

    window: nwg::Window,
    status_label: nwg::Label,
    listen_label: nwg::Label,
    config_label: nwg::Label,
    student_list: nwg::ListBox<String>,
    start_full_btn: nwg::Button,
    start_window_btn: nwg::Button,
    start_student_btn: nwg::Button,
    stop_broadcast_btn: nwg::Button,
    audio_on_btn: nwg::Button,
    audio_off_btn: nwg::Button,
    audio_force_btn: nwg::Button,
    audio_allow_btn: nwg::Button,
    send_file_btn: nwg::Button,
    refresh_btn: nwg::Button,
    auto_open_checkbox: nwg::CheckBox,
    timer: nwg::AnimationTimer,
}

impl ControlPanel {
    fn new(ctx: UiContext) -> Self {
        Self {
            ctx,
            handler: None,
            students: Vec::new(),
            window: nwg::Window::default(),
            status_label: nwg::Label::default(),
            listen_label: nwg::Label::default(),
            config_label: nwg::Label::default(),
            student_list: nwg::ListBox::default(),
            start_full_btn: nwg::Button::default(),
            start_window_btn: nwg::Button::default(),
            start_student_btn: nwg::Button::default(),
            stop_broadcast_btn: nwg::Button::default(),
            audio_on_btn: nwg::Button::default(),
            audio_off_btn: nwg::Button::default(),
            audio_force_btn: nwg::Button::default(),
            audio_allow_btn: nwg::Button::default(),
            send_file_btn: nwg::Button::default(),
            refresh_btn: nwg::Button::default(),
            auto_open_checkbox: nwg::CheckBox::default(),
            timer: nwg::AnimationTimer::default(),
        }
    }

    fn build_ui(app: &Rc<RefCell<Self>>) -> Result<()> {
        let mut panel = app.borrow_mut();

        nwg::Window::builder()
            .size((720, 460))
            .position((300, 160))
            .title("Classroom Broadcast - Teacher Control")
            .build(&mut panel.window)?;

        nwg::Label::builder()
            .parent(&panel.window)
            .text("Status: Idle")
            .position((20, 20))
            .size((680, 24))
            .build(&mut panel.status_label)?;

        nwg::Label::builder()
            .parent(&panel.window)
            .text("Listening on: --")
            .position((20, 380))
            .size((680, 24))
            .build(&mut panel.listen_label)?;

        nwg::Label::builder()
            .parent(&panel.window)
            .text("Config file:")
            .position((20, 410))
            .size((680, 24))
            .build(&mut panel.config_label)?;
        panel
            .config_label
            .set_text(&format!("Config file: {}", panel.ctx.config_path.display()));

        nwg::ListBox::builder()
            .parent(&panel.window)
            .position((20, 60))
            .size((340, 300))
            .build(&mut panel.student_list)?;

        panel.build_buttons()?;

        nwg::AnimationTimer::builder()
            .parent(&panel.window)
            .interval(Duration::from_millis(2000))
            .build(&mut panel.timer)?;

        let app_rc = Rc::clone(app);
        let handler = nwg::full_bind_event_handler(&panel.window.handle, move |evt, _, handle| {
            let mut panel = app_rc.borrow_mut();
            match evt {
                nwg::Event::OnButtonClick => {
                    panel.handle_button(handle);
                }
                nwg::Event::OnWindowClose => {
                    let _ = panel.ctx.command_tx.send(ServerCommand::Quit);
                    nwg::stop_thread_dispatch();
                }
                nwg::Event::OnTimerTick => {
                    if handle == panel.timer.handle {
                        panel.refresh_all().log_error("auto refresh failed");
                    }
                }
                _ => {}
            }
        });
        panel.handler = Some(handler);

        Ok(())
    }

    fn build_buttons(&mut self) -> Result<()> {
        let x = 380;
        let mut y = 60;
        let width = 300;
        let height = 32;
        let gap = 8;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Start Teacher (Fullscreen)")
            .position((x, y))
            .size((width, height))
            .build(&mut self.start_full_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Start Teacher (Window)")
            .position((x, y))
            .size((width, height))
            .build(&mut self.start_window_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Broadcast Selected Student")
            .position((x, y))
            .size((width, height))
            .build(&mut self.start_student_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Stop Broadcast")
            .position((x, y))
            .size((width, height))
            .build(&mut self.stop_broadcast_btn)?;
        y += height + gap * 2;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Start Audio")
            .position((x, y))
            .size((width, height))
            .build(&mut self.audio_on_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Stop Audio")
            .position((x, y))
            .size((width, height))
            .build(&mut self.audio_off_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Force Student Audio")
            .position((x, y))
            .size((width, height))
            .build(&mut self.audio_force_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Allow Student Mute")
            .position((x, y))
            .size((width, height))
            .build(&mut self.audio_allow_btn)?;
        y += height + gap * 2;

        nwg::CheckBox::builder()
            .parent(&self.window)
            .text("Request auto-open on student")
            .position((x, y))
            .size((width, height))
            .build(&mut self.auto_open_checkbox)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Send File to Students...")
            .position((x, y))
            .size((width, height))
            .build(&mut self.send_file_btn)?;
        y += height + gap;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Refresh Status")
            .position((x, y))
            .size((width, height))
            .build(&mut self.refresh_btn)?;

        Ok(())
    }

    fn handle_button(&mut self, handle: nwg::ControlHandle) {
        if handle == self.start_full_btn.handle {
            self.start_teacher(BroadcastMode::Fullscreen);
        } else if handle == self.start_window_btn.handle {
            self.start_teacher(BroadcastMode::Window);
        } else if handle == self.start_student_btn.handle {
            self.start_student();
        } else if handle == self.stop_broadcast_btn.handle {
            self.stop_broadcast();
        } else if handle == self.audio_on_btn.handle {
            self.audio_on();
        } else if handle == self.audio_off_btn.handle {
            self.audio_off();
        } else if handle == self.audio_force_btn.handle {
            self.audio_force(true);
        } else if handle == self.audio_allow_btn.handle {
            self.audio_force(false);
        } else if handle == self.send_file_btn.handle {
            self.send_file();
        } else if handle == self.refresh_btn.handle {
            self.refresh_all().log_error("manual refresh failed");
        }
    }

    fn start_teacher(&mut self, mode: BroadcastMode) {
        let (tx, rx) = oneshot::channel();
        let command = ServerCommand::StartTeacher {
            mode,
            respond_to: Some(tx),
        };
        if self.ctx.command_tx.send(command).is_err() {
            self.alert("Teacher service is not running.");
            return;
        }
        match Self::recv_ack(rx, "操作超时") {
            Ok(()) => self.refresh_status().log_error("refresh status failed"),
            Err(err) => self.alert(&format!("{:#}", err)),
        }
    }

    fn stop_broadcast(&mut self) {
        let (tx, rx) = oneshot::channel();
        if self
            .ctx
            .command_tx
            .send(ServerCommand::StopBroadcast {
                respond_to: Some(tx),
            })
            .is_err()
        {
            self.alert("Teacher service is not running.");
            return;
        }
        match Self::recv_ack(rx, "操作超时") {
            Ok(()) => self.refresh_status().log_error("refresh status failed"),
            Err(err) => self.alert(&format!("{:#}", err)),
        }
    }

    fn start_student(&mut self) {
        if let Some(index) = self.student_list.selection() {
            if let Some(student) = self.students.get(index as usize) {
                let (tx, rx) = oneshot::channel();
                if self
                    .ctx
                    .command_tx
                    .send(ServerCommand::StartStudent {
                        student_id: student.student_id.clone(),
                        respond_to: Some(tx),
                    })
                    .is_err()
                {
                    self.alert("Teacher service is not running.");
                    return;
                }
                match Self::recv_ack(rx, "操作超时") {
                    Ok(()) => self.refresh_status().log_error("refresh status failed"),
                    Err(err) => self.alert(&format!("{:#}", err)),
                }
            }
        } else {
            self.alert("Select a student in the list first.");
        }
    }

    fn audio_on(&mut self) {
        let (tx, rx) = oneshot::channel();
        if self
            .ctx
            .command_tx
            .send(ServerCommand::AudioStart {
                respond_to: Some(tx),
            })
            .is_err()
        {
            self.alert("Teacher service is not running.");
            return;
        }
        match Self::recv_ack(rx, "操作超时") {
            Ok(()) => self.refresh_status().log_error("refresh status failed"),
            Err(err) => self.alert(&format!("{:#}", err)),
        }
    }

    fn audio_off(&mut self) {
        let (tx, rx) = oneshot::channel();
        if self
            .ctx
            .command_tx
            .send(ServerCommand::AudioStop {
                respond_to: Some(tx),
            })
            .is_err()
        {
            self.alert("Teacher service is not running.");
            return;
        }
        if let Err(err) = Self::recv_ack(rx, "操作超时") {
            self.alert(&format!("{:#}", err));
        } else {
            self.refresh_status().log_error("refresh status failed");
        }
    }

    fn audio_force(&mut self, force: bool) {
        let (tx, rx) = oneshot::channel();
        if self
            .ctx
            .command_tx
            .send(ServerCommand::AudioForce {
                force,
                respond_to: Some(tx),
            })
            .is_err()
        {
            self.alert("Teacher service is not running.");
            return;
        }
        if let Err(err) = Self::recv_ack(rx, "操作超时") {
            self.alert(&format!("{:#}", err));
        } else {
            self.refresh_status().log_error("refresh status failed");
        }
    }

    fn send_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Choose a file to broadcast")
            .pick_file()
        {
            let (tx, rx) = oneshot::channel();
            let auto_open = self.auto_open_checkbox.check_state() == CheckBoxState::Checked;
            if self
                .ctx
                .command_tx
                .send(ServerCommand::SendFile {
                    path: path.into(),
                    auto_open_override: auto_open,
                    respond_to: Some(tx),
                })
                .is_err()
            {
                self.alert("Teacher service is not running.");
                return;
            }
            if let Err(err) = Self::recv_ack(rx, "文件分发结果未知") {
                self.alert(&format!("{:#}", err));
            }
        }
    }

    fn refresh_all(&mut self) -> Result<()> {
        self.refresh_students()?;
        self.refresh_status()?;
        Ok(())
    }

    fn refresh_students(&mut self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.ctx
            .command_tx
            .send(ServerCommand::ListStudents { respond_to: tx })
            .map_err(|_| anyhow!("Teacher service is not running"))?;
        let list = Self::recv_list(rx, "学生列表请求超时")?;
        self.students = list;
        self.student_list.clear();
        for student in &self.students {
            let display = format!(
                "{} ({}) @ {}",
                student.display_name, student.student_id, student.addr
            );
            self.student_list.push(display);
        }
        Ok(())
    }

    fn refresh_status(&mut self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.ctx
            .command_tx
            .send(ServerCommand::QueryStatus { respond_to: tx })
            .map_err(|_| anyhow!("Teacher service is not running"))?;
        let status = Self::recv_status(rx, "状态请求超时")?;
        self.update_status(status);
        Ok(())
    }

    fn update_status(&mut self, status: ServerStatus) {
        let source_text = match status.broadcast_source {
            Some(BroadcastSource::Teacher) => "Teacher screen".to_string(),
            Some(BroadcastSource::Student {
                student_id,
                student_name,
            }) => {
                let name = student_name.unwrap_or_else(|| student_id.clone());
                format!("Student {}", name)
            }
            None => "Idle".to_string(),
        };

        let mode_text = match status.broadcast_mode {
            BroadcastMode::Fullscreen => "Fullscreen mode",
            BroadcastMode::Window => "Window mode",
        };

        let audio_text = if status.audio_enabled {
            if status.audio_forced {
                "Audio: forced"
            } else {
                "Audio: on"
            }
        } else {
            "Audio: off"
        };

        self.status_label.set_text(&format!(
            "Current: {} | {} | Students: {} | {}",
            source_text, mode_text, status.connected_students, audio_text
        ));
        self.listen_label
            .set_text(&format!("Listening on: {}", status.listen_addr));
    }

    fn recv_ack(rx: oneshot::Receiver<Result<(), String>>, timeout_message: &str) -> Result<()> {
        match rx.blocking_recv() {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => Err(anyhow!(err)),
            Err(_) => Err(anyhow!(timeout_message.to_string())),
        }
    }

    fn recv_list(
        rx: oneshot::Receiver<Result<Vec<StudentSummary>, String>>,
        timeout_message: &str,
    ) -> Result<Vec<StudentSummary>> {
        match rx.blocking_recv() {
            Ok(Ok(list)) => Ok(list),
            Ok(Err(err)) => Err(anyhow!(err)),
            Err(_) => Err(anyhow!(timeout_message.to_string())),
        }
    }

    fn recv_status(
        rx: oneshot::Receiver<ServerStatus>,
        timeout_message: &str,
    ) -> Result<ServerStatus> {
        rx.blocking_recv()
            .map_err(|_| anyhow!(timeout_message.to_string()))
    }

    fn alert(&self, message: &str) {
        nwg::simple_message("Attention", message);
    }
}

trait ResultExt {
    fn log_error(self, context: &str);
}

impl<T, E> ResultExt for Result<T, E>
where
    E: std::fmt::Debug,
{
    fn log_error(self, context: &str) {
        if let Err(err) = self {
            error!(?err, "{context}");
        }
    }
}

use std::path::PathBuf;
use iced::{Element, Task, Subscription, Length};
use iced::widget::container;

use crate::db::{Db, Project, AgentTemplate};
use crate::agents::AgentPool;
use crate::ui::terminal_widget::TerminalWidget;

pub struct App {
    pub db: Db,
    pub pool: AgentPool,
    pub projects: Vec<Project>,
    pub templates: Vec<AgentTemplate>,
    pub sidebar_expanded_project: Option<String>,
    pub terminal_cols: u16,
    pub terminal_rows: u16,
}

#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),
    PasteFromClipboard,
    PasteText(String),
    PickDirectory,
    DirectoryPicked(Option<PathBuf>),
    ToggleTemplateMenu(String),
    SpawnAgent { project_id: String, template_id: String },
    FocusAgent(String),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let db = Db::open().expect("failed to open database");
        let projects = db.list_projects();
        let templates = db.list_templates();
        let app = App {
            db,
            pool: AgentPool::new(),
            projects,
            templates,
            sidebar_expanded_project: None,
            terminal_cols: 80,
            terminal_rows: 24,
        };
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PtyTick => {
                self.pool.tick_all();
            }
            Message::KeyInput(bytes) => {
                if let Some(agent) = self.pool.focused_mut() {
                    let _ = agent.pty.write_bytes(&bytes);
                }
            }
            Message::WindowResized(width, height) => {
                let cw = TerminalWidget::CHAR_WIDTH;
                let ch = TerminalWidget::CHAR_HEIGHT;
                let workspace_w = (width as f32 - 250.0).max(0.0);
                let cols = (workspace_w / cw).floor() as u16;
                let rows = (height as f32 / ch).floor() as u16;
                if cols > 0 && rows > 0 {
                    self.terminal_cols = cols;
                    self.terminal_rows = rows;
                    self.pool.resize_all(cols, rows);
                }
            }
            Message::CopyToClipboard(text) => {
                return iced::clipboard::write(text).map(|_: ()| Message::PtyTick);
            }
            Message::PasteFromClipboard => {
                return iced::clipboard::read()
                    .map(|opt| Message::PasteText(opt.unwrap_or_default()));
            }
            Message::PasteText(text) => {
                if let Some(agent) = self.pool.focused_mut() {
                    let _ = agent.pty.write_bytes(text.as_bytes());
                }
            }
            Message::PickDirectory => {
                return Task::future(async {
                    let folder = rfd::AsyncFileDialog::new().pick_folder().await;
                    Message::DirectoryPicked(folder.map(|f| f.path().to_path_buf()))
                });
            }
            Message::DirectoryPicked(Some(path)) => {
                let name = path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());
                let project = self.db.insert_project(&path, &name);
                if !self.projects.iter().any(|p| p.id == project.id) {
                    self.projects.push(project);
                }
            }
            Message::DirectoryPicked(None) => {}
            Message::ToggleTemplateMenu(project_id) => {
                if self.sidebar_expanded_project.as_deref() == Some(&project_id) {
                    self.sidebar_expanded_project = None;
                } else {
                    self.sidebar_expanded_project = Some(project_id);
                }
            }
            Message::SpawnAgent { project_id, template_id } => {
                if let (Some(project), Some(template)) = (
                    self.projects.iter().find(|p| p.id == project_id).cloned(),
                    self.templates.iter().find(|t| t.id == template_id).cloned(),
                ) {
                    self.db.insert_instance(&project_id, &template_id);
                    self.pool.spawn(&project, &template, self.terminal_cols, self.terminal_rows);
                    self.sidebar_expanded_project = None;
                }
            }
            Message::FocusAgent(agent_id) => {
                self.pool.focus(&agent_id);
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        // Sidebar is wired in Task 6 after sidebar.rs is created
        match self.pool.focused() {
            Some(agent) => TerminalWidget::new(&agent.emulator.screen).into(),
            None => container(iced::widget::text("Sélectionne un agent").size(14))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(iced::alignment::Horizontal::Center)
                .align_y(iced::alignment::Vertical::Center)
                .into(),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        use std::time::Duration;

        let poll = iced::time::every(Duration::from_millis(8))
            .map(|_| Message::PtyTick);

        let resize = iced::event::listen_with(|event, _status, _id| {
            match event {
                iced::Event::Window(iced::window::Event::Resized(size)) => {
                    Some(Message::WindowResized(size.width as u32, size.height as u32))
                }
                _ => None,
            }
        });

        Subscription::batch([poll, resize])
    }
}

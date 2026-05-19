use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use iced::{Element, Task, Subscription};

use crate::terminal::{PtyHandle, TerminalEmulator, spawn_pty};
use crate::ui::terminal_widget::TerminalWidget;

const COLS: usize = 80;
const ROWS: usize = 24;

pub struct App {
    pub emulator: TerminalEmulator,
    pub pty: PtyHandle,
    pub pty_rx: Arc<Mutex<mpsc::Receiver<Vec<u8>>>>,
}

#[derive(Debug, Clone)]
pub enum Message {
    PtyTick,
    KeyInput(Vec<u8>),
    WindowResized(u32, u32),
    CopyToClipboard(String),
    PasteFromClipboard,
    PasteText(String),
}

impl App {
    pub fn new() -> (Self, Task<Message>) {
        let cwd = std::env::current_dir().unwrap_or_default();
        #[cfg(windows)]
        let default_cmd = "cmd.exe".to_string();
        #[cfg(not(windows))]
        let default_cmd = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let (pty, rx) = spawn_pty(COLS as u16, ROWS as u16, &cwd, &default_cmd, &[]);
        let app = App {
            emulator: TerminalEmulator::new(COLS, ROWS),
            pty,
            pty_rx: Arc::new(Mutex::new(rx)),
        };
        (app, Task::none())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::PtyTick => {
                while let Ok(bytes) = self.pty_rx.lock().unwrap().try_recv() {
                    self.emulator.process(&bytes);
                }
            }
            Message::KeyInput(bytes) => {
                let _ = self.pty.write_bytes(&bytes);
            }
            Message::WindowResized(width, height) => {
                let char_w = TerminalWidget::CHAR_WIDTH;
                let char_h = TerminalWidget::CHAR_HEIGHT;
                let cols = (width as f32 / char_w).floor() as usize;
                let rows = (height as f32 / char_h).floor() as usize;
                if cols > 0 && rows > 0 {
                    self.emulator.resize(cols, rows);
                    self.pty.resize(cols as u16, rows as u16);
                }
            }
            Message::CopyToClipboard(text) => {
                return iced::clipboard::write(text).map(|_: ()| Message::PtyTick);
            }
            Message::PasteFromClipboard => {
                return iced::clipboard::read()
                    .map(|opt| {
                        Message::PasteText(opt.unwrap_or_default())
                    });
            }
            Message::PasteText(text) => {
                let _ = self.pty.write_bytes(text.as_bytes());
            }
        }
        Task::none()
    }

    pub fn view(&self) -> Element<'_, Message> {
        TerminalWidget::new(&self.emulator.screen).into()
    }

    pub fn subscription(&self) -> Subscription<Message> {
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

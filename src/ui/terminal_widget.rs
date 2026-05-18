use iced::{
    Element, Length, Rectangle, Point, Size,
    widget::canvas::{self, Canvas, Frame, Geometry, Text},
    mouse,
};
use crate::terminal::screen::{TerminalScreen, Rgb};
use crate::ui::app::Message;

const FONT_SIZE: f32 = 14.0;
const NERD_FONT: iced::Font = iced::Font::with_name("JetBrainsMono Nerd Font");

pub struct TerminalWidget<'a> {
    screen: &'a TerminalScreen,
}

impl<'a> TerminalWidget<'a> {
    pub const CHAR_WIDTH: f32 = FONT_SIZE * 0.601;
    pub const CHAR_HEIGHT: f32 = FONT_SIZE * 1.2;

    pub fn new(screen: &'a TerminalScreen) -> Self {
        TerminalWidget { screen }
    }
}

impl<'a> canvas::Program<Message> for TerminalWidget<'a> {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: canvas::Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> (canvas::event::Status, Option<Message>) {
        let bytes = match event {
            canvas::Event::Keyboard(key_event) => map_key_event(key_event),
            _ => return (canvas::event::Status::Ignored, None),
        };

        match bytes {
            Some(b) => (canvas::event::Status::Captured, Some(Message::KeyInput(b))),
            None => (canvas::event::Status::Ignored, None),
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), iced::Color::BLACK);

        let cw = Self::CHAR_WIDTH;
        let ch = Self::CHAR_HEIGHT;

        for (row_idx, row) in self.screen.grid.iter().enumerate() {
            for (col_idx, cell) in row.iter().enumerate() {
                let x = col_idx as f32 * cw;
                let y = row_idx as f32 * ch;

                if cell.bg != Rgb::BLACK {
                    frame.fill_rectangle(
                        Point::new(x, y),
                        Size::new(cw, ch),
                        rgb_to_iced(cell.bg),
                    );
                }

                if cell.ch != ' ' {
                    frame.fill_text(Text {
                        content: cell.ch.to_string(),
                        position: Point::new(x, y),
                        color: rgb_to_iced(cell.fg),
                        size: iced::Pixels(FONT_SIZE),
                        font: NERD_FONT,
                        horizontal_alignment: iced::alignment::Horizontal::Left,
                        vertical_alignment: iced::alignment::Vertical::Top,
                        ..Text::default()
                    });
                }
            }
        }

        vec![frame.into_geometry()]
    }
}

fn rgb_to_iced(rgb: Rgb) -> iced::Color {
    iced::Color::from_rgb8(rgb.r, rgb.g, rgb.b)
}

fn map_key_event(event: iced::keyboard::Event) -> Option<Vec<u8>> {
    use iced::keyboard::{Event, Key, key::Named};

    match event {
        Event::KeyPressed { key, modifiers, .. } => {
            if modifiers.control() {
                if let Key::Character(s) = &key {
                    let c = s.chars().next()?;
                    let byte = match c {
                        'c' | 'C' => 0x03u8,
                        'd' | 'D' => 0x04,
                        'z' | 'Z' => 0x1A,
                        'l' | 'L' => 0x0C,
                        'a' | 'A' => 0x01,
                        'e' | 'E' => 0x05,
                        'u' | 'U' => 0x15,
                        'k' | 'K' => 0x0B,
                        'w' | 'W' => 0x17,
                        _ => return None,
                    };
                    return Some(vec![byte]);
                }
            }

            match key {
                Key::Character(s) => Some(s.as_bytes().to_vec()),
                Key::Named(Named::Enter) => Some(b"\r".to_vec()),
                Key::Named(Named::Backspace) => Some(vec![0x7F]),
                Key::Named(Named::Tab) => Some(b"\t".to_vec()),
                Key::Named(Named::Escape) => Some(vec![0x1B]),
                Key::Named(Named::Space) => Some(b" ".to_vec()),
                Key::Named(Named::ArrowUp) => Some(b"\x1b[A".to_vec()),
                Key::Named(Named::ArrowDown) => Some(b"\x1b[B".to_vec()),
                Key::Named(Named::ArrowRight) => Some(b"\x1b[C".to_vec()),
                Key::Named(Named::ArrowLeft) => Some(b"\x1b[D".to_vec()),
                Key::Named(Named::Home) => Some(b"\x1b[H".to_vec()),
                Key::Named(Named::End) => Some(b"\x1b[F".to_vec()),
                Key::Named(Named::Delete) => Some(b"\x1b[3~".to_vec()),
                Key::Named(Named::PageUp) => Some(b"\x1b[5~".to_vec()),
                Key::Named(Named::PageDown) => Some(b"\x1b[6~".to_vec()),
                _ => None,
            }
        }
        _ => None,
    }
}

impl<'a> From<TerminalWidget<'a>> for Element<'a, Message> {
    fn from(w: TerminalWidget<'a>) -> Self {
        Canvas::new(w)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }
}

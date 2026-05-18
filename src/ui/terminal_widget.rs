use iced::{Element, Length, widget::container};
use crate::terminal::screen::TerminalScreen;
use crate::ui::app::Message;

const FONT_SIZE: f32 = 14.0;

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

impl<'a> From<TerminalWidget<'a>> for Element<'a, Message> {
    fn from(_w: TerminalWidget<'a>) -> Self {
        container(iced::widget::text("terminal loading..."))
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|_theme| iced::widget::container::Style {
                background: Some(iced::Background::Color(iced::Color::BLACK)),
                ..Default::default()
            })
            .into()
    }
}

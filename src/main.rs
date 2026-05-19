mod terminal;
mod db;
mod ui;

use ui::app::App;

static NERD_FONT_BYTES: &[u8] =
    include_bytes!("../assets/JetBrainsMonoNerdFont-Regular.ttf");

fn main() -> iced::Result {
    iced::application("ChoirCli", App::update, App::view)
        .subscription(App::subscription)
        .font(NERD_FONT_BYTES)
        .run_with(App::new)
}

pub mod app;
pub mod input;
pub mod mouse;
pub mod screens;
pub mod view;

use std::io;
use std::panic;
use std::path::PathBuf;

use anyhow::Result;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;

use crate::paths::Paths;

pub use app::App;

pub fn run(paths: Paths, exe: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        hook(info);
    }));
    let result = execute!(io::stdout(), EnableMouseCapture).map_err(anyhow::Error::from).and_then(|()| App::new(paths, exe).run(&mut terminal));
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}

pub mod app;
pub mod input;
pub mod screens;
pub mod view;

use std::path::PathBuf;

use anyhow::Result;

use crate::paths::Paths;

pub use app::App;

pub fn run(paths: Paths, exe: PathBuf) -> Result<()> {
    let mut terminal = ratatui::init();
    let result = App::new(paths, exe).run(&mut terminal);
    ratatui::restore();
    result
}

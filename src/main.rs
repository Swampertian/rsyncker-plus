use std::env;

use anyhow::Result;
use clap::Parser;
use rsync_tui::cli::Cli;
use rsync_tui::commands;
use rsync_tui::paths::Paths;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env()?;
    let exe = env::current_exe()?;
    commands::execute(cli, paths, exe)
}

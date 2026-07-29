use std::process::Command;

use anyhow::{Context, Result};

pub fn exec_shell(command: String, args: Vec<String>) -> Result<()> {
    Command::new(command)
        .args(args)
        .spawn()
        .context("Command Failed!")?;
    Ok(())
}

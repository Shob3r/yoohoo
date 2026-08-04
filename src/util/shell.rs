use std::process::Command;

use anyhow::{Context, Result};

pub fn exec_shell(command: &str, args: &[String]) -> Result<()> {
    Command::new(command)
        .args(args)
        .spawn()
        .context("Command Failed!")?;
    Ok(())
}

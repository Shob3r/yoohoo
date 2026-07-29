use std::process::Command;

pub fn exec_shell(
    command: String,
    args: Vec<String>,
) -> Result<(), &'static str> {
    Command::new(command)
        .args(&args)
        .spawn()
        .expect("Command failed!");
    Ok(())
}


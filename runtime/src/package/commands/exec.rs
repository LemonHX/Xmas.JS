//! Exec command implementation.

use color_eyre::eyre::{eyre, ContextCompat, Result};
use compact_str::ToCompactString;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env::{current_dir, current_exe, set_current_dir, temp_dir};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Command;
use which::which;

use crate::package::commands::add::add_packages;
use crate::package::commands::{install, new_path};
use crate::package::progress::log_verbose;
use crate::package::util::save_package;

/// Execute the exec command.
pub fn cmd_exec(args: &crate::package::Options, exe: &OsString, cmd_args: &[OsString]) -> Result<()> {
    install(args)?;

    exec_with_args(exe.as_ref(), cmd_args, vec![(OsString::from("PATH"), new_path()?)])
}

/// Execute the download-and-exec (x) command.
pub fn cmd_download_and_exec(
    args: &crate::package::Options,
    name: &OsString,
    cmd_args: &[OsString],
) -> Result<()> {
    if let Err(e) = which(name) {
        log_verbose(&e.to_string());
        install_bin_temp(args, name.to_str().wrap_err("package name invalid")?)?;
    }
    // The user agent a scaffold reads to decide whether to prompt. It used to be set for the whole
    // process, which is unsafe once there is more than one thread; it travels with the child now.
    exec_with_args(
        name.as_ref(),
        cmd_args,
        vec![
            (OsString::from("PATH"), new_path()?),
            (
                OsString::from("npm_config_user_agent"),
                OsString::from("yarn/1.22.19 npm/none xmas/0.0.0"),
            ),
        ],
    )
}

/// Execute a command with arguments
///
/// On unix this replaces the process, which is what makes `xmas dlx` and `xmas run` behave like
/// the command itself: the exit code, the signals and the terminal are the child's. Windows has no
/// way to become another process, so there we spawn it and exit with its code, which looks the
/// same from the outside and costs one more process.
#[tracing::instrument]
pub fn exec_with_args(
    exe: &OsStr,
    args: &[OsString],
    env: Vec<(OsString, OsString)>,
) -> Result<()> {
    let mut args = args.to_vec();
    args.insert(0, exe.to_os_string());

    #[cfg(unix)]
    {
        // std's own exec(), which is execvp underneath and needs no crate of its own. It returns
        // only when the command could not be run at all.
        use std::os::unix::process::CommandExt;
        let error = Command::new(exe).args(&args[1..]).envs(env).exec();
        Err(eyre!(
            "Failed to execute {}: {}",
            exe.to_string_lossy(),
            error
        ))
    }

    #[cfg(not(unix))]
    {
        let status = Command::new(exe).args(&args[1..]).envs(env).status()?;
        std::process::exit(status.code().unwrap_or(1));
    }
}

/// Install a binary package to a temporary directory.
pub fn install_bin_temp(args: &crate::package::Options, package_name: &str) -> Result<()> {
    let orig_dir = current_dir()?;

    let dir_name: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    let mut temp_dir = temp_dir();
    temp_dir.push(dir_name);
    std::fs::create_dir(&temp_dir)?;
    set_current_dir(&temp_dir)?;
    log_verbose(&format!("Now in {temp_dir:?}"));

    save_package(&Value::Object(Map::new()))?;
    add_packages(&[package_name.to_compact_string()], false, false)?;
    install(args)?;
    let current_exe = current_exe().map(|p| p.to_string_lossy().to_string())?;

    std::fs::create_dir_all("node_modules/.bin")?;
    #[cfg(windows)]
    {
        // 使用 cmd shim 代替符号链接
        let shim_content = format!("@ECHO off\r\n\"{current_exe}\" %*\r\n");
        std::fs::write("node_modules/.bin/yarn.cmd", &shim_content)?;
        let ps1_content =
            format!("#!/usr/bin/env pwsh\r\n& \"{current_exe}\" $args\r\nexit $LASTEXITCODE\r\n");
        std::fs::write("node_modules/.bin/yarn.ps1", &ps1_content)?;
    }
    #[cfg(unix)]
    {
        crate::package::plan::symlink(&current_exe, "node_modules/.bin/yarn", None)?;
    }

    set_current_dir(&orig_dir)?;
    log_verbose(&format!("Now in {orig_dir:?}"));

    Ok(())
}

/// Run one script the way a shell would.
///
/// The platform's own shell does the parsing: sh on unix, cmd on windows. That is what npm does,
/// so a script gets the shell its author tested against instead of a reimplementation with gaps
/// of its own. The child inherits this process's terminal, so a prompt or a colour still works,
/// and a ctrl-C reaches it the way it reaches anything else in the foreground.
pub fn shell(text: &str, cwd: PathBuf, new_env: HashMap<OsString, OsString>) -> Result<i32> {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd");
        command.arg("/C").arg(text);
        command
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(text);
        command
    };

    command.current_dir(cwd);
    for (name, value) in new_env {
        command.env(name, value);
    }

    let status = command.status()?;

    Ok(exit_code(status))
}

/// The code a script ended with, in the shell's own convention: 128 plus the signal that
/// stopped it when a signal did.
fn exit_code(status: std::process::ExitStatus) -> i32 {
    if let Some(code) = status.code() {
        return code;
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128 + signal;
        }
    }
    1
}

//! Exec command implementation.

use color_eyre::eyre::{eyre, ContextCompat, Result};
use compact_str::ToCompactString;
use deno_task_shell::KillSignal;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::env::{current_dir, current_exe, set_current_dir, set_var, temp_dir};
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use tokio::fs::create_dir;
use which::which;

use crate::commands::add::add_packages;
use crate::commands::{install, join_paths};
use crate::progress::log_verbose;
use crate::util::save_package;

/// Execute the exec command.
pub async fn cmd_exec(args: &crate::Args, exe: &OsString, cmd_args: &[OsString]) -> Result<()> {
    install(args).await?;
    join_paths()?;

    exec_with_args(exe.as_ref(), cmd_args)
}

/// Execute the download-and-exec (x) command.
pub async fn cmd_download_and_exec(
    args: &crate::Args,
    name: &OsString,
    cmd_args: &[OsString],
) -> Result<()> {
    if let Err(e) = which(name) {
        log_verbose(&e.to_string());
        install_bin_temp(args, name.to_str().wrap_err("package name invalid")?).await?;
    }
    exec_with_args(name.as_ref(), cmd_args)
}

/// Execute a command with arguments
#[tracing::instrument]
pub fn exec_with_args(exe: &OsStr, args: &[OsString]) -> Result<()> {
    let mut args = args.to_vec();
    args.insert(0, exe.to_os_string());
    let error = exec::execvp(&exe, &args);
    match error {
        exec::Error::BadArgument(nul_error) => Err(eyre!("Invalid argument: {}", nul_error)),
        exec::Error::Errno(errno) => {
            if errno.0 != 0 {
                Err(eyre!("Failed to execute command: errno {}", errno.0))
            } else {
                Ok(())
            }
        }
    }
}

/// Install a binary package to a temporary directory.
pub async fn install_bin_temp(args: &crate::Args, package_name: &str) -> Result<()> {
    let orig_dir = current_dir()?;

    let dir_name: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(10)
        .map(char::from)
        .collect();

    let mut temp_dir = temp_dir();
    temp_dir.push(dir_name);
    create_dir(&temp_dir).await?;
    set_current_dir(&temp_dir)?;
    log_verbose(&format!("Now in {temp_dir:?}"));

    save_package(&Value::Object(Map::new())).await?;
    add_packages(&[package_name.to_compact_string()], false, false).await?;
    install(args).await?;
    set_var("npm_config_user_agent", "yarn/1.22.19 npm/none xmas/0.0.0");
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
        crate::plan::symlink(&current_exe, "node_modules/.bin/yarn", None)?;
    }

    join_paths()?;

    set_current_dir(&orig_dir)?;
    log_verbose(&format!("Now in {orig_dir:?}"));

    Ok(())
}

pub async fn shell(
    text: &str,
    cwd: PathBuf,
    new_env: HashMap<OsString, OsString>,
    kill_signal: KillSignal,
) -> Result<i32> {
    // parse
    let list =
        deno_task_shell::parser::parse(&text).map_err(|e| eyre!("Shell parse error: {}", e))?;

    // execute
    let mut env_vars = std::env::vars_os().collect::<HashMap<_, _>>();
    for (k, v) in new_env {
        let _ = env_vars.insert(k, v);
    }

    let exit_code = deno_task_shell::execute(
        list,
        env_vars,
        cwd,
        Default::default(), // custom commands
        kill_signal,
    )
    .await;

    Ok(exit_code)
}

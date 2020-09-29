// SPDX-FileCopyrightText: 2020 Serokell <https://serokell.io/>
//
// SPDX-License-Identifier: MPL-2.0

use clap::Clap;

use std::process::Stdio;
use tokio::process::Command;

use std::path::Path;

extern crate pretty_env_logger;
#[macro_use]
extern crate log;

#[macro_use]
extern crate serde_derive;

#[macro_use]
mod utils;

/// Activation portion of the simple Rust Nix deploy tool
#[derive(Clap, Debug)]
#[clap(version = "1.0", author = "notgne2 <gen2@gen2.space>")]
struct Opts {
    profile_path: String,
    closure: String,

    /// Command for activating the given profile
    #[clap(long)]
    activate_cmd: Option<String>,

    /// Command for bootstrapping
    #[clap(long)]
    bootstrap_cmd: Option<String>,

    /// Auto rollback if failure
    #[clap(long)]
    auto_rollback: bool,
}

pub async fn activate(
    profile_path: String,
    closure: String,
    activate_cmd: Option<String>,
    bootstrap_cmd: Option<String>,
    auto_rollback: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    info!("Activating profile");

    Command::new("nix-env")
        .arg("-p")
        .arg(&profile_path)
        .arg("--set")
        .arg(&closure)
        .stdout(Stdio::null())
        .spawn()?
        .await?;

    if let (Some(bootstrap_cmd), false) = (bootstrap_cmd, !Path::new(&profile_path).exists()) {
        let bootstrap_status = Command::new("bash")
            .arg("-c")
            .arg(&bootstrap_cmd)
            .env("PROFILE", &profile_path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await;

        match bootstrap_status {
            Ok(s) if s.success() => (),
            _ => {
                tokio::fs::remove_file(&profile_path).await?;
                good_panic!("Failed to execute bootstrap command");
            }
        }
    }

    if let Some(activate_cmd) = activate_cmd {
        let activate_status = Command::new("bash")
            .arg("-c")
            .arg(&activate_cmd)
            .env("PROFILE", &profile_path)
            .status()
            .await;

        match activate_status {
            Ok(s) if s.success() => (),
            _ if auto_rollback => {
                Command::new("nix-env")
                    .arg("-p")
                    .arg(&profile_path)
                    .arg("--rollback")
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?
                    .await?;

                let c = Command::new("nix-env")
                    .arg("-p")
                    .arg(&profile_path)
                    .arg("--list-generations")
                    .output()
                    .await?;
                let generations_list = String::from_utf8(c.stdout)?;

                let last_generation_line = generations_list
                    .lines()
                    .last()
                    .expect("Expected to find a generation in list");

                let last_generation_id = last_generation_line
                    .split_whitespace()
                    .next()
                    .expect("Expected to get ID from generation entry");

                debug!("Removing generation entry {}", last_generation_line);
                warn!("Removing generation by ID {}", last_generation_id);

                Command::new("nix-env")
                    .arg("-p")
                    .arg(&profile_path)
                    .arg("--delete-generations")
                    .arg(last_generation_id)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()?
                    .await?;

                // TODO: Find some way to make sure this command never changes, otherwise this will not work
                Command::new("bash")
                    .arg("-c")
                    .arg(&activate_cmd)
                    .spawn()?
                    .await?;

                good_panic!("Failed to execute activation command");
            }
            _ => {}
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var("DEPLOY_LOG").is_err() {
        std::env::set_var("DEPLOY_LOG", "info");
    }

    pretty_env_logger::init_custom_env("DEPLOY_LOG");

    let opts: Opts = Opts::parse();

    activate(
        opts.profile_path,
        opts.closure,
        opts.activate_cmd,
        opts.bootstrap_cmd,
        opts.auto_rollback,
    )
    .await?;

    Ok(())
}
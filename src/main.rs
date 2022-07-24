//use anyhow::{bail, Context};
use clap::Parser;
use std::{process::Stdio, sync::Arc};
use tokio::{
    fs,
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    process::Command,
    runtime,
    sync::{OwnedSemaphorePermit, Semaphore},
};

/// Simple utility to queue commands to be run
#[derive(Parser, Debug)]
#[clap(version, about, long_about = None)]
struct Config {
    /// Max number of parallel jobs
    #[clap(short, long, default_value = "1")]
    jobs: u16,
    #[clap(index = 1)]
    filename: Option<String>,
}

#[derive(Clone)]
struct RunnerConfig {
    inherit_output: bool,
    inherit_input: bool,
}

impl From<Config> for RunnerConfig {
    fn from(c: Config) -> Self {
        Self {
            inherit_input: c.filename.is_some() && c.jobs == 1,
            inherit_output: c.jobs == 1,
        }
    }
}

async fn runner(command: String, cfg: RunnerConfig, perm: OwnedSemaphorePermit) {
    let parts: Vec<&str> = command.split_ascii_whitespace().collect();
    let mut command = Command::new(parts[0]);
    let output_stdio = || {
        if cfg.inherit_output {
            Stdio::inherit()
        } else {
            Stdio::null()
        }
    };
    command
        .args(&parts[1..])
        .kill_on_drop(false)
        .stdin(if cfg.inherit_input {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(output_stdio())
        .stdout(output_stdio());
    if let Err(e) = command.status().await {
        eprintln!(
            "Error running child process \"{}\" with args {:#?}: {}",
            parts[0],
            &parts[1..],
            e
        );
    }
    drop(perm);
}

fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    rt.block_on(async move {
        let input = fs::OpenOptions::new()
            .read(true)
            .open(
                &cfg.filename
                    .as_ref()
                    .map(String::as_str)
                    .unwrap_or("/dev/stdin"),
            )
            .await?;
        let reader = BufReader::new(input);
        let mut lines = reader.lines();
        let semaphore = Arc::from(Semaphore::new(cfg.jobs as usize));
        let rt_conf = RunnerConfig::from(cfg);
        while let Some(line) = lines.next_line().await? {
            let perm = semaphore.clone().acquire_owned().await.unwrap();
            tokio::spawn(runner(line.clone(), rt_conf.clone(), perm));
        }
        Ok(())
    })
}

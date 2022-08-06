//use anyhow::{bail, Context};
use clap::Parser;
use std::{process::Stdio, sync::Arc};
use tokio::{
    fs,
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    runtime,
    sync::{mpsc, OwnedSemaphorePermit, Semaphore},
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

async fn runner(command_str: String, cfg: RunnerConfig, perm: OwnedSemaphorePermit) {
    let mut command = Command::new("sh");
    let output_stdio = || {
        if cfg.inherit_output {
            Stdio::inherit()
        } else {
            Stdio::null()
        }
    };
    command
        .args(&["-c", &command_str])
        .kill_on_drop(false)
        .stdin(if cfg.inherit_input {
            Stdio::inherit()
        } else {
            Stdio::null()
        })
        .stderr(output_stdio())
        .stdout(output_stdio());
    if !cfg.inherit_output {
        eprintln!("Starting job \"{}\".",&command_str);
    }
    match command.status().await {
        Err(e) => {
            eprintln!("Error running job \"{}\": {}", &command_str, e);
        }
        Ok(status) => {
            if !cfg.inherit_output {
                if status.success() {
                    eprintln!("Job \"{}\" finished.",&command_str);
                } else {
                    eprintln!("Job \"{}\" failed.",&command_str);
                }
            }
        }
    }
    drop(perm);
}

async fn awaiter_loop(mut rx: mpsc::UnboundedReceiver<tokio::task::JoinHandle<()>>) {
    while let Some(job) = rx.recv().await {
        let _ = job.await;
    }
}

fn main() -> anyhow::Result<()> {
    let cfg = Config::parse();
    let rt = runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (tx, rx) = mpsc::unbounded_channel();
    let wait_for_all = rt.spawn(async move {
        awaiter_loop(rx).await;
    });
    let res = rt.block_on(async move {
        let input = fs::OpenOptions::new()
            .read(true)
            .open(
                &cfg.filename
                    .as_deref()
                    .unwrap_or("/dev/stdin"),
            )
            .await?;
        let reader = BufReader::new(input);
        let mut lines = reader.lines();
        let semaphore = Arc::from(Semaphore::new(cfg.jobs as usize));
        let rt_conf = RunnerConfig::from(cfg);
        while let Some(line) = lines.next_line().await? {
            let perm = semaphore.clone().acquire_owned().await.unwrap();
            tx.send(tokio::spawn(runner(line.clone(), rt_conf.clone(), perm)))
                .unwrap();
        }
        Ok(())
    });
    rt.block_on(wait_for_all)?;
    res
}

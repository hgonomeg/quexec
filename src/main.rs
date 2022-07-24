use anyhow::{bail, Context};
use std::process::Stdio;
use tokio::{
    fs,
    io::{AsyncBufRead, AsyncBufReadExt, BufReader},
    process::Command,
    runtime,
};

async fn runner(command: &str) {
    let parts: Vec<&str> = command.split_ascii_whitespace().collect();
    let mut command = Command::new(parts[0]);
    command
        .args(&parts[1..])
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::inherit());
    if let Err(e) = command.status().await {
        eprintln!(
            "Error running child process \"{}\" with args {:#?}: {}",
            parts[0],
            &parts[1..],
            e
        );
    }
}

fn main() -> anyhow::Result<()> {
    if let Some(filename) = std::env::args().skip(1).next() {
        let rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        rt.block_on(async {
            let input = fs::OpenOptions::new().read(true).open(&filename).await?;
            let reader = BufReader::new(input);
            let mut lines = reader.lines();
            while let Some(line) = lines.next_line().await? {
                runner(&line).await;
            }
            Ok(())
        })
    } else {
        bail!("No input file given!")
    }
}

/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Debug;
use std::fs::File as FsFile;
use std::io::BufRead;
use std::io::BufReader;
use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use edenfs_utils::path_from_bytes;
use hg_util::path::expand_path;
use serde::Deserialize;
use serde::Serialize;

use crate::ExitCode;
use crate::Subcommand;

#[cfg(target_os = "macos")]
#[derive(Parser, Debug)]
#[clap(
    name = "file-access-monitor",
    alias = "fam",
    about = "File Access Monitor(FAM) to audit processes.\nAvailable only on macOS."
)]
pub struct FileAccessMonitorCmd {
    #[clap(subcommand)]
    subcommand: FileAccessMonitorSubcommand,
}

#[derive(Parser, Debug)]
#[clap(about = "Start File Access Monitor. File access events are logged to the output file.")]
struct StartCmd {
    #[clap(
        help = "A list of paths that FAM should use as filters when monitoring file access events.",
        short = 'p',
        long = "paths",
        required = true
    )]
    paths: Vec<String>,

    #[clap(
        help = "The path of the output file where the file access events are logged.",
        short = 'o',
        long = "output"
    )]
    output: Option<String>,

    #[clap(
        help = "When set, the command returns immediately, leaving FAM running in the background.\nTo stop it, run 'eden fam stop'.",
        short = 'b',
        long = "background"
    )]
    background: bool,

    #[clap(
        help = "How long FAM should run in seconds. This should not be set when '--background' is set.",
        short = 't',
        long = "timeout",
        default_value = "30",
        conflicts_with = "background"
    )]
    timeout: u64,

    #[clap(help = "When set, the output file is uploaded and a link is returned.")]
    upload: bool,
}

#[async_trait]
impl crate::Subcommand for StartCmd {
    async fn run(&self) -> Result<ExitCode> {
        println!("Starting File Access Monitor");

        let mut monitor_paths: Vec<PathBuf> = Vec::new();

        for path in &self.paths {
            monitor_paths.push(expand_path(path));
        }

        let output_path = self.output.as_ref().map(expand_path);

        let start_result = EdenFsInstance::global()
            .start_file_access_monitor(&monitor_paths, output_path, self.upload)
            .await?;

        println!("File Access Monitor started [pid {}]", start_result.pid);
        println!(
            "Temp output file path: {}",
            path_from_bytes(&start_result.tmpOutputPath)?.display()
        );

        if self.background {
            println!("File Access Monitor is running in the background");
            return Ok(0);
        }

        // TODO[lxw]: handle timeout

        stop_fam().await
    }
}

async fn stop_fam() -> Result<ExitCode> {
    let stop_result = EdenFsInstance::global().stop_file_access_monitor().await?;
    println!("File Access Monitor stopped");
    // TODO: handle the case when the output file is specified
    let output_path = path_from_bytes(&stop_result.specifiedOutputPath)?;

    println!("Output file saved to {}", output_path.display());

    if stop_result.shouldUpload {
        // TODO[lxw]: handle uploading outputfile
        println!("Upload not implemented yet");
        return Ok(1);
    }
    Ok(0)
}

#[derive(Parser, Debug)]
#[clap(about = "Stop File Access Monitor to audit processes.")]
struct StopCmd {}

#[async_trait]
impl crate::Subcommand for StopCmd {
    async fn run(&self) -> Result<ExitCode> {
        stop_fam().await
    }
}

#[derive(Parser, Debug)]
#[clap(about = "Read the output file and parse it to a summary of file access events.")]
struct ReadCmd {
    #[clap(
        help = "Path to the FAM output file. This file is generated by FAM when monitoring file system activity.",
        short = 'p',
        long = "path",
        required = true
    )]
    path: String,

    #[clap(
        help = "Print verbose information about parsed events.",
        long = "verbose",
        required = false
    )]
    verbose: bool,

    #[clap(
        help = "Specify the maximum number of PIDs to be displayed in the output. If set to 0, all PIDs will be displayed.",
        short = 'k',
        required = false,
        default_value = "10"
    )]
    count: usize,
}

#[derive(Serialize, Deserialize, Debug)]
struct File {
    path: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct Process {
    pid: u64,
    ppid: u64,
    uid: u64,
    ancestors: Vec<u64>,
    args: Vec<String>,
    command: String,
}
#[derive(Serialize, Deserialize, Debug)]
struct Event {
    event_type: String,
    file: File,
    process: Process,
    event_timestamp: u64,
}

fn parse_events<R: BufRead>(reader: R) -> Result<Vec<Event>> {
    let mut objects: Vec<Event> = Vec::new();
    let mut newObject = String::new();
    for line in reader.lines().map_while(Result::ok) {
        newObject.push_str(&line);
        if line == "}" {
            objects.push(serde_json::from_str(&newObject)?);
            newObject.clear();
        }
    }
    Ok(objects)
}

#[async_trait]
impl crate::Subcommand for ReadCmd {
    async fn run(&self) -> Result<ExitCode> {
        // construct the path
        let path = PathBuf::from(&self.path);
        let file = FsFile::open(path)?;
        let reader = BufReader::new(file);

        let objects = parse_events(reader)?;

        if self.verbose {
            println!("Parsed {} objects", objects.len());
            println!("{:#?}", objects);
        }

        Ok(0)
    }
}

#[derive(Parser, Debug)]
enum FileAccessMonitorSubcommand {
    Start(StartCmd),
    Stop(StopCmd),
    Read(ReadCmd),
}

#[async_trait]
impl Subcommand for FileAccessMonitorCmd {
    async fn run(&self) -> Result<ExitCode> {
        use FileAccessMonitorSubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            Start(cmd) => cmd,
            Stop(cmd) => cmd,
            Read(cmd) => cmd,
        };
        sc.run().await
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn test_parse_complete_event() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "path": "what"
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)));
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1);
    }

    #[test]
    fn test_parse_complete_events() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "path": "what"
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}

{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "path": "what"
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)));
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 2);
    }

    #[test]
    fn test_parse_incomplete_events() {
        let event = r#"
{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "path": "what"
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/python3",
    "pid": 22222,
    "ppid": 99999,
    "uid": 67890
  },
  "event_timestamp": 1740024705
}

{
  "event_type": "NOTIFY_OPEN",
  "file": {
    "path": "what"
  },
  "process": {
    "ancestors": [],
    "args": [],
    "command": "/usr/local/bin/pyth
        "#;
        let parsed = parse_events(BufReader::new(Cursor::new(event)));
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap().len(), 1);
    }
}

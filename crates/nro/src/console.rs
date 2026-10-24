//! Client for the mod's developer console: `\\.\pipe\nrpvp-<pid>`, one request line, reply lines up to a line holding only EOT
//! (nightreign-pvp `crates/nrpvp/src/console/pipe.rs`). One client at a time, so every request opens, asks and closes.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::time::{Duration, Instant};

const END: &str = "\u{4}";

pub fn pipe_path(pid: u32) -> String {
    format!(r"\\.\pipe\nrpvp-{pid}")
}

pub fn exists(pid: u32) -> bool {
    std::fs::read_dir(r"\\.\pipe\")
        .map(|d| {
            d.filter_map(|e| e.ok())
                .any(|e| e.file_name().to_string_lossy() == format!("nrpvp-{pid}"))
        })
        .unwrap_or(false)
}

/// Send one line, return the reply (lines joined with '\n'). Retries the open while another client holds the pipe.
pub fn ask(pid: u32, line: &str, timeout: Duration) -> Result<String, String> {
    let start = Instant::now();
    let f = loop {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .open(pipe_path(pid))
        {
            Ok(f) => break f,
            Err(e) if start.elapsed() < timeout => {
                let _ = e;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return Err(format!("console pipe of pid {pid}: {e}")),
        }
    };
    let mut w = f.try_clone().map_err(|e| e.to_string())?;
    w.write_all(format!("{}\n", line.trim()).as_bytes())
        .map_err(|e| e.to_string())?;
    w.flush().map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for l in BufReader::new(f).lines() {
        let l = l.map_err(|e| e.to_string())?;
        if l == END {
            break;
        }
        out.push(l);
    }
    Ok(out.join("\n"))
}

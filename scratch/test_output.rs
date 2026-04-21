use std::process::{Command, Stdio};
use std::io::{Write, BufReader, BufRead};
use std::time::Duration;
use std::thread;

fn main() {
    let mut child = Command::new("cargo")
        .args(&["run", "--release"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("Failed to start engine");

    let mut stdin = child.stdin.take().expect("Failed to open stdin");
    let stdout = child.stdout.take().expect("Failed to open stdout");

    // Send UCI commands
    writeln!(stdin, "uci").unwrap();
    writeln!(stdin, "isready").unwrap();
    writeln!(stdin, "position startpos").unwrap();
    writeln!(stdin, "go depth 12").unwrap(); // Depth 12 should take > 300ms on most machines
    writeln!(stdin, "quit").unwrap();

    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        let line = line.unwrap();
        println!("ENGINE: {}", line);
        if line.contains("bestmove") {
            break;
        }
    }
}

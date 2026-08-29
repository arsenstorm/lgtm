//! `lgtm terminal`: put this tty on the shell in a task's worktree.

use std::io::{Read, Write};
use std::process::Stdio;

use lgtm_client::Client;
use tokio::sync::mpsc;

/// Ctrl-], the way telnet spells "let me out".
const DETACH: u8 = 0x1d;

/// Restores the tty however the attach ends, panic included.
struct Raw;

impl Raw {
    fn enter() -> Self {
        stty(&["raw", "-echo"]);
        Raw
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        stty(&["sane"]);
    }
}

/// `stty <args> < /dev/tty`: no dependency here allocates or configures a
/// tty, and the terminal is the one the person is sitting at, not stdin,
/// which is about to become a pipe of keystrokes.
fn stty(args: &[&str]) {
    let Ok(tty) = std::fs::File::open("/dev/tty") else {
        return;
    };
    let _ = std::process::Command::new("stty")
        .args(args)
        .stdin(Stdio::from(tty))
        .status();
}

pub async fn attach(client: &Client, id: &str) -> anyhow::Result<i32> {
    let mut shell = client.terminal(id).await?;
    eprintln!("attached to {id}; ctrl-] to detach");
    let _raw = Raw::enter();
    let mut keys = keystrokes();
    let mut stdout = std::io::stdout();
    loop {
        tokio::select! {
            output = shell.next() => match output {
                Some(text) => {
                    stdout.write_all(text.as_bytes())?;
                    stdout.flush()?;
                }
                None => return Ok(0),
            },
            typed = keys.recv() => match typed {
                Some(bytes) => shell.send(&String::from_utf8_lossy(&bytes)).await?,
                None => return Ok(0),
            },
        }
    }
}

/// Stdin has no async form, so a blocking thread reads it and the attach loop
/// selects on the channel. The thread ends on Ctrl-], which ends the attach.
fn keystrokes() -> mpsc::UnboundedReceiver<Vec<u8>> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let mut buf = [0u8; 1024];
        while let Ok(read) = stdin.read(&mut buf) {
            if read == 0 || buf[..read].contains(&DETACH) {
                return;
            }
            if tx.send(buf[..read].to_vec()).is_err() {
                return;
            }
        }
    });
    rx
}

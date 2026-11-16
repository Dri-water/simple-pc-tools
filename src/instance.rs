//! Keeps one copy running. The running copy listens on a local port for one-
//! line commands (SHOW, QUIT). A second launch from the Start menu sends SHOW
//! and exits; the installer and uninstaller send QUIT. The port carries no
//! data and takes no other commands: the UI itself is served inside the
//! window, not over the network.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use anyhow::{bail, Result};

pub const DEFAULT_PORT: u16 = 47615;
const HELLO: &str = "simple-pc-tools";

pub fn addr(port: u16) -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], port))
}

/// Sends a command to the running copy. Errors if none is running.
pub fn send(port: u16, command: &str) -> Result<()> {
    let mut stream = TcpStream::connect_timeout(&addr(port), Duration::from_millis(500))?;
    stream.set_read_timeout(Some(Duration::from_secs(3)))?;
    writeln!(stream, "{command}")?;
    let mut reply = String::new();
    let _ = stream.take(64).read_to_string(&mut reply);
    if !reply.starts_with(HELLO) {
        bail!("port {port} is used by another program");
    }
    Ok(())
}

pub fn is_running(port: u16) -> bool {
    send(port, "PING").is_ok()
}

/// Waits up to `secs` for a copy to come up (after starting the sign-in task).
pub fn wait_for(port: u16, secs: u64) -> bool {
    for _ in 0..secs * 4 {
        if is_running(port) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    false
}

pub enum Command {
    Show,
    Quit,
}

/// Claims the port; fails if another copy (or program) has it.
pub fn claim(port: u16) -> std::io::Result<TcpListener> {
    TcpListener::bind(addr(port))
}

/// Answers commands on a background thread.
pub fn serve(listener: TcpListener, on_command: impl Fn(Command) + Send + 'static) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut line = String::new();
            let mut reader = BufReader::new(&stream);
            if reader.read_line(&mut line).is_err() {
                continue;
            }
            let _ = (&stream).write_all(format!("{HELLO} ok\n").as_bytes());
            match line.trim() {
                "SHOW" => on_command(Command::Show),
                "QUIT" => on_command(Command::Quit),
                _ => {}
            }
        }
    });
}

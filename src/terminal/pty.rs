use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty};
use std::io::{Read, Write};
use std::sync::mpsc;

pub struct PtyHandle {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    #[allow(dead_code)] // kept alive to prevent premature shell exit
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
}

impl PtyHandle {
    pub fn write_bytes(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
    }
}

pub fn spawn_pty(
    cols: u16,
    rows: u16,
    cwd: &std::path::Path,
    cmd: &str,
    args: &[String],
) -> (PtyHandle, mpsc::Receiver<Vec<u8>>) {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .expect("failed to open PTY");

    let mut builder = CommandBuilder::new(cmd);
    builder.cwd(cwd);
    for arg in args {
        builder.arg(arg);
    }

    let child = pair.slave.spawn_command(builder).expect("failed to spawn shell");
    let writer = pair.master.take_writer().expect("failed to get PTY writer");

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let mut reader = pair.master.try_clone_reader().expect("failed to clone PTY reader");

    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() { break; }
                }
            }
        }
    });

    (PtyHandle { master: pair.master, writer, child }, rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore]
    fn pty_spawns_shell_and_echo_works() {
        let cwd = std::env::current_dir().unwrap();
        #[cfg(windows)]
        let (cmd, args): (&str, Vec<String>) = ("cmd.exe", vec![]);
        #[cfg(not(windows))]
        let (cmd, args): (String, Vec<String>) = (
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
            vec![],
        );
        let (mut pty, rx) = spawn_pty(80, 24, &cwd, &cmd, &args);

        std::thread::sleep(Duration::from_millis(500));

        let input = if cfg!(windows) { b"echo __HELLO__\r\n".as_ref() } else { b"echo __HELLO__\n".as_ref() };
        pty.write_bytes(input).unwrap();

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut output = Vec::new();
        while Instant::now() < deadline {
            if let Ok(data) = rx.try_recv() {
                output.extend_from_slice(&data);
                if output.windows(9).any(|w| w == b"__HELLO__") { return; }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("PTY did not echo: {:?}", String::from_utf8_lossy(&output));
    }
}

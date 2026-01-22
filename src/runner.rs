use std::sync::mpsc;
use std::{io, thread};
use std::io::{Read, Write};

use portable_pty::{CommandBuilder, PtySize};

use crate::ssh_args::Invocation;

#[derive(Debug, Clone, Copy)]
pub(crate) struct PtyExit {
    pub(crate) code: i32,
    pub(crate) auth_failed: bool,
}

// PTY relationship:
// - Slave: SSH process sees this as a "terminal" interface
//   * SSH needs a terminal to display interactive prompts (e.g., "Password:")
//   * SSH reads user input from the terminal
//   * Without a terminal, SSH may not show interactive prompts
// - Master: Our control program uses this for I/O
//   * Read: Receive SSH output (including prompts) from master
//   * Write: Send input (e.g., password) to master, SSH receives it from slave
pub(crate) fn run_ssh_with_pty(
    inv: &Invocation,
    password: Option<&str>,
    kill_rx: mpsc::Receiver<()>,
) -> io::Result<PtyExit> {
    // Use the native pty implementation for the system
    let pty_system = portable_pty::native_pty_system();
    // Create a new pty
    let pair = pty_system
        .openpty(PtySize {
            rows: 24,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, format!("openpty failed: {e}")))?;

    let mut cmd = CommandBuilder::new(&inv.program);
    for a in &inv.args {
        cmd.arg(a);
    }

    // Spawn SSH process attached to slave side (SSH thinks it's using a terminal).
    let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
        io::Error::new(
            io::ErrorKind::Other,
            format!("spawn ssh in pty failed: {e}"),
        )
    })?;
    // Drop slave handle: SSH process now owns the slave side and will keep it open until it exits.
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("pty reader failed: {e}"))
    })?;
    let mut writer = pair.master.take_writer().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("pty writer failed: {e}"))
    })?;

    // portable-pty uses blocking I/O; read PTY output on a dedicated thread and forward via mpsc.
    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>();
    let reader_handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if out_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut sent_password = false;
    // Keep a small tail to catch prompts split across chunks,
    // but avoid matching old prompts repeatedly.
    let mut tail = String::new();
    let mut auth_failed = false;

    // Main loop: handle shutdown, forward output, respond to prompts, and poll process exit.
    loop {
        // Check shutdown (triggered by supervisor on Ctrl-C)
        match kill_rx.try_recv() {
            Ok(()) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader_handle.join();
                return Ok(PtyExit {
                    code: 0,
                    auth_failed: false,
                });
            }
            Err(mpsc::TryRecvError::Empty) => {}
            Err(mpsc::TryRecvError::Disconnected) => {}
        }

        // Use timeout to allow polling child status.
        match out_rx.recv_timeout(std::time::Duration::from_millis(200)) {
            Ok(chunk) => {
                // Forward output to console (PTY mixes stdout/stderr)
                let _ = io::stdout().write_all(&chunk);
                let _ = io::stdout().flush();

                let s = String::from_utf8_lossy(&chunk);
                let combined = format!("{}{}", tail, s);
                let lower = combined.to_lowercase();

                // Safer default: do NOT auto-accept unknown host keys.
                // If this prompt appears, instruct user to configure StrictHostKeyChecking in ssh_extra_args.
                if lower.contains("are you sure you want to continue connecting") {
                    eprintln!(
                        "\nEncountered host key confirmation prompt. \
Please add an ssh option like: -o StrictHostKeyChecking=accept-new (recommended) \
or pre-populate known_hosts, then retry."
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = reader_handle.join();
                    return Ok(PtyExit {
                        code: 1,
                        auth_failed: false,
                    });
                }

                // Authentication failure (wrong password / password auth disabled / etc.)
                // Once detected, stop auto-retry for this rule.
                if lower.contains("permission denied") || lower.contains("too many authentication failures") {
                    auth_failed = true;
                }

                let mut handled_prompt_this_chunk = false;

                // Password prompt: answer only once to avoid infinite loops.
                if lower.contains("password:") || lower.contains("password for") {
                    if sent_password {
                        eprintln!("\nPassword was requested again; aborting. (Check ssh_password)");
                        let _ = child.kill();
                        let _ = child.wait();
                        let _ = reader_handle.join();
                        return Ok(PtyExit {
                            code: 1,
                            auth_failed,
                        });
                    }
                    // Only send password if one was provided
                    if let Some(pw) = password {
                        writer
                            .write_all(pw.as_bytes())
                            .and_then(|_| writer.write_all(b"\n"))
                            .map_err(|e| {
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    format!("write password failed: {e}"),
                                )
                            })?;
                        let _ = writer.flush();
                        sent_password = true;
                        handled_prompt_this_chunk = true;
                    } else {
                        // No password provided but password prompt appeared
                        auth_failed = true;
                    }
                }

                // Key passphrase prompt (reuse ssh_password if provided).
                if lower.contains("enter passphrase") && !sent_password {
                    if let Some(pw) = password {
                        writer
                            .write_all(pw.as_bytes())
                            .and_then(|_| writer.write_all(b"\n"))
                            .map_err(|e| {
                                io::Error::new(
                                    io::ErrorKind::BrokenPipe,
                                    format!("write passphrase failed: {e}"),
                                )
                            })?;
                        let _ = writer.flush();
                        sent_password = true;
                        handled_prompt_this_chunk = true;
                    } else {
                        // No password provided but passphrase prompt appeared
                        auth_failed = true;
                    }
                }

                // If we just responded to a prompt, drop the tail completely so we don't
                // re-match the same prompt on the next output chunk.
                if handled_prompt_this_chunk {
                    tail.clear();
                } else {
                    // Update tail to last N chars of combined
                    const TAIL_MAX: usize = 128;
                    if combined.len() <= TAIL_MAX {
                        tail = combined;
                    } else {
                        tail = combined[combined.len() - TAIL_MAX..].to_string();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // fall through to child polling
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                // reader ended; wait for process
                break;
            }
        }

        // Poll for process exit without blocking the prompt/kill handling.
        match child.try_wait() {
            Ok(Some(status)) => {
                let code = if status.success() { 0 } else { 1 };
                let _ = reader_handle.join();
                return Ok(PtyExit { code, auth_failed });
            }
            Ok(None) => {}
            Err(_) => {}
        }
    }

    let status = child.wait().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("wait failed: {e}"))
    })?;
    let code = if status.success() { 0 } else { 1 };
    let _ = reader_handle.join();
    Ok(PtyExit { code, auth_failed })
}


use std::{io, sync::mpsc};

use tokio::sync::watch;
use tokio::time::{sleep, Duration};

use crate::config::{Config, ForwardingRule};
use crate::runner::run_ssh_with_pty;
use crate::ssh_args::{build_invocation, Invocation};

// format rule full information, for logging
fn format_rule_full(rule: &ForwardingRule) -> String {
    format!(
        "local {}:{} -> {} via {}@{}:{}",
        rule.local_bind,
        rule.local_port,
        rule.remote_address,
        rule.ssh_user,
        rule.ssh_host,
        rule.ssh_port
    )
}

// Supervise a single forwarding rule: run ssh, auto-restart on disconnect, stop on auth failure or shutdown.
pub async fn supervise_ssh(rule: ForwardingRule, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    let mut attempt: u32 = 0;

    // Restart loop: reconnect on failure with exponential backoff (max 20s).
    loop {
        if *shutdown.borrow() {
            break;
        }

        // Build ssh command-line invocation from rule config.
        let inv = match build_invocation(&rule) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("Config error for {}: {}", format_rule_full(&rule), e);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
            }
        };

        println!("Starting ssh forward: {}", format_rule_full(&rule));

        // Unified PTY mode: works for both password and non-password modes.
        let password = rule.ssh_password.clone().filter(|s| !s.is_empty());
        let (kill_tx, kill_rx) = mpsc::channel::<()>();
        let inv2 = Invocation {
            program: inv.program.clone(),
            args: inv.args.clone(),
        };

        // PTY operations are blocking; run on a blocking task.
        let mut handle = tokio::task::spawn_blocking(move || {
            run_ssh_with_pty(&inv2, password.as_deref(), kill_rx)
        });

        // Wait for ssh to exit or shutdown signal; stop retrying on auth failure.
        tokio::select! {
            res = &mut handle => {
                match res {
                    // double result: spawn_blocking exit ok, run_ssh_with_pty exit ok
                    Ok(Ok(exit)) => {
                        eprintln!(
                            "ssh exited ({}:{} -> {}): code={}",
                            rule.local_bind, rule.local_port, rule.remote_address, exit.code
                        );
                        // Auth failure: stop retrying this rule to avoid log spam.
                        if exit.auth_failed {
                            eprintln!(
                                "Authentication failed for {}; not retrying.",
                                format_rule_full(&rule)
                            );
                            return Ok(());
                        }
                    }
                    Ok(Err(e)) => {
                        eprintln!(
                            "ssh pty error ({}:{} -> {}): {}",
                            rule.local_bind, rule.local_port, rule.remote_address, e
                        );
                    }
                    Err(e) => {
                        eprintln!(
                            "ssh pty task join error ({}:{} -> {}): {}",
                            rule.local_bind, rule.local_port, rule.remote_address, e
                        );
                    }
                }
            }
            _ = shutdown.changed() => {
                let _ = kill_tx.send(());
                let _ = handle.await;
                break;
            }
        }

        if *shutdown.borrow() {
            break;
        }

        // Auto-restart on disconnect/exit (with backoff)
        attempt = attempt.saturating_add(1);
        let backoff = Duration::from_secs((attempt.min(10) as u64).saturating_mul(2).max(1));
        eprintln!(
            "Restarting in {:?} ({}:{} -> {})",
            backoff, rule.local_bind, rule.local_port, rule.remote_address
        );
        sleep(backoff).await;
    }

    Ok(())
}

// Main entry point: start one supervisor task per forwarding rule, handle Ctrl-C gracefully.
pub async fn run(config: Config) -> io::Result<()> {
    println!("Loaded {} forwarding rule(s)", config.forwarding.len());

    // watch::channel broadcasts shutdown signal to all supervisor tasks.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start and supervise one persistent ssh process per rule
    let mut join_set = tokio::task::JoinSet::new();
    for rule in config.forwarding.into_iter() {
        let rx = shutdown_rx.clone();
        join_set.spawn(async move {
            if let Err(e) = supervise_ssh(rule, rx).await {
                eprintln!("forwarding task error: {}", e);
            }
        });
    }

    // Exit on Ctrl-C OR when all forwarding tasks finish (e.g. auth failure + no-retry).
    loop {
        tokio::select! {
            // Ctrl-C: broadcast shutdown, wait for all tasks to finish, then exit.
            _ = tokio::signal::ctrl_c() => {
                println!("Shutting down...");
                let _ = shutdown_tx.send(true);
                while let Some(_res) = join_set.join_next().await {
                    // drain
                }
                break;
            }
            // One task finished (e.g., auth failure); keep waiting for others or Ctrl-C.
            res = join_set.join_next() => {
                match res {
                    Some(_res) => {
                        // one task finished; keep waiting for others or Ctrl-C
                    }
                    None => {
                        println!("All forwarding tasks finished; exiting.");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}


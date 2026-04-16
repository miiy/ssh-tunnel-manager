use std::{io, sync::mpsc, time::Instant};

use tokio::sync::watch;
use tokio::time::{sleep, Duration};
use tracing::{error, info, warn};

use crate::config::{Config, ForwardingRule, TunnelMode};
use crate::runner::run_ssh_with_pty;
use crate::ssh_args::{build_invocation, Invocation};

// format rule summary, for logging
fn format_rule(rule: &ForwardingRule) -> String {
    let forwards: Vec<String> = rule
        .forwards
        .iter()
        .map(|f| {
            let mode_label = match f.mode {
                TunnelMode::Local => "L",
                TunnelMode::Remote => "R",
            };
            format!("[{}]{} -> {}", mode_label, f.local_address, f.remote_address)
        })
        .collect();
    format!(
        "{} via {}@{}:{}",
        forwards.join(", "),
        rule.ssh_user,
        rule.ssh_host,
        rule.ssh_port
    )
}

fn restart_backoff(attempt: u32) -> Duration {
    match attempt {
        0 => Duration::from_secs(0),
        1 => Duration::from_secs(30),
        2 => Duration::from_secs(60),
        3 => Duration::from_secs(120),
        _ => Duration::from_secs(300),
    }
}

// Supervise a single forwarding rule: run ssh, auto-restart on disconnect, stop on auth failure or shutdown.
pub async fn supervise_ssh(rule: ForwardingRule, mut shutdown: watch::Receiver<bool>) -> io::Result<()> {
    // Build ssh command-line invocation from rule config once (rule doesn't change in the loop).
    let inv = match build_invocation(&rule) {
        Ok(i) => i,
        Err(e) => {
            error!("Config error for {}: {}", format_rule(&rule), e);
            return Err(io::Error::new(io::ErrorKind::InvalidInput, e));
        }
    };

    let rule_desc = format_rule(&rule);
    let mut attempt: u32 = 0;

    // Restart loop: reconnect on failure with stepped backoff after a stable disconnect.
    loop {
        if *shutdown.borrow() {
            break;
        }

        info!("Starting ssh forward: {}", rule_desc);

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

        // Record start time to determine if connection was successfully established
        let start_time = Instant::now();
        // Wait for ssh to exit or shutdown signal; stop retrying on auth failure.
        let mut should_reset_attempt = false;

        // Note: If SSH process runs successfully, select! will wait
        tokio::select! {
            res = &mut handle => {
                match res {
                    // double result: spawn_blocking exit ok, run_ssh_with_pty exit ok
                    Ok(Ok(exit)) => {
                        let elapsed = start_time.elapsed();
                        warn!("ssh exited ({}): code={}, elapsed={:?}", rule_desc, exit.code, elapsed);
                        // Auth failure: stop retrying this rule to avoid log spam.
                        if exit.auth_failed {
                            error!("Authentication failed for {}; not retrying.", rule_desc);
                            return Ok(());
                        }
                        // Reset attempt if process ran for at least 5 seconds (connection was established before disconnect)
                        if elapsed.as_secs() >= 5 {
                            should_reset_attempt = true;
                        }
                    }
                    Ok(Err(e)) => {
                        error!("ssh pty error ({}): {}", rule_desc, e);
                    }
                    Err(e) => {
                        error!("ssh pty task join error ({}): {}", rule_desc, e);
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

        // Auto-restart on disconnect/exit. After a stable run, retry immediately once.
        // If reconnects keep failing, back off as: 30s -> 60s -> 120s -> 300s -> 300s...
        if should_reset_attempt {
            // Connection was established before the disconnect; allow one immediate retry.
            attempt = 0;
        } else {
            // Reconnect failed quickly; increase backoff for subsequent retries.
            attempt = attempt.saturating_add(1);
        }
        let backoff = restart_backoff(attempt);
        if backoff.is_zero() {
            warn!("Restarting immediately ({})", rule_desc);
        } else {
            warn!("Restarting in {:?} ({})", backoff, rule_desc);
            sleep(backoff).await;
        }
    }

    Ok(())
}

// Main entry point: start one supervisor task per forwarding rule, handle Ctrl-C gracefully.
pub async fn run(config: Config) -> io::Result<()> {
    info!("Loaded {} forwarding rule(s)", config.forwarding.len());

    // watch::channel broadcasts shutdown signal to all supervisor tasks.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Start and supervise one persistent ssh process per rule
    let mut join_set = tokio::task::JoinSet::new();
    for rule in config.forwarding.into_iter() {
        let rx = shutdown_rx.clone();
        join_set.spawn(async move {
            if let Err(e) = supervise_ssh(rule, rx).await {
                error!("forwarding task error: {}", e);
            }
        });
    }

    // Exit on Ctrl-C OR when all forwarding tasks finish (e.g. auth failure + no-retry).
    loop {
        tokio::select! {
            // Ctrl-C: broadcast shutdown, wait for all tasks to finish, then exit.
            _ = tokio::signal::ctrl_c() => {
                info!("Shutting down...");
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
                        info!("All forwarding tasks finished; exiting.");
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

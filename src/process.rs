//! Real process-kill campaigns.
//!
//! The simulated faults in [`crate::vfs`] and [`crate::writer`] never leave the test process.
//! This module does the opposite: it starts a real child process that writes to real files,
//! sends it `SIGKILL` (via [`std::process::Child::kill`]) at a random moment, and then hands
//! control to a verifier that reopens the storage and checks it.
//!
//! ## Acknowledgement protocol
//!
//! The child reports progress by printing lines to stdout. Only complete lines (ending in
//! `\n`) are collected, so a line cut off by the kill is never counted. A storage engine test
//! typically prints `ACK <n>` after operation `n` has returned (that is, after it was committed),
//! and the verifier then checks that every acknowledged operation survived.
//!
//! ```no_run
//! use std::process::Command;
//! use std::time::Duration;
//! use crash_sim::process::KillCampaign;
//!
//! let report = KillCampaign::new(1_000)
//!     .seed(7)
//!     .kill_window(Duration::from_millis(1), Duration::from_millis(40))
//!     .run(
//!         |trial| {
//!             let mut cmd = Command::new("./my-writer");
//!             cmd.arg(trial.seed.to_string());
//!             cmd
//!         },
//!         |_trial, outcome| {
//!             // Reopen the store and compare it with `outcome.acks`.
//!             let _acked = outcome.acks.len();
//!             Ok(())
//!         },
//!     )
//!     .expect("a trial failed verification");
//! println!("{report}");
//! ```

use std::fmt;
use std::io::{BufRead, BufReader};
use std::process::{Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use crate::prng::SimpleRng;

/// A series of spawn → wait a random time → `SIGKILL` → verify trials.
#[derive(Debug, Clone)]
pub struct KillCampaign {
    trials: u64,
    seed: u64,
    min_delay: Duration,
    max_delay: Duration,
}

/// What the verifier knows about one trial before the child is spawned.
#[derive(Debug, Clone, Copy)]
pub struct TrialContext {
    /// Zero-based trial number.
    pub index: u64,
    /// Per-trial seed derived from the campaign seed; pass it to the child for reproducible work.
    pub seed: u64,
    /// How long after spawning the child is killed.
    pub kill_after: Duration,
}

/// What happened to the child in one trial.
#[derive(Debug, Clone)]
pub struct TrialOutcome {
    /// Complete stdout lines printed before the child died, in order.
    pub acks: Vec<String>,
    /// `true` if the child was still running and was killed; `false` if it exited first.
    pub killed: bool,
    /// The child's exit status (a signal status when killed).
    pub status: ExitStatus,
}

/// A trial whose verifier returned an error, with everything needed to reproduce it.
#[derive(Debug, Clone)]
pub struct TrialFailure {
    pub trial: TrialContext,
    pub outcome: Option<TrialOutcome>,
    pub message: String,
}

impl fmt::Display for TrialFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "trial {} (seed {:#x}, kill after {:?}) failed: {}",
            self.trial.index, self.trial.seed, self.trial.kill_after, self.message
        )
    }
}

impl std::error::Error for TrialFailure {}

/// Totals for a finished campaign.
#[derive(Debug, Clone, Default)]
pub struct CampaignReport {
    pub trials: u64,
    /// Trials in which the child was killed while running.
    pub killed: u64,
    /// Trials in which the child exited on its own before the kill.
    pub exited_early: u64,
    /// Complete stdout lines collected across all trials.
    pub lines: u64,
    pub elapsed: Duration,
}

impl fmt::Display for CampaignReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} trials: {} SIGKILLed mid-run, {} exited before the kill, {} stdout lines collected, {:.1?}",
            self.trials, self.killed, self.exited_early, self.lines, self.elapsed
        )
    }
}

impl KillCampaign {
    /// A campaign of `trials` trials with seed 1 and a 1–50 ms kill window.
    pub fn new(trials: u64) -> Self {
        Self {
            trials,
            seed: 1,
            min_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(50),
        }
    }

    /// Sets the campaign seed. The same seed gives the same per-trial seeds and kill delays.
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = seed;
        self
    }

    /// Kills each child after a delay drawn uniformly from `[min, max]`.
    pub fn kill_window(mut self, min: Duration, max: Duration) -> Self {
        assert!(min <= max, "kill_window: min must not exceed max");
        self.min_delay = min;
        self.max_delay = max;
        self
    }

    /// Runs every trial, stopping at the first verifier error.
    ///
    /// `build` returns the command for a trial; its stdout is captured, stdin is closed and
    /// stderr is inherited. `verify` runs after the child is dead. Only the direct child is
    /// killed, so the writer should be a single process (use `exec` in shell wrappers): a
    /// grandchild that keeps stdout open delays the trial until it exits.
    pub fn run<B, V>(&self, mut build: B, mut verify: V) -> Result<CampaignReport, TrialFailure>
    where
        B: FnMut(&TrialContext) -> Command,
        V: FnMut(&TrialContext, &TrialOutcome) -> Result<(), String>,
    {
        let started = Instant::now();
        let mut rng = SimpleRng::new(self.seed);
        let mut report = CampaignReport::default();
        let span = self.max_delay.saturating_sub(self.min_delay).as_micros() as u64;

        for index in 0..self.trials {
            let seed = rng.next_u64();
            let jitter = if span == 0 { 0 } else { rng.next_u64() % (span + 1) };
            let trial = TrialContext {
                index,
                seed,
                kill_after: self.min_delay + Duration::from_micros(jitter),
            };

            let outcome = run_one(&trial, build(&trial)).map_err(|message| TrialFailure {
                trial,
                outcome: None,
                message,
            })?;

            report.trials += 1;
            report.lines += outcome.acks.len() as u64;
            if outcome.killed {
                report.killed += 1;
            } else {
                report.exited_early += 1;
            }

            if let Err(message) = verify(&trial, &outcome) {
                return Err(TrialFailure {
                    trial,
                    outcome: Some(outcome),
                    message,
                });
            }
        }

        report.elapsed = started.elapsed();
        Ok(report)
    }
}

fn run_one(trial: &TrialContext, mut cmd: Command) -> Result<TrialOutcome, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let mut child = cmd.spawn().map_err(|e| format!("spawn failed: {e}"))?;
    let stdout = child.stdout.take().expect("stdout is piped");

    let reader = thread::spawn(move || {
        let mut lines = Vec::new();
        let mut reader = BufReader::new(stdout);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    // A line without its newline was cut off by the kill: ignore it.
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                        lines.push(String::from_utf8_lossy(&buf).into_owned());
                    }
                }
            }
        }
        lines
    });

    thread::sleep(trial.kill_after);
    let killed = match child.try_wait() {
        Ok(Some(_)) => false,
        _ => child.kill().is_ok(),
    };
    let status = child.wait().map_err(|e| format!("wait failed: {e}"))?;
    let acks = reader.join().map_err(|_| "stdout reader panicked".to_string())?;

    Ok(TrialOutcome {
        acks,
        killed,
        status,
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn kills_a_running_child_and_keeps_only_complete_lines() {
        let report = KillCampaign::new(3)
            .kill_window(Duration::from_millis(150), Duration::from_millis(150))
            .run(
                |_| {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("echo ACK 0; printf 'partial'; exec sleep 5");
                    cmd
                },
                |_, outcome| {
                    if !outcome.killed {
                        return Err("child was not killed".into());
                    }
                    if outcome.acks != vec!["ACK 0".to_string()] {
                        return Err(format!("unexpected acks {:?}", outcome.acks));
                    }
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(report.killed, 3);
    }

    #[test]
    fn reports_children_that_exit_before_the_kill() {
        let report = KillCampaign::new(2)
            .kill_window(Duration::from_millis(300), Duration::from_millis(300))
            .run(
                |_| {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("echo done");
                    cmd
                },
                |_, outcome| {
                    assert!(!outcome.killed);
                    assert!(outcome.status.success());
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(report.exited_early, 2);
    }

    #[test]
    fn verifier_errors_carry_the_trial() {
        let err = KillCampaign::new(5)
            .seed(9)
            .kill_window(Duration::from_millis(1), Duration::from_millis(2))
            .run(
                |_| {
                    let mut cmd = Command::new("sh");
                    cmd.arg("-c").arg("true");
                    cmd
                },
                |trial, _| if trial.index == 2 { Err("boom".into()) } else { Ok(()) },
            )
            .unwrap_err();
        assert_eq!(err.trial.index, 2);
        assert!(err.to_string().contains("boom"));
    }
}

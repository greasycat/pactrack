use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::Duration;

use chrono::Local;
use log::{debug, info, warn};

use crate::commands::{DetectedAurHelper, perform_check};
use crate::config::EffectiveConfig;
use crate::state::{AppState, UpdateSnapshot};

#[derive(Debug, Clone)]
pub struct SchedulerUpdate {
    pub state: AppState,
    pub snapshot: Option<UpdateSnapshot>,
    pub helper: Option<DetectedAurHelper>,
}

#[derive(Debug)]
pub enum SchedulerCommand {
    RefreshNow,
    Quit,
}

pub fn start_scheduler(
    config: EffectiveConfig,
    updates_tx: Sender<SchedulerUpdate>,
) -> Sender<SchedulerCommand> {
    let (cmd_tx, cmd_rx) = mpsc::channel::<SchedulerCommand>();

    thread::spawn(move || run_scheduler(config, cmd_rx, updates_tx));
    cmd_tx
}

fn run_scheduler(
    config: EffectiveConfig,
    commands: Receiver<SchedulerCommand>,
    updates_tx: Sender<SchedulerUpdate>,
) {
    let interval = Duration::from_secs(config.poll_minutes.max(1) * 60);
    let mut last_state = AppState::default();
    let mut last_helper: Option<DetectedAurHelper> = None;

    run_once(
        &config,
        &updates_tx,
        &mut last_state,
        &mut last_helper,
        "startup",
    );

    loop {
        match commands.recv_timeout(interval) {
            Ok(SchedulerCommand::RefreshNow) => run_once(
                &config,
                &updates_tx,
                &mut last_state,
                &mut last_helper,
                "manual-refresh",
            ),
            Ok(SchedulerCommand::Quit) => {
                info!("scheduler received quit command");
                break;
            }
            Err(RecvTimeoutError::Timeout) => run_once(
                &config,
                &updates_tx,
                &mut last_state,
                &mut last_helper,
                "periodic",
            ),
            Err(RecvTimeoutError::Disconnected) => {
                debug!("scheduler command channel disconnected");
                break;
            }
        }
    }
}

fn run_once(
    config: &EffectiveConfig,
    updates_tx: &Sender<SchedulerUpdate>,
    last_state: &mut AppState,
    last_helper: &mut Option<DetectedAurHelper>,
    trigger: &str,
) {
    let checking_state = last_state.clone().with_checking();
    let _ = updates_tx.send(SchedulerUpdate {
        state: checking_state,
        snapshot: None,
        helper: *last_helper,
    });

    info!("running update check ({trigger})");
    let checked_at = Local::now();

    match perform_check(config) {
        Ok(outcome) => {
            let state = AppState::from_snapshot(&outcome.snapshot, checked_at);
            *last_state = state.clone();
            *last_helper = outcome.helper;

            let _ = updates_tx.send(SchedulerUpdate {
                state,
                snapshot: Some(outcome.snapshot),
                helper: outcome.helper,
            });
        }
        Err(err) => {
            warn!("update check failed: {err}");
            let state = last_state.clone().with_error(err.to_string(), checked_at);
            *last_state = state.clone();
            let _ = updates_tx.send(SchedulerUpdate {
                state,
                snapshot: None,
                helper: *last_helper,
            });
        }
    }
}

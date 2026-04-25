//! Multi-task runner (v0.5).
//!
//! Runs N tasks concurrently against a single llama-server using its
//! slot pool for KV-cache isolation. llama-server is started with
//! `--parallel N`; each task is assigned a unique `id_slot` so its KV
//! cache survives independently of every other task's. Equivalent to
//! Linux preemptive multitasking with per-process page tables — except
//! "page table" is "KV cache slot."
//!
//! v0.5 ships thread-per-task. v1.0 may replace with a true scheduler
//! that round-robins one CPU thread across all slots (closer to a real
//! Linux scheduler), once measurements show whether the per-thread cost
//! of one ureq HTTP client per task matters on Pi 5.

use crate::iod::{Daemon, DaemonConfig, TaskOutcome};
use anyhow::Result;
use std::sync::Arc;
use std::thread;

/// A pending task description.
#[derive(Debug, Clone)]
pub struct Task {
    pub goal: String,
    pub slot_id: i32,
    pub config: DaemonConfig,
}

/// Result of one task in a multi-task run.
#[derive(Debug)]
pub struct CompletedTask {
    pub goal: String,
    pub slot_id: i32,
    pub outcome: Option<TaskOutcome>,
    pub error: Option<String>,
}

/// Run all tasks concurrently. Each task spawns its own thread + ureq
/// client + Daemon. Returns when every task is complete (success or
/// failure).
///
/// **Caller responsibility**: ensure llama-server was started with
/// `--parallel N` where N ≥ tasks.len(). Otherwise tasks beyond the slot
/// count queue at the server, defeating concurrency.
pub fn run_all(tasks: Vec<Task>) -> Result<Vec<CompletedTask>> {
    let tasks = Arc::new(tasks);
    let mut handles = Vec::with_capacity(tasks.len());
    for i in 0..tasks.len() {
        let tasks = Arc::clone(&tasks);
        let h = thread::spawn(move || -> CompletedTask {
            let task = &tasks[i];
            let mut cfg = task.config.clone();
            cfg.slot_id = Some(task.slot_id);
            let daemon = match Daemon::new(cfg) {
                Ok(d) => d,
                Err(e) => {
                    return CompletedTask {
                        goal: task.goal.clone(),
                        slot_id: task.slot_id,
                        outcome: None,
                        error: Some(format!("daemon init failed: {e}")),
                    };
                }
            };
            match daemon.run_task(&task.goal) {
                Ok(o) => CompletedTask {
                    goal: task.goal.clone(),
                    slot_id: task.slot_id,
                    outcome: Some(o),
                    error: None,
                },
                Err(e) => CompletedTask {
                    goal: task.goal.clone(),
                    slot_id: task.slot_id,
                    outcome: None,
                    error: Some(format!("{e}")),
                },
            }
        });
        handles.push(h);
    }

    let mut out = Vec::with_capacity(handles.len());
    for h in handles {
        match h.join() {
            Ok(c) => out.push(c),
            Err(_) => out.push(CompletedTask {
                goal: "<panicked>".into(),
                slot_id: -1,
                outcome: None,
                error: Some("worker thread panicked".into()),
            }),
        }
    }
    Ok(out)
}

/// Helper: build a Vec<Task> from goals + a base config, auto-assigning
/// slot ids 0..N-1.
pub fn from_goals(goals: Vec<String>, base: DaemonConfig) -> Vec<Task> {
    goals
        .into_iter()
        .enumerate()
        .map(|(i, goal)| Task {
            goal,
            slot_id: i as i32,
            config: base.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_goals_assigns_unique_slot_ids() {
        let cfg = DaemonConfig::default();
        let tasks = from_goals(
            vec!["a".into(), "b".into(), "c".into()],
            cfg,
        );
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].slot_id, 0);
        assert_eq!(tasks[1].slot_id, 1);
        assert_eq!(tasks[2].slot_id, 2);
        let goals: Vec<&str> = tasks.iter().map(|t| t.goal.as_str()).collect();
        assert_eq!(goals, vec!["a", "b", "c"]);
    }
}

//! Preemptive scheduler (v0.5).
//!
//! Tracks per-task token + wall-clock budgets. The daemon consults the
//! scheduler after each segment and, if the budget is exceeded, forces a
//! `<|halt|>status=partial` regardless of model preference. This is the
//! "Windows 3.1 → 95" jump in design §2 — without it one runaway agent
//! eats the whole machine.
//!
//! v0.5 implements **hard preemption** at budget exhaustion. **Soft
//! preemption** via logit-bias toward halt (when 80% budget used) is
//! deferred to v1.0 because it requires kernel-specific tokenizer info
//! the daemon doesn't currently load. Documented in
//! [`docs/grammar-swap-design.md`] §1 alongside the related work.
//!
//! Multi-task: in v0.5 a single iod process runs one task at a time, so
//! "scheduling" reduces to "is this task over budget?". v1.0's
//! multi-task posture (KV-swap, prompt-prefix isolation) makes the
//! scheduler a true round-robin.

use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct Budget {
    pub wall: Duration,
    pub max_tokens: u64,
    /// Fraction of budget at which soft preemption kicks in. v0.5 only
    /// emits a warning at this point; v1.0 wires the logit-bias path.
    pub soft_preempt_ratio: f64,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            wall: Duration::from_secs(600),
            max_tokens: 32_768,
            soft_preempt_ratio: 0.8,
        }
    }
}

#[derive(Debug)]
pub struct Scheduler {
    budget: Budget,
    started: Instant,
    tokens_used: u64,
    soft_preempt_warned: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerState {
    /// Continue normally.
    Run,
    /// Soft preempt — within budget but past `soft_preempt_ratio`. v1.0
    /// will use this signal to bias toward `<|halt|>`. v0.5 logs once.
    SoftPreempt,
    /// Hard preempt — budget exhausted. The daemon must force a halt.
    HardPreempt {
        reason: PreemptReason,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreemptReason {
    WallBudget,
    TokenBudget,
}

impl Scheduler {
    pub fn new(budget: Budget) -> Self {
        Self {
            budget,
            started: Instant::now(),
            tokens_used: 0,
            soft_preempt_warned: false,
        }
    }

    pub fn tokens_used(&self) -> u64 {
        self.tokens_used
    }

    pub fn wall_elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Account for a freshly-emitted segment. Pass the byte length of the
    /// raw segment text; the daemon uses the same char/4 heuristic as
    /// [`crate::swap`] for token estimation, keeping the two subsystems in
    /// agreement on what "token-used" means.
    pub fn account_segment_bytes(&mut self, bytes: usize) {
        self.tokens_used = self
            .tokens_used
            .saturating_add((bytes / 4 + 1) as u64);
    }

    pub fn check(&mut self) -> SchedulerState {
        let wall_elapsed = self.started.elapsed();
        if wall_elapsed >= self.budget.wall {
            return SchedulerState::HardPreempt {
                reason: PreemptReason::WallBudget,
            };
        }
        if self.tokens_used >= self.budget.max_tokens {
            return SchedulerState::HardPreempt {
                reason: PreemptReason::TokenBudget,
            };
        }

        let wall_ratio =
            wall_elapsed.as_secs_f64() / self.budget.wall.as_secs_f64();
        let token_ratio =
            self.tokens_used as f64 / self.budget.max_tokens as f64;
        if wall_ratio >= self.budget.soft_preempt_ratio
            || token_ratio >= self.budget.soft_preempt_ratio
        {
            if !self.soft_preempt_warned {
                log::warn!(
                    "scheduler: soft-preempt threshold reached (wall {:.0}%, tokens {:.0}%)",
                    wall_ratio * 100.0,
                    token_ratio * 100.0
                );
                self.soft_preempt_warned = true;
            }
            return SchedulerState::SoftPreempt;
        }
        SchedulerState::Run
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn under_budget_returns_run() {
        let mut s = Scheduler::new(Budget {
            wall: Duration::from_secs(60),
            max_tokens: 1000,
            soft_preempt_ratio: 0.8,
        });
        s.account_segment_bytes(40);
        assert!(matches!(s.check(), SchedulerState::Run));
    }

    #[test]
    fn token_budget_exceeded_hard_preempts() {
        let mut s = Scheduler::new(Budget {
            wall: Duration::from_secs(60),
            max_tokens: 10,
            soft_preempt_ratio: 0.8,
        });
        s.account_segment_bytes(200); // 200/4 + 1 = 51 > 10
        assert!(matches!(
            s.check(),
            SchedulerState::HardPreempt {
                reason: PreemptReason::TokenBudget
            }
        ));
    }

    #[test]
    fn wall_budget_exceeded_hard_preempts() {
        let mut s = Scheduler::new(Budget {
            wall: Duration::from_millis(20),
            max_tokens: 1_000_000,
            soft_preempt_ratio: 0.8,
        });
        sleep(Duration::from_millis(50));
        assert!(matches!(
            s.check(),
            SchedulerState::HardPreempt {
                reason: PreemptReason::WallBudget
            }
        ));
    }

    #[test]
    fn soft_preempt_signal_at_threshold() {
        let mut s = Scheduler::new(Budget {
            wall: Duration::from_secs(600),
            max_tokens: 100,
            soft_preempt_ratio: 0.5,
        });
        s.account_segment_bytes(240); // 240/4 + 1 = 61, > 50% of 100
        assert!(matches!(s.check(), SchedulerState::SoftPreempt));
    }
}

//! Deterministic KV cache pager — Phase 2 of the improvement plan.
//!
//! Replaces the LLM-based summarization approach with positional token
//! eviction using anchors. No LLM call during compaction = fully
//! deterministic, < 5ms latency.
//!
//! Layout:
//! ```text
//! [SYSTEM PROMPT | ANCHORS... | EVICTABLE WINDOW | RECENT TAIL]
//!  ^never evict   ^protected   ^evict oldest     ^never evict
//! ```
//!
//! Anchors: ISA state preamble, open loop goals, cartridge schemas.
//! Recent tail: last N tokens (configurable, default 2048).
//! Evictable: everything between anchors and tail.

use crate::swap::IsaState;
use serde::{Deserialize, Serialize};

/// A range of token positions in the KV cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenRange {
    /// Human-readable label for debugging.
    pub label: String,
    /// Start position (inclusive).
    pub start: i32,
    /// End position (exclusive).
    pub end: i32,
}

impl TokenRange {
    pub fn len(&self) -> i32 {
        (self.end - self.start).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Configuration for the KV pager.
#[derive(Debug, Clone)]
pub struct PagerConfig {
    /// Fraction of context window that triggers compaction (default: 0.70).
    pub threshold: f64,
    /// Fraction of evictable window to drop per compaction (default: 0.25).
    pub eviction_ratio: f64,
    /// Number of tokens to always preserve at the tail (default: 2048).
    pub tail_size: i32,
    /// Minimum number of tokens in system prompt to protect (default: 256).
    pub system_prompt_size: i32,
}

impl Default for PagerConfig {
    fn default() -> Self {
        Self {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 2048,
            system_prompt_size: 256,
        }
    }
}

/// The KV pager manages compaction without LLM involvement.
pub struct KvPager {
    config: PagerConfig,
    /// Registered anchor ranges that must never be evicted.
    anchors: Vec<TokenRange>,
    /// Total context window size (from model params).
    n_ctx: i32,
    /// Position of the ISA state preamble anchor (updated on compaction).
    state_anchor: Option<TokenRange>,
    /// Number of compactions performed.
    compaction_count: u32,
}

impl KvPager {
    /// Create a new pager for a given context window size.
    pub fn new(n_ctx: u32, config: PagerConfig) -> Self {
        Self {
            config,
            anchors: Vec::new(),
            n_ctx: n_ctx as i32,
            state_anchor: None,
            compaction_count: 0,
        }
    }

    /// Check if compaction is needed based on current KV usage.
    pub fn should_compact(&self, used_cells: i32) -> bool {
        let ratio = used_cells as f64 / self.n_ctx as f64;
        ratio > self.config.threshold
    }

    /// Register an anchor range that must survive compaction.
    pub fn register_anchor(&mut self, label: &str, start: i32, end: i32) {
        self.anchors.push(TokenRange {
            label: label.to_string(),
            start,
            end,
        });
    }

    /// Remove anchors that are no longer relevant (e.g., closed loops).
    pub fn remove_anchor(&mut self, label: &str) {
        self.anchors.retain(|a| a.label != label);
    }

    /// Compute the eviction plan: which positions to remove from KV cache.
    ///
    /// Returns the range [evict_start, evict_end) that should be passed to
    /// `kv_cache_seq_rm()`. Returns None if no eviction is possible.
    pub fn compute_eviction(&self, current_pos: i32) -> Option<EvictionPlan> {
        // Protected regions:
        // 1. System prompt: [0, system_prompt_size)
        // 2. All anchors
        // 3. Recent tail: [current_pos - tail_size, current_pos)

        let system_end = self.config.system_prompt_size;
        let tail_start = (current_pos - self.config.tail_size).max(system_end);

        // The evictable window is between the last anchor (or system_end)
        // and the tail_start.
        let evictable_start = self.evictable_start(system_end);
        let evictable_end = tail_start;

        if evictable_end <= evictable_start {
            // Nothing to evict — the tail and anchors overlap.
            return None;
        }

        let evictable_len = evictable_end - evictable_start;
        let evict_amount = (evictable_len as f64 * self.config.eviction_ratio).ceil() as i32;
        let evict_amount = evict_amount.max(1);

        // Evict from the oldest part of the evictable window.
        let evict_end = (evictable_start + evict_amount).min(evictable_end);

        Some(EvictionPlan {
            evict_start: evictable_start,
            evict_end,
            evicted_tokens: evict_end - evictable_start,
            evictable_total: evictable_len,
        })
    }

    /// Execute a compaction using the provided ISA state.
    ///
    /// This method:
    /// 1. Computes the eviction plan
    /// 2. Returns the plan (caller does the actual KV cache removal)
    /// 3. Updates the ISA state anchor
    ///
    /// The caller is responsible for:
    /// - Calling `ctx.kv_cache_seq_rm(0, plan.evict_start, plan.evict_end)`
    /// - Injecting the ISA state preamble tokens at the appropriate position
    pub fn compact(&mut self, current_pos: i32, isa_state: &IsaState) -> Option<CompactionResult> {
        let plan = self.compute_eviction(current_pos)?;

        // Update state anchor
        if isa_state.is_nontrivial() {
            let preamble = isa_state.to_preamble();
            self.state_anchor = Some(TokenRange {
                label: "isa_state".to_string(),
                start: plan.evict_start, // Will be injected at the eviction boundary
                end: plan.evict_start + estimate_token_count(&preamble),
            });
        } else {
            self.state_anchor = None;
        }

        self.compaction_count += 1;

        // Remove any anchors that fall within the evicted range
        self.anchors.retain(|a| {
            // Keep anchors that are entirely outside the eviction range
            a.end <= plan.evict_start || a.start >= plan.evict_end
        });

        Some(CompactionResult {
            plan,
            preamble: if isa_state.is_nontrivial() {
                Some(isa_state.to_preamble())
            } else {
                None
            },
            compaction_number: self.compaction_count,
        })
    }

    /// Get the number of compactions performed.
    pub fn compaction_count(&self) -> u32 {
        self.compaction_count
    }

    /// Find the start of the evictable window (after system prompt and anchors).
    fn evictable_start(&self, system_end: i32) -> i32 {
        let mut start = system_end;

        // Skip past any anchors in the system region
        for anchor in &self.anchors {
            if anchor.start <= start && anchor.end > start {
                start = anchor.end;
            }
        }

        // Also skip past the state anchor
        if let Some(ref sa) = self.state_anchor {
            if sa.start <= start && sa.end > start {
                start = sa.end;
            }
        }

        start
    }
}

/// Plan for what to evict from the KV cache.
#[derive(Debug, Clone)]
pub struct EvictionPlan {
    /// Start position of the eviction (inclusive).
    pub evict_start: i32,
    /// End position of the eviction (exclusive).
    pub evict_end: i32,
    /// Number of tokens that will be evicted.
    pub evicted_tokens: i32,
    /// Total evictable tokens available.
    pub evictable_total: i32,
}

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// The eviction plan (what was removed).
    pub plan: EvictionPlan,
    /// ISA state preamble to inject after eviction (if state is nontrivial).
    pub preamble: Option<String>,
    /// Which compaction this is (1-indexed).
    pub compaction_number: u32,
}

/// Estimate token count from string length (char/4 + 1 heuristic).
fn estimate_token_count(text: &str) -> i32 {
    (text.len() as i32 / 4) + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_compact_respects_threshold() {
        let pager = KvPager::new(1000, PagerConfig::default());
        assert!(!pager.should_compact(500)); // 50% < 70%
        assert!(!pager.should_compact(700)); // 70% = 70% (not >)
        assert!(pager.should_compact(701)); // 70.1% > 70%
        assert!(pager.should_compact(900)); // 90% > 70%
    }

    #[test]
    fn eviction_plan_basic() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 200,
            system_prompt_size: 100,
        };
        let pager = KvPager::new(1000, config);

        // Current pos = 800. Evictable = [100, 600). Evict 25% of 500 = 125.
        let plan = pager.compute_eviction(800).unwrap();
        assert_eq!(plan.evict_start, 100);
        assert_eq!(plan.evict_end, 225); // 100 + 125
        assert_eq!(plan.evicted_tokens, 125);
        assert_eq!(plan.evictable_total, 500);
    }

    #[test]
    fn eviction_plan_none_when_tail_overlaps() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 900, // Tail is almost the whole window
            system_prompt_size: 100,
        };
        let pager = KvPager::new(1000, config);

        // Current pos = 500. Tail starts at max(500-900, 100) = 100.
        // Evictable = [100, 100) = empty.
        let plan = pager.compute_eviction(500);
        assert!(plan.is_none());
    }

    #[test]
    fn anchors_adjust_evictable_start() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 200,
            system_prompt_size: 100,
        };
        let mut pager = KvPager::new(1000, config);
        pager.register_anchor("loop_header", 100, 150);

        // Evictable starts after the anchor: [150, 600)
        let plan = pager.compute_eviction(800).unwrap();
        assert_eq!(plan.evict_start, 150);
        assert_eq!(plan.evictable_total, 450); // 600 - 150
    }

    #[test]
    fn compact_with_nontrivial_state() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 200,
            system_prompt_size: 100,
        };
        let mut pager = KvPager::new(1000, config);

        let mut isa_state = IsaState::default();
        isa_state.loop_depth = 2;
        isa_state.open_loops = vec!["navigate".into(), "scan".into()];

        let result = pager.compact(800, &isa_state).unwrap();
        assert!(result.preamble.is_some());
        let preamble = result.preamble.unwrap();
        assert!(preamble.contains("<|state|>"));
        assert!(preamble.contains("\"loop_depth\":2"));
        assert_eq!(result.compaction_number, 1);
    }

    #[test]
    fn compact_with_trivial_state_no_preamble() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.25,
            tail_size: 200,
            system_prompt_size: 100,
        };
        let mut pager = KvPager::new(1000, config);

        let isa_state = IsaState::default(); // trivial: all zeros/empty

        let result = pager.compact(800, &isa_state).unwrap();
        assert!(result.preamble.is_none());
        assert_eq!(result.compaction_number, 1);
    }

    #[test]
    fn remove_anchor_works() {
        let config = PagerConfig::default();
        let mut pager = KvPager::new(1000, config);
        pager.register_anchor("loop_1", 100, 120);
        pager.register_anchor("loop_2", 200, 220);
        assert_eq!(pager.anchors.len(), 2);

        pager.remove_anchor("loop_1");
        assert_eq!(pager.anchors.len(), 1);
        assert_eq!(pager.anchors[0].label, "loop_2");
    }

    #[test]
    fn compaction_removes_stale_anchors() {
        let config = PagerConfig {
            threshold: 0.70,
            eviction_ratio: 0.50, // evict 50%
            tail_size: 200,
            system_prompt_size: 100,
        };
        let mut pager = KvPager::new(1000, config);
        pager.register_anchor("old_loop", 120, 140); // inside eviction range
        pager.register_anchor("recent_loop", 500, 520); // outside eviction range

        let isa_state = IsaState::default();
        let _result = pager.compact(800, &isa_state).unwrap();

        // "old_loop" at [120,140) should be evicted (eviction range starts at 100)
        // "recent_loop" at [500,520) should survive
        assert_eq!(pager.anchors.len(), 1);
        assert_eq!(pager.anchors[0].label, "recent_loop");
    }

    #[test]
    fn token_range_len() {
        let r = TokenRange { label: "test".into(), start: 10, end: 20 };
        assert_eq!(r.len(), 10);
        assert!(!r.is_empty());

        let empty = TokenRange { label: "empty".into(), start: 10, end: 10 };
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
    }
}

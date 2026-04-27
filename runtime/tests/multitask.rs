//! Multi-task integration test.
//!
//! Two tasks running concurrently against one mock server, each using a
//! distinct `id_slot`. Verifies that:
//!   - Both tasks complete.
//!   - The mock received bodies tagged with both slot ids.
//!   - No deadlock with shared cartridge state (sim_world's mutex).

use llm_os_runtime::iod::DaemonConfig;
use llm_os_runtime::mock_server::{MockServer, ScriptedResponse};
use llm_os_runtime::multitask::{from_goals, run_all};
use llm_os_runtime::parser::HaltStatus;
use std::time::Duration;

fn boot_mock(mock: &MockServer) -> u16 {
    let port = mock.start();
    std::thread::sleep(Duration::from_millis(80));
    port
}

#[test]
fn two_tasks_run_concurrently_against_slot_pool() {
    let mock = MockServer::new();
    // Each task makes one segment then halts — push 4 completion responses
    // (2 per task). Order doesn't matter since the mock pops FIFO and the
    // tasks race; what matters is total count and that both finish.
    for _ in 0..4 {
        mock.push(ScriptedResponse::completion(
            "<|halt|>status=success\n",
        ));
    }
    let port = boot_mock(&mock);

    let cfg = DaemonConfig {
        server_url: format!("http://127.0.0.1:{port}"),
        grammar_path: "../grammar/isa.gbnf".into(),
        cart_root: "../cart".into(),
        task_budget: Duration::from_secs(30),
        max_loop_depth: 4,
        temperature: 0.0,
        max_predict_per_segment: 256,
        trace_path: None,
        max_tokens_per_task: 100_000,
        slot_id: None,
        ollama: false,
        model: String::new(),
    };

    let tasks = from_goals(vec!["task A".into(), "task B".into()], cfg);
    let results = run_all(tasks).expect("run_all");

    assert_eq!(results.len(), 2);
    for r in &results {
        assert!(
            r.error.is_none(),
            "task '{}' failed: {:?}",
            r.goal,
            r.error
        );
        let outcome = r.outcome.as_ref().expect("outcome present");
        assert_eq!(outcome.status, HaltStatus::Success);
    }

    // Slot ids should be 0 and 1.
    let mut slots: Vec<i32> = results.iter().map(|r| r.slot_id).collect();
    slots.sort();
    assert_eq!(slots, vec![0, 1]);

    // Mock should have received at least 2 POST bodies (one per task).
    let bodies = mock.received_bodies();
    assert!(bodies.len() >= 2, "expected ≥2 POST bodies, got {}", bodies.len());

    // Each request should have included its slot id.
    let slot_0_bodies = bodies.iter().filter(|b| b.contains("\"id_slot\":0")).count();
    let slot_1_bodies = bodies.iter().filter(|b| b.contains("\"id_slot\":1")).count();
    assert!(slot_0_bodies >= 1, "no body with id_slot:0");
    assert!(slot_1_bodies >= 1, "no body with id_slot:1");
}

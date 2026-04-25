//! End-to-end integration tests against the mock server.
//!
//! These tests build a real `Daemon` (with the real grammar file + real
//! cartridge registry) and point it at an ephemeral mock llama-server.
//! No actual model required — the mock returns scripted ISA emissions.
//!
//! Tests are sequential by default (cargo's integration-test runner uses
//! one thread per test file); each test has its own MockServer instance
//! so port allocation is independent.

use llm_os_runtime::iod::{Daemon, DaemonConfig};
use llm_os_runtime::mock_server::{MockServer, ScriptedResponse};
use llm_os_runtime::parser::HaltStatus;
use std::time::Duration;

fn make_config(port: u16) -> DaemonConfig {
    DaemonConfig {
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
    }
}

fn boot_mock(mock: &MockServer) -> u16 {
    let port = mock.start();
    // Tiny grace period so the listener is accepting before the daemon connects.
    std::thread::sleep(Duration::from_millis(80));
    port
}

#[test]
fn simple_halt_completes_with_success() {
    let mock = MockServer::new();
    mock.push(ScriptedResponse::completion(
        "<|halt|>status=success\n",
    ));
    let port = boot_mock(&mock);

    let daemon = Daemon::new(make_config(port)).expect("daemon init");
    let outcome = daemon.run_task("trivial halt").expect("run_task");
    assert_eq!(outcome.status, HaltStatus::Success);
    assert_eq!(outcome.steps, 1);
}

#[test]
fn read_then_halt_injects_result_block_into_next_segment() {
    let mock = MockServer::new();
    mock.push(ScriptedResponse::completion(
        "<|read|>fd=0 len=0\n",
    ));
    mock.push(ScriptedResponse::completion(
        "<|halt|>status=success\n",
    ));
    let port = boot_mock(&mock);

    let daemon = Daemon::new(make_config(port)).expect("daemon init");
    let outcome = daemon.run_task("read fd 0 then halt").expect("run_task");
    assert_eq!(outcome.status, HaltStatus::Success);
    // 2 model statements: read + halt.
    assert_eq!(outcome.steps, 2);

    // Daemon should have made 2 POST /v1/completions requests; the second
    // one's prompt should include the `<|result|>` injected after read.
    let bodies = mock.received_bodies();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies[1].contains("<|result|>"),
        "second segment prompt missing <|result|>: {}",
        bodies[1]
    );
}

#[test]
fn call_demo_echo_dispatches_to_real_handler() {
    let mock = MockServer::new();
    mock.push(ScriptedResponse::completion(
        "<|call|>demo.echo {\"text\":\"hello\"} <|/call|>\n",
    ));
    mock.push(ScriptedResponse::completion(
        "<|halt|>status=success\n",
    ));
    let port = boot_mock(&mock);

    let daemon = Daemon::new(make_config(port)).expect("daemon init");
    let outcome = daemon.run_task("echo hello via demo").expect("run_task");
    assert_eq!(outcome.status, HaltStatus::Success);

    // The injected result for demo.echo is `{"text":"hello","len":5}` —
    // verify the daemon got that into the next segment's prompt.
    let bodies = mock.received_bodies();
    assert_eq!(bodies.len(), 2);
    assert!(
        bodies[1].contains("\\\"text\\\":\\\"hello\\\""),
        "second segment missing echo result: {}",
        bodies[1]
    );
    // Demo handler returns len = 5 for "hello".
    assert!(bodies[1].contains("\\\"len\\\":5"), "len field missing: {}", bodies[1]);
}

#[test]
fn schema_violation_surfaces_expected_grammar_in_result() {
    let mock = MockServer::new();
    // demo.echo's args_schema requires `text` (string). Send int instead.
    mock.push(ScriptedResponse::completion(
        "<|call|>demo.echo {\"text\":42} <|/call|>\n",
    ));
    mock.push(ScriptedResponse::completion(
        "<|halt|>status=failure\n",
    ));
    let port = boot_mock(&mock);

    let daemon = Daemon::new(make_config(port)).expect("daemon init");
    let outcome = daemon.run_task("trigger schema violation").expect("run_task");
    assert_eq!(outcome.status, HaltStatus::Failure);

    let bodies = mock.received_bodies();
    assert_eq!(bodies.len(), 2);
    let second = &bodies[1];
    assert!(
        second.contains("schema_violation"),
        "expected schema_violation in second segment: {second}"
    );
    // v0.5: the compiled GBNF should be surfaced in the result so the
    // model sees what it should have emitted.
    assert!(
        second.contains("expected_grammar"),
        "expected_grammar field missing from violation result: {second}"
    );
}

#[test]
fn token_budget_exceeded_force_halts_partial() {
    let mock = MockServer::new();
    // Each chunk eats budget; with max_tokens_per_task tiny the daemon
    // should preempt before the first model-emitted halt.
    let big_chunk: String = "<|think|>".to_string()
        + &"x".repeat(2000)
        + "<|/think|>\n";
    // Push enough chunks to exhaust budget.
    for _ in 0..10 {
        mock.push(ScriptedResponse::completion(big_chunk.clone()));
    }
    let port = boot_mock(&mock);

    let mut cfg = make_config(port);
    cfg.max_tokens_per_task = 100; // very small
    let daemon = Daemon::new(cfg).expect("daemon init");
    let outcome = daemon.run_task("eat budget").expect("run_task");
    // Scheduler hard-preempted → partial.
    assert_eq!(outcome.status, HaltStatus::Partial);
}

#[test]
fn roclaw_call_compiles_to_bytecode_via_real_handler() {
    let mock = MockServer::new();
    mock.push(ScriptedResponse::completion(
        "<|call|>roclaw.forward {\"left\":150,\"right\":150} <|/call|>\n",
    ));
    mock.push(ScriptedResponse::completion(
        "<|halt|>status=success\n",
    ));
    let port = boot_mock(&mock);

    let daemon = Daemon::new(make_config(port)).expect("daemon init");
    let outcome = daemon.run_task("forward 150 150").expect("run_task");
    assert_eq!(outcome.status, HaltStatus::Success);

    let bodies = mock.received_bodies();
    assert_eq!(bodies.len(), 2);
    let second = &bodies[1];
    // The roclaw handler returns the hex of the encoded frame.
    // forward(150, 150) → AA 01 96 96 01 FF (per design §1.3)
    assert!(
        second.contains("AA 01 96 96 01 FF"),
        "roclaw result hex missing from second segment: {second}"
    );
    // bytes:6 envelope.
    assert!(second.contains("\\\"bytes\\\":6"));
}

use mockito::Server;
use quantamind_lib::commands::prompt::prompt::run_prompt_inner;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::token_handler::make_token_handler;
use quantamind_lib::metrics::timing::RunTiming;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// F1: when the per-token emit closure fails (e.g. window closed), the run
/// must cancel within one token and the recorded token_count must match the
/// number of *successful* emits, not the number of attempts.
#[tokio::test]
async fn emit_failure_cancels_stream_and_metrics_match_real_emits() {
    let mut server = Server::new_async().await;
    let body = concat!(
        "data: {\"choices\":[{\"delta\":{\"content\":\"A\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"B\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"C\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{\"content\":\"D\"},\"finish_reason\":null}]}\n\n",
        "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    let _mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(body)
        .create_async()
        .await;

    let cancel = CancellationToken::new();
    let timing = Arc::new(Mutex::new(RunTiming::start()));
    let attempts = Arc::new(AtomicUsize::new(0));
    let attempts_inner = attempts.clone();

    let fake_emit = move |_t: &str| -> Result<(), ()> {
        let n = attempts_inner.fetch_add(1, Ordering::SeqCst) + 1;
        if n == 3 { Err(()) } else { Ok(()) }
    };
    let handler = make_token_handler(fake_emit, cancel.clone(), timing.clone());

    let result = run_prompt_inner(BackendKind::LlamaCpp, &server.url(), "m", "p", None, None, None, cancel.clone(), handler).await;

    assert!(result.is_ok(), "stream should exit cleanly on emit-failure cancel");
    assert!(cancel.is_cancelled(), "emit failure must have triggered cancel");

    let total_attempts = attempts.load(Ordering::SeqCst);
    assert_eq!(
        total_attempts, 3,
        "stream must stop at the failing emit; got {total_attempts} attempts (expected exactly 3)"
    );

    let recorded = timing.lock().unwrap().token_count;
    assert_eq!(
        recorded, 2,
        "token_count must reflect successful emits (2), not attempts ({total_attempts}); got {recorded}"
    );
}

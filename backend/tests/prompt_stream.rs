use mockito::Server;
use quantamind_lib::commands::prompt::prompt::run_prompt_inner;
use quantamind_lib::errors::AppError;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use tokio_util::sync::CancellationToken;

/// llama-server's PRIMARY path is the templated `/v1/chat/completions` endpoint,
/// streamed as OpenAI SSE `data: {json}` events.
const SSE_BODY: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"The \"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"sky \"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"is \"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"content\":\"blue.\"},\"finish_reason\":null}]}\n\n",
    "data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
);

#[tokio::test]
async fn tokens_arrive_in_order_and_concat_to_fixture() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .with_status(200)
        .with_body(SSE_BODY)
        .create_async()
        .await;

    let mut tokens: Vec<String> = Vec::new();
    run_prompt_inner(
        BackendKind::LlamaCpp,
        &server.url(),
        "phi3-mini",
        "Why is the sky blue?",
        None,
        None,
        None,
        CancellationToken::new(),
        |t| tokens.push(t.to_string()),
    )
    .await
    .unwrap();

    mock.assert_async().await;
    assert_eq!(tokens, vec!["The ", "sky ", "is ", "blue."]);
    assert_eq!(tokens.concat(), "The sky is blue.");
}

#[tokio::test]
async fn empty_prompt_rejected_before_http() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .expect(0)
        .create_async()
        .await;

    match run_prompt_inner(
        BackendKind::LlamaCpp,
        &server.url(),
        "phi3-mini",
        "   ",
        None,
        None,
        None,
        CancellationToken::new(),
        |_| {},
    )
    .await
    {
        Err(AppError::Validation(msg)) => assert!(msg.contains("prompt"), "msg: {msg}"),
        other => panic!("expected Validation err, got {other:?}"),
    }
    mock.assert_async().await;
}

#[tokio::test]
async fn empty_model_rejected_before_http() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("POST", "/v1/chat/completions")
        .expect(0)
        .create_async()
        .await;

    match run_prompt_inner(BackendKind::LlamaCpp, &server.url(), "", "hi", None, None, None, CancellationToken::new(), |_| {}).await {
        Err(AppError::Validation(msg)) => assert!(msg.contains("model"), "msg: {msg}"),
        other => panic!("expected Validation err, got {other:?}"),
    }
    mock.assert_async().await;
}

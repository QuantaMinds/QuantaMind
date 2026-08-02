use mockito::Server;
use quantamind_lib::commands::eval::eval_run::run_and_score;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::eval::eval_task::{EvalTask, Scoring};
use std::collections::BTreeMap;

fn server_ndjson(text: &str) -> String {
    format!("{{\"response\":\"{text}\",\"done\":false}}\n{{\"response\":\"\",\"done\":true}}\n")
}

fn task(scoring: Scoring) -> EvalTask {
    EvalTask { id: "t".into(), category: "c".into(), prompt: "p".into(), scoring }
}

#[tokio::test]
async fn run_and_score_passes_an_exact_task_on_matching_output() {
    let mut server = Server::new_async().await;
    let _m = server.mock("POST", "/api/generate")
        .with_status(200).with_body(server_ndjson("POSITIVE")).create_async().await;

    let r = run_and_score(BackendKind::LlamaCpp, &server.url(), "m",
        &task(Scoring::Exact { expected: "POSITIVE".into() })).await.unwrap();
    assert!(r.passed);
    assert_eq!(r.output, "POSITIVE");
    assert!(r.token_count >= 1);
}

#[tokio::test]
async fn run_and_score_fails_an_exact_task_on_wrong_output() {
    let mut server = Server::new_async().await;
    let _m = server.mock("POST", "/api/generate")
        .with_status(200).with_body(server_ndjson("NEGATIVE")).create_async().await;

    let r = run_and_score(BackendKind::LlamaCpp, &server.url(), "m",
        &task(Scoring::Exact { expected: "POSITIVE".into() })).await.unwrap();
    assert!(!r.passed);
}

#[tokio::test]
async fn run_and_score_validates_a_json_schema_task() {
    let mut server = Server::new_async().await;
    // response value is the JSON object (escaped for the NDJSON envelope).
    let body = "{\"response\":\"{\\\"name\\\":\\\"get_weather\\\",\\\"args\\\":{}}\",\"done\":false}\n\
                {\"response\":\"\",\"done\":true}\n";
    let _m = server.mock("POST", "/api/generate")
        .with_status(200).with_body(body).create_async().await;

    let scoring = Scoring::JsonSchema {
        required: vec!["name".into(), "args".into()],
        types: BTreeMap::from([("name".into(), "string".into()), ("args".into(), "object".into())]),
    };
    let r = run_and_score(BackendKind::LlamaCpp, &server.url(), "m", &task(scoring)).await.unwrap();
    assert!(r.passed, "detail: {}", r.detail);
}

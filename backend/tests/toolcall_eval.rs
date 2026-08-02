use mockito::Server;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::eval::toolcall::eval::run_eval;
use quantamind_lib::inference::eval::toolcall::tasks::{Call, Expected, ToolSchema, ToolTask};
use serde_json::json;

fn ndjson(completion: &str) -> String {
    let chunk = json!({ "response": completion, "done": false }).to_string();
    let end = json!({ "response": "", "done": true }).to_string();
    format!("{chunk}\n{end}\n")
}

fn weather_task() -> ToolTask {
    ToolTask {
        id: "w".into(),
        category: "single".into(),
        prompt: "Weather in Paris?".into(),
        tools: vec![ToolSchema {
            name: "get_weather".into(),
            description: "Get weather".into(),
            parameters: json!({ "type": "object", "properties": { "city": { "type": "string" } }, "required": ["city"] }),
        }],
        expected: Expected::Call(Call { name: "get_weather".into(), args: json!({ "city": "Paris" }) }),
    }
}

#[tokio::test]
async fn all_correct_scores_100() {
    let mut server = Server::new_async().await;
    let _m = server.mock("POST", "/api/generate").with_status(200)
        .with_body(ndjson("{\"name\":\"get_weather\",\"args\":{\"city\":\"Paris\"}}"))
        .create_async().await;

    let r = run_eval(BackendKind::LlamaCpp, &server.url(), "m", &[weather_task()]).await.unwrap();
    assert_eq!(r.parse_rate, Some(1.0));
    assert_eq!(r.tool_selection_acc, Some(1.0));
    assert_eq!(r.arg_acc, Some(1.0));
    assert_eq!(r.composite, Some(1.0));
}

#[tokio::test]
async fn prose_only_gives_zero_parse_rate_not_a_crash() {
    let mut server = Server::new_async().await;
    let _m = server.mock("POST", "/api/generate").with_status(200)
        .with_body(ndjson("I'm not sure what you mean, could you clarify?"))
        .create_async().await;

    let r = run_eval(BackendKind::LlamaCpp, &server.url(), "m", &[weather_task()]).await.unwrap();
    assert_eq!(r.parse_rate, Some(0.0));
    assert_eq!(r.tool_selection_acc, None); // no parsed call-tasks → n/a, never fabricated
    assert_eq!(r.composite, Some(0.0)); // mean of [0.0]
}

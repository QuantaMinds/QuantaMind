use super::*;

#[test]
fn validation_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::Validation("empty".into())).unwrap();
    assert_eq!(json, r#"{"kind":"validation","message":"empty"}"#);
}

#[test]
fn not_found_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::NotFound("model x".into())).unwrap();
    assert_eq!(json, r#"{"kind":"not_found","message":"model x"}"#);
}

#[test]
fn internal_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::Internal("boom".into())).unwrap();
    assert_eq!(json, r#"{"kind":"internal","message":"boom"}"#);
}

#[test]
fn display_format_matches_thiserror_attr() {
    assert_eq!(format!("{}", AppError::Validation("x".into())), "validation: x");
}

#[test]
fn timeout_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::Timeout("list_models after 5s".into())).unwrap();
    assert_eq!(json, r#"{"kind":"timeout","message":"list_models after 5s"}"#);
}

#[test]
fn auth_required_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::AuthRequired("meta-llama/Llama-3".into())).unwrap();
    assert_eq!(json, r#"{"kind":"auth_required","message":"meta-llama/Llama-3"}"#);
}

#[test]
fn invalid_task_schema_serializes_as_tagged_json() {
    let json = serde_json::to_string(&AppError::InvalidTaskSchema("task 'w': empty tools".into())).unwrap();
    assert_eq!(json, r#"{"kind":"invalid_task_schema","message":"task 'w': empty tools"}"#);
}

#[test]
fn from_io_error_maps_to_io() {
    let e: AppError = std::io::Error::new(std::io::ErrorKind::NotFound, "missing").into();
    assert!(matches!(e, AppError::Io(_)));
}

#[test]
fn from_serde_error_maps_to_invalid_task_schema() {
    let err = serde_json::from_str::<i32>("not json").unwrap_err();
    let e: AppError = err.into();
    assert!(matches!(e, AppError::InvalidTaskSchema(_)));
}

#[test]
fn friendly_maps_connection_refused_to_server_down() {
    let e = AppError::Inference("error trying to connect: Connection refused".into());
    assert_eq!(e.friendly(), "The local server is not running. Start it and try again.");
}

#[test]
fn friendly_maps_model_not_found() {
    let e = AppError::Inference("model 'llama3' not found, try pulling it".into());
    assert!(e.friendly().contains("isn't installed"));
}

#[test]
fn friendly_maps_out_of_memory() {
    let e = AppError::Inference("llama runner: out of memory".into());
    assert!(e.friendly().contains("Not enough memory"));
}

#[test]
fn friendly_passes_through_unknown() {
    let e = AppError::Validation("weird thing".into());
    assert_eq!(e.friendly(), "validation: weird thing");
}

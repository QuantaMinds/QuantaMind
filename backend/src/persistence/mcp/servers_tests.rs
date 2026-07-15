use super::*;
use tempfile::tempdir;

fn sample() -> McpServerConfig {
    McpServerConfig {
        id: "filesystem".into(),
        command: "npx".into(),
        args: vec!["-y".into(), "@modelcontextprotocol/server-filesystem".into()],
        env_keys: vec![],
        roots: vec![],
        enabled: true,
    }
}

#[test]
fn save_then_load_round_trips() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mcp_servers.yaml");
    let reg = McpRegistry { servers: vec![sample()] };
    save(&path, &reg).unwrap();
    assert_eq!(load(&path).unwrap(), reg);
}

#[test]
fn missing_file_loads_empty() {
    let dir = tempdir().unwrap();
    assert_eq!(load(&dir.path().join("nope.yaml")).unwrap(), McpRegistry::default());
}

#[test]
fn omitted_enabled_defaults_true() {
    let reg: McpRegistry =
        serde_yaml::from_str("servers:\n  - id: fs\n    command: npx\n").unwrap();
    assert!(reg.servers[0].enabled, "an omitted `enabled` means enabled");
}

#[test]
fn env_values_are_never_persisted_only_names() {
    // The struct carries no secret VALUE field at all — only env_keys (names).
    let dir = tempdir().unwrap();
    let path = dir.path().join("mcp_servers.yaml");
    let mut cfg = sample();
    cfg.env_keys = vec!["GITHUB_TOKEN".into()];
    save(&path, &McpRegistry { servers: vec![cfg] }).unwrap();
    let yaml = std::fs::read_to_string(&path).unwrap();
    assert!(yaml.contains("GITHUB_TOKEN"), "the name is stored");
    // (there is no value to leak; this documents the invariant)
}

#[test]
fn validate_rejects_empty_and_duplicate_ids_and_empty_command() {
    let empty_id = McpRegistry { servers: vec![McpServerConfig { id: " ".into(), ..sample() }] };
    assert!(validate(&empty_id).is_err());

    let dup = McpRegistry { servers: vec![sample(), sample()] };
    assert!(validate(&dup).is_err(), "duplicate ids rejected");

    let no_cmd = McpRegistry { servers: vec![McpServerConfig { command: "".into(), ..sample() }] };
    assert!(validate(&no_cmd).is_err());
}

#[test]
fn canonical_roots_resolves_dirs_and_rejects_bad_ones() {
    let dir = tempdir().unwrap();
    let real = dir.path().join("world");
    std::fs::create_dir(&real).unwrap();
    let file = dir.path().join("a_file.txt");
    std::fs::write(&file, "x").unwrap();

    let ok = McpServerConfig { roots: vec![real.to_str().unwrap().into()], ..sample() };
    let resolved = ok.canonical_roots().unwrap();
    assert_eq!(resolved.len(), 1);
    assert!(resolved[0].is_absolute());

    let missing = McpServerConfig { roots: vec![dir.path().join("nope").to_str().unwrap().into()], ..sample() };
    assert!(missing.canonical_roots().is_err(), "nonexistent root rejected");

    let not_dir = McpServerConfig { roots: vec![file.to_str().unwrap().into()], ..sample() };
    assert!(not_dir.canonical_roots().is_err(), "a file is not a valid root");
}

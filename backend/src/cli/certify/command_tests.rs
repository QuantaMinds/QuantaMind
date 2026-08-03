use super::*;
use std::path::{Path, PathBuf};

fn argv(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|s| s.to_string()).collect()
}

struct Paths {
    ws: PathBuf,
    task_file: PathBuf,
    otel: PathBuf,
    db: PathBuf,
}
fn paths() -> Paths {
    Paths {
        ws: PathBuf::from("/tmp/qm-cert-1-0/workspace"),
        task_file: PathBuf::from("/tmp/qm-cert-1-0/task.json"),
        otel: PathBuf::from("/tmp/qm-cert-1-0/otel"),
        db: PathBuf::from("/tmp/qm-cert-1-0/workspace/data.db"),
    }
}

fn ctx<'a>(p: &'a Paths, goal: &'a str, db: Option<&'a Path>) -> AttemptContext<'a> {
    AttemptContext {
        task_id: "t-1",
        goal,
        workspace: &p.ws,
        task_file: &p.task_file,
        otel_dir: &p.otel,
        db,
        attempt: 1,
    }
}

#[test]
fn placeholders_substitute_inside_a_larger_argument() {
    let c = AgentCommand::new(&argv(&["agent", "--dir={workspace}", "--prompt={task}"]), false, vec![]).unwrap();
    let p = paths();
    let got = c.argv_for(&ctx(&p, "do the thing", None)).unwrap();
    assert_eq!(got, vec!["--dir=/tmp/qm-cert-1-0/workspace", "--prompt=do the thing"]);
}

#[test]
fn a_whole_argument_placeholder_substitutes_too() {
    let c = AgentCommand::new(&argv(&["agent", "--task", "{task}", "--ws", "{workspace}"]), false, vec![]).unwrap();
    let p = paths();
    let got = c.argv_for(&ctx(&p, "goal text", None)).unwrap();
    assert_eq!(got, vec!["--task", "goal text", "--ws", "/tmp/qm-cert-1-0/workspace"]);
}

/// THE INJECTION TEST. argv is a `Vec<String>` handed to `Command`, never a string
/// handed to a shell — so shell metacharacters in task data are inert. If this ever
/// regresses to a `sh -c` string, a task author could execute arbitrary commands on
/// every machine that runs the suite.
#[test]
fn shell_metacharacters_in_the_task_stay_one_inert_argument() {
    let c = AgentCommand::new(&argv(&["agent", "--prompt", "{task}"]), false, vec![]).unwrap();
    let p = paths();
    let evil = "; rm -rf / #";
    let got = c.argv_for(&ctx(&p, evil, None)).unwrap();
    assert_eq!(got.len(), 2, "must remain exactly two arguments: {got:?}");
    assert_eq!(got[1], evil, "verbatim, unsplit, uninterpreted");
}

#[test]
fn command_substitution_and_pipes_in_the_task_are_also_inert() {
    let c = AgentCommand::new(&argv(&["agent", "{task}"]), false, vec![]).unwrap();
    let p = paths();
    for evil in ["$(whoami)", "`id`", "a | b", "a && b", "$HOME", "%PATH%"] {
        let got = c.argv_for(&ctx(&p, evil, None)).unwrap();
        assert_eq!(got, vec![evil.to_string()], "{evil:?} must pass through untouched");
    }
}

#[test]
fn an_unknown_placeholder_is_a_config_error_naming_it() {
    let e = AgentCommand::new(&argv(&["agent", "--x={nope}"]), false, vec![]).unwrap_err();
    assert_eq!(e, CommandError::UnknownPlaceholder("{nope}".into()));
    assert!(e.to_string().contains("{nope}"), "the message must name the offender");
}

/// A placeholder in argv[0] would let task data choose the executable.
#[test]
fn a_placeholder_in_the_program_name_is_refused() {
    let e = AgentCommand::new(&argv(&["{task}", "--x"]), false, vec![]).unwrap_err();
    assert_eq!(e, CommandError::PlaceholderInProgram("{task}".into()));
}

#[test]
fn an_empty_command_is_refused() {
    assert_eq!(AgentCommand::new(&[], false, vec![]).unwrap_err(), CommandError::Empty);
}

#[test]
fn db_placeholder_on_a_filesystem_world_is_an_error_not_an_empty_string() {
    let c = AgentCommand::new(&argv(&["agent", "--db={db}"]), false, vec![]).unwrap();
    let p = paths();
    assert_eq!(c.argv_for(&ctx(&p, "g", None)).unwrap_err(), CommandError::DbPlaceholderOnFsWorld);
}

#[test]
fn db_placeholder_resolves_on_a_db_world() {
    let c = AgentCommand::new(&argv(&["agent", "--db={db}"]), false, vec![]).unwrap();
    let p = paths();
    let got = c.argv_for(&ctx(&p, "g", Some(&p.db))).unwrap();
    assert_eq!(got, vec!["--db=/tmp/qm-cert-1-0/workspace/data.db"]);
}

/// The report must echo the TEMPLATE. The expanded argv embeds an absolute
/// workspace path under the user's temp dir, which is exactly the machine info
/// rule 7f forbids in any log or payload.
#[test]
fn the_template_is_pre_substitution_and_carries_no_absolute_path() {
    let c = AgentCommand::new(&argv(&["my-agent", "--task", "{task}", "--ws", "{workspace}"]), false, vec![]).unwrap();
    let t = c.template();
    assert_eq!(t, "my-agent --task {task} --ws {workspace}");
    assert!(!t.contains("/tmp/"), "no expanded path may appear: {t}");
}

/// A QuantaMind credential must never reach arbitrary customer code.
#[test]
fn inherited_qm_variables_are_stripped_and_ours_are_set() {
    std::env::set_var("QM_API_KEY", "sk-secret");
    std::env::set_var("QM_BASE", "http://internal");
    let c = AgentCommand::new(&argv(&["agent"]), false, vec![]).unwrap();
    let p = paths();
    let env = c.env_for(&ctx(&p, "the goal", None), None);
    std::env::remove_var("QM_API_KEY");
    std::env::remove_var("QM_BASE");

    assert!(!env.contains_key("QM_API_KEY"), "an inherited credential must not reach the child");
    assert!(!env.contains_key("QM_BASE"));
    assert_eq!(env.get("QM_TASK").map(String::as_str), Some("the goal"));
    assert_eq!(env.get("QM_TASK_ID").map(String::as_str), Some("t-1"));
    assert_eq!(env.get("QM_ATTEMPT").map(String::as_str), Some("1"));
    assert_eq!(env.get("QM_WORKSPACE").map(String::as_str), Some("/tmp/qm-cert-1-0/workspace"));
}

#[test]
fn the_agents_own_environment_is_inherited_by_default() {
    // The agent needs its own provider key and PATH or it cannot run at all.
    std::env::set_var("ANTHROPIC_API_KEY", "sk-theirs");
    let c = AgentCommand::new(&argv(&["agent"]), false, vec![]).unwrap();
    let p = paths();
    let env = c.env_for(&ctx(&p, "g", None), None);
    std::env::remove_var("ANTHROPIC_API_KEY");
    assert_eq!(env.get("ANTHROPIC_API_KEY").map(String::as_str), Some("sk-theirs"));
}

#[test]
fn clean_env_keeps_only_the_allowlist_plus_named_passthroughs_plus_ours() {
    std::env::set_var("SOMETHING_ELSE", "leak");
    std::env::set_var("MY_AGENT_TOKEN", "keep-me");
    let c = AgentCommand::new(&argv(&["agent"]), true, vec!["MY_AGENT_TOKEN".into()]).unwrap();
    let p = paths();
    let env = c.env_for(&ctx(&p, "g", None), None);
    std::env::remove_var("SOMETHING_ELSE");
    std::env::remove_var("MY_AGENT_TOKEN");

    assert!(!env.contains_key("SOMETHING_ELSE"), "clean-env must not inherit it");
    assert_eq!(env.get("MY_AGENT_TOKEN").map(String::as_str), Some("keep-me"), "named passthrough kept");
    assert!(env.contains_key("QM_TASK"), "ours are always set");
}

#[test]
fn clean_env_still_strips_qm_variables_even_if_named() {
    std::env::set_var("QM_API_KEY", "sk-secret");
    // Even an explicit passthrough must not resurrect a QM_ credential: the
    // allowlist filter runs first, and QM_* is never in it.
    let c = AgentCommand::new(&argv(&["agent"]), true, vec!["QM_API_KEY".into()]).unwrap();
    let p = paths();
    let env = c.env_for(&ctx(&p, "g", None), None);
    std::env::remove_var("QM_API_KEY");
    assert_eq!(env.get("QM_API_KEY").map(String::as_str), None, "must stay stripped");
}

#[test]
fn otel_variables_are_set_only_when_an_endpoint_is_given() {
    let c = AgentCommand::new(&argv(&["agent"]), false, vec![]).unwrap();
    let p = paths();

    let off = c.env_for(&ctx(&p, "g", None), None);
    assert!(!off.contains_key("OTEL_EXPORTER_OTLP_ENDPOINT"));

    let on = c.env_for(&ctx(&p, "g", None), Some("http://127.0.0.1:4318"));
    assert_eq!(on.get("OTEL_EXPORTER_OTLP_ENDPOINT").map(String::as_str), Some("http://127.0.0.1:4318"));
    assert_eq!(
        on.get("OTEL_SEMCONV_STABILITY_OPT_IN").map(String::as_str),
        Some("gen_ai_latest_experimental"),
        "pins which generation of the pre-1.0 GenAI attributes an instrumented framework emits"
    );
}

#[test]
fn a_command_with_no_placeholders_is_valid() {
    // The agent can read everything from the environment instead.
    let c = AgentCommand::new(&argv(&["my-agent", "run"]), false, vec![]).unwrap();
    let p = paths();
    assert_eq!(c.argv_for(&ctx(&p, "g", None)).unwrap(), vec!["run"]);
    assert_eq!(c.program(), "my-agent");
}

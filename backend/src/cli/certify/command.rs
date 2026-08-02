//! How the customer's agent is invoked. **Pure** — builds argv and an environment,
//! spawns nothing, so every rule below is unit-testable.

use crate::inference::eval::harness::AttemptContext;
use std::collections::BTreeMap;

/// Placeholders a user may write in their command's arguments.
const P_TASK: &str = "{task}";
const P_WORKSPACE: &str = "{workspace}";
const P_TASK_FILE: &str = "{task_file}";
const P_DB: &str = "{db}";

/// Environment we always strip before adding our own, so a QuantaMind credential
/// can never reach arbitrary customer code and the contract is unambiguous: the
/// child sees exactly the `QM_*` we set, never an inherited one.
const QM_PREFIX: &str = "QM_";

/// The allowlist under `--clean-env`. Enough to run a program at all; anything
/// else the agent needs must be named explicitly with `--env`.
#[cfg(unix)]
const CLEAN_ENV_KEEP: &[&str] = &["PATH", "HOME", "TMPDIR", "LANG", "LC_ALL"];
#[cfg(not(unix))]
const CLEAN_ENV_KEEP: &[&str] =
    &["PATH", "USERPROFILE", "TEMP", "TMP", "SystemRoot", "windir", "COMSPEC", "PATHEXT"];

/// A validated agent command.
#[derive(Debug, Clone)]
pub struct AgentCommand {
    /// Absolute when the user gave a path; a bare name is left for PATH lookup.
    program: String,
    /// Exactly what the user typed — what reports echo.
    template_program: String,
    args: Vec<String>,
    clean_env: bool,
    passthrough: Vec<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum CommandError {
    Empty,
    /// A placeholder in `argv[0]`. Refused because the program name would then be
    /// chosen by task data — the exact shape of an injection.
    PlaceholderInProgram(String),
    /// A `{…}` we do not recognise. Never passed through verbatim: silently
    /// forwarding an unknown placeholder is as dishonest as silently dropping it.
    UnknownPlaceholder(String),
    /// `{db}` on a filesystem world — there is no database to point at.
    DbPlaceholderOnFsWorld,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Empty => write!(f, "no agent command given (pass it after `--`)"),
            CommandError::PlaceholderInProgram(p) => write!(
                f,
                "placeholder {p} is not allowed in the program name — only in its arguments"
            ),
            CommandError::UnknownPlaceholder(p) => write!(
                f,
                "unknown placeholder {p} — known: {P_TASK} {P_WORKSPACE} {P_TASK_FILE} {P_DB}"
            ),
            CommandError::DbPlaceholderOnFsWorld => {
                write!(f, "{P_DB} used but this task's world is a filesystem, not a database")
            }
        }
    }
}

impl AgentCommand {
    /// Validate a command template. Rejects at *config* time what would otherwise
    /// be a confusing failure per attempt.
    pub fn new(argv: &[String], clean_env: bool, passthrough: Vec<String>) -> Result<Self, CommandError> {
        let (program, args) = argv.split_first().ok_or(CommandError::Empty)?;
        if let Some(p) = first_placeholder(program) {
            return Err(CommandError::PlaceholderInProgram(p));
        }
        for a in args {
            let mut rest = a.as_str();
            while let Some(start) = rest.find('{') {
                let Some(end) = rest[start..].find('}') else { break };
                let ph = &rest[start..start + end + 1];
                if !matches!(ph, P_TASK | P_WORKSPACE | P_TASK_FILE | P_DB) {
                    return Err(CommandError::UnknownPlaceholder(ph.to_string()));
                }
                rest = &rest[start + end + 1..];
            }
        }
        Ok(AgentCommand {
            program: resolve_program(program),
            template_program: program.clone(),
            args: args.to_vec(),
            clean_env,
            passthrough,
        })
    }

    /// The template as configured — pre-substitution. This is what reports echo:
    /// the *expanded* argv embeds an absolute workspace path, which must never
    /// reach a log or a JSON payload (rule 7f).
    pub fn template(&self) -> String {
        std::iter::once(&self.template_program)
            .chain(self.args.iter())
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn program(&self) -> &str {
        &self.program
    }

    /// Substitute this attempt's paths into the arguments.
    ///
    /// The result is a `Vec<String>` handed straight to `Command`, never a string
    /// handed to a shell. That is what makes injection structurally impossible:
    /// a task whose text is `"; rm -rf / #"` stays exactly one argv element with
    /// no interpretation, on every platform.
    pub fn argv_for(&self, ctx: &AttemptContext) -> Result<Vec<String>, CommandError> {
        let mut out = Vec::with_capacity(self.args.len());
        for a in &self.args {
            let mut s = a.clone();
            if s.contains(P_DB) && ctx.db.is_none() {
                return Err(CommandError::DbPlaceholderOnFsWorld);
            }
            s = s.replace(P_TASK, ctx.goal);
            s = s.replace(P_WORKSPACE, &ctx.workspace.to_string_lossy());
            s = s.replace(P_TASK_FILE, &ctx.task_file.to_string_lossy());
            if let Some(db) = ctx.db {
                s = s.replace(P_DB, &db.to_string_lossy());
            }
            out.push(s);
        }
        Ok(out)
    }

    /// The child's environment.
    ///
    /// Default is inherit-minus-`QM_*`. Inheriting is necessary — the agent needs
    /// its own provider key, `PATH`, `HOME` to function at all — but every `QM_*`
    /// is dropped first so `QM_API_KEY` cannot leak into arbitrary customer code.
    /// `--clean-env` narrows to an allowlist plus whatever `--env NAME` names.
    ///
    /// Values are never logged. Only names appear anywhere.
    pub fn env_for(&self, ctx: &AttemptContext, otel_endpoint: Option<&str>) -> BTreeMap<String, String> {
        let mut env: BTreeMap<String, String> = std::env::vars()
            // `QM_*` is stripped in BOTH modes, and the strip runs BEFORE the
            // allowlist — so even an explicit `--env QM_API_KEY` cannot resurrect
            // a QuantaMind credential into arbitrary customer code. The contract
            // has to be absolute to be worth anything: the child sees exactly the
            // `QM_*` we set, never an inherited one.
            .filter(|(k, _)| !k.starts_with(QM_PREFIX))
            .filter(|(k, _)| {
                !self.clean_env || CLEAN_ENV_KEEP.contains(&k.as_str()) || self.passthrough.contains(k)
            })
            .collect();

        env.insert("QM_TASK".into(), ctx.goal.to_string());
        env.insert("QM_TASK_ID".into(), ctx.task_id.to_string());
        env.insert("QM_WORKSPACE".into(), ctx.workspace.to_string_lossy().into_owned());
        env.insert("QM_TASK_FILE".into(), ctx.task_file.to_string_lossy().into_owned());
        env.insert("QM_ATTEMPT".into(), ctx.attempt.to_string());
        if let Some(db) = ctx.db {
            env.insert("QM_DB".into(), db.to_string_lossy().into_owned());
        }
        if let Some(ep) = otel_endpoint {
            // We own the child's environment, so step visibility costs the customer
            // nothing: an already-instrumented agent exports to us with no code
            // change. The opt-in pins which generation of the (pre-1.0) GenAI
            // attributes an instrumented framework emits — where the framework
            // supports the transition; it is a narrowing, not a guarantee.
            env.insert("OTEL_EXPORTER_OTLP_ENDPOINT".into(), ep.to_string());
            env.insert("OTEL_SERVICE_NAME".into(), "agent-under-test".into());
            env.insert("OTEL_SEMCONV_STABILITY_OPT_IN".into(), "gen_ai_latest_experimental".into());
        }
        env
    }
}

/// Resolve a program path against the directory the user ran `qm` from.
///
/// The child's cwd is the *workspace*, so the agent can use relative paths into
/// the world it is grading. That would otherwise silently break the natural
/// invocation `-- ./my-agent`, which a user writes relative to their repo: the
/// child would look for `./my-agent` inside a temp directory and report "could not
/// start". So a program that looks like a path is made absolute here, once, at
/// config time; a bare name (`python`, `node`) is left alone for a normal PATH
/// lookup.
fn resolve_program(program: &str) -> String {
    let looks_like_path =
        program.contains('/') || program.contains('\\') || program.starts_with('.');
    if !looks_like_path {
        return program.to_string();
    }
    std::fs::canonicalize(program)
        .map(|p| p.to_string_lossy().into_owned())
        // Leave it as typed on failure: `spawn` then reports the real reason,
        // which is more useful than a canonicalize error.
        .unwrap_or_else(|_| program.to_string())
}

fn first_placeholder(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let end = s[start..].find('}')?;
    Some(s[start..start + end + 1].to_string())
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;

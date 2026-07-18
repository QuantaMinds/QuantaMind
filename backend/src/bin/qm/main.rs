//! `qm` — QuantaMind's headless CLI. Thin by design: parse args, call the pure
//! `cli::{doctor,run,init}` engines in `quantamind_lib`, render, and map the
//! documented exit-code contract. No logic here (architecture: thin command, pure core).
//!
//! Stream discipline: the report (data) → stdout; every `[QM-CODE]` fix line
//! (diagnostics) → stderr. So `qm doctor --json | jq` is never polluted by prose.

use clap::{Parser, Subcommand, ValueEnum};
use quantamind_lib::cli::doctor::render::label;
use quantamind_lib::cli::doctor::{self, DoctorOptions};
use quantamind_lib::cli::run::config::QmConfig;
use quantamind_lib::cli::run::{self, FailOn, RunMode, RunOptions, RunOutcome};
use quantamind_lib::commands::remote::remote_health::RemoteAuthStatus;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::eval::agentic::difficulty::passk::ThinkPreset;
use quantamind_lib::inference::eval::agentic::spec::Tier;
use quantamind_lib::redact::redact_path;
use quantamind_lib::secrets;
use std::io::{IsTerminal, Write};

#[derive(Parser)]
#[command(name = "qm", version, about = "QuantaMind headless CLI — local agent-readiness")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Diagnose backends: reachability, models, credentials, tool-calling, version.
    Doctor(DoctorArgs),
    /// Run the built-in tool-calling suite → a Ready/Conditional/NotReady verdict.
    Run(RunArgs),
    /// Auto-detect a backend, write qm.json, and run the suite (zero config).
    Init(InitArgs),
}

#[derive(clap::Args)]
struct InitArgs {
    /// Emit the machine-readable report as JSON on stdout (progress/errors to stderr).
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct RunArgs {
    /// Backend to run against (falls back to qm.json, then ollama).
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Model to run (falls back to qm.json; env QM_MODEL).
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Built-in collection id.
    #[arg(long, default_value = "easy-coding")]
    collection: String,
    /// Endpoint override (required for remote backends). Env: QM_BASE.
    #[arg(long, env = "QM_BASE")]
    base: Option<String>,
    /// Readiness profile id (a built-in: general-agent, rag-assistant, coding-agent).
    #[arg(long, default_value = "general-agent")]
    profile: String,
    /// Override the strict pass^k run count (default: the collection tier's).
    #[arg(long)]
    k: Option<u32>,
    /// Difficulty tier (scales token budget + default k). Default: the collection's own.
    #[arg(long, value_enum)]
    tier: Option<TierArg>,
    /// Calling path to exercise.
    #[arg(long, value_enum, default_value = "prompt_based")]
    mode: ModeArg,
    /// Reasoning-scratchpad budget: lean (off) / standard / deep.
    #[arg(long, value_enum, default_value = "lean")]
    thinking: ThinkingArg,
    /// Which verdicts fail the process exit (CI gate).
    #[arg(long, value_enum, default_value = "conditional")]
    fail_on: FailOnArg,
    /// Emit the machine-readable report as JSON on stdout (progress/errors to stderr).
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum TierArg {
    Easy,
    Medium,
    Hard,
    Extreme,
}
impl From<TierArg> for Tier {
    fn from(t: TierArg) -> Self {
        match t {
            TierArg::Easy => Tier::Easy,
            TierArg::Medium => Tier::Medium,
            TierArg::Hard => Tier::Hard,
            TierArg::Extreme => Tier::Extreme,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ModeArg {
    #[value(name = "prompt_based", alias = "prompt", alias = "prompt-based")]
    PromptBased,
    Native,
    Both,
}
impl From<ModeArg> for RunMode {
    fn from(m: ModeArg) -> Self {
        match m {
            ModeArg::PromptBased => RunMode::PromptBased,
            ModeArg::Native => RunMode::Native,
            ModeArg::Both => RunMode::Both,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum ThinkingArg {
    Lean,
    Standard,
    Deep,
}
impl From<ThinkingArg> for ThinkPreset {
    fn from(t: ThinkingArg) -> Self {
        match t {
            ThinkingArg::Lean => ThinkPreset::Lean,
            ThinkingArg::Standard => ThinkPreset::Standard,
            ThinkingArg::Deep => ThinkPreset::Deep,
        }
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum FailOnArg {
    Conditional,
    #[value(name = "notready", alias = "not_ready", alias = "not-ready")]
    NotReady,
    Never,
}

impl From<FailOnArg> for FailOn {
    fn from(f: FailOnArg) -> Self {
        match f {
            FailOnArg::Conditional => FailOn::Conditional,
            FailOnArg::NotReady => FailOn::NotReady,
            FailOnArg::Never => FailOn::Never,
        }
    }
}

#[derive(clap::Args)]
struct DoctorArgs {
    /// Limit the check to one backend (default: scan all five).
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Endpoint URL for the targeted backend (only with --backend). Env: QM_BASE.
    #[arg(long, env = "QM_BASE")]
    base: Option<String>,
    /// Model to check native tool-calling against (Ollama). Env: QM_MODEL.
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Emit the machine-readable report as JSON on stdout (fixes still go to stderr).
    #[arg(long)]
    json: bool,
}

/// The `--backend` values, mapped 1:1 onto `BackendKind`'s wire strings.
#[derive(Clone, Copy, ValueEnum)]
enum BackendArg {
    Ollama,
    #[value(name = "llama_cpp", alias = "llama-cpp", alias = "llamacpp")]
    LlamaCpp,
    Mlx,
    Vllm,
    Sglang,
}

impl From<BackendArg> for BackendKind {
    fn from(b: BackendArg) -> Self {
        match b {
            BackendArg::Ollama => BackendKind::Ollama,
            BackendArg::LlamaCpp => BackendKind::LlamaCpp,
            BackendArg::Mlx => BackendKind::Mlx,
            BackendArg::Vllm => BackendKind::VLlm,
            BackendArg::Sglang => BackendKind::SgLang,
        }
    }
}

/// Remote bearer credential — env first (never argv, rule 7), then the keychain
/// slot for the targeted remote backend.
fn resolve_key(backend: Option<BackendKind>) -> Option<String> {
    if let Ok(k) = std::env::var("QM_API_KEY") {
        if !k.is_empty() {
            return Some(k);
        }
    }
    match backend {
        Some(BackendKind::VLlm) => secrets::get(secrets::VLLM_API_KEY),
        Some(BackendKind::SgLang) => secrets::get(secrets::SGLANG_API_KEY),
        _ => None,
    }
}

#[tokio::main]
async fn main() {
    // clap exits 2 on a parse error — the documented "bad args" code, for free.
    let cli = Cli::parse();
    match cli.command {
        Command::Doctor(args) => run_doctor(args).await,
        Command::Run(args) => run_suite(args).await,
        Command::Init(args) => run_init(args).await,
    }
}

async fn run_suite(args: RunArgs) {
    // Flags win; otherwise fall back to qm.json (written by `qm init`), then — in an
    // interactive terminal — an on-the-fly dropdown; otherwise fail fast (CI-safe).
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = QmConfig::load(&cwd);
    let base = args.base.clone().or_else(|| cfg.as_ref().and_then(|c| c.base.clone()));

    let backend = resolve_backend(args.backend, &cfg).await;
    let api_key = resolve_key(Some(backend));
    let model = match args.model.clone().or_else(|| cfg.as_ref().map(|c| c.model.clone())) {
        Some(m) => m,
        None => match pick_model(backend, base.as_deref(), api_key.as_deref()).await {
            Some(m) => m,
            None => {
                eprintln!("[QM-NO-MODEL] no model — pass --model, run `qm init`, or run in a terminal to pick one.");
                std::process::exit(2);
            }
        },
    };

    let opts = RunOptions {
        backend,
        model,
        collection: args.collection,
        base,
        api_key,
        k: args.k,
        tier: args.tier.map(Tier::from),
        think: ThinkPreset::from(args.thinking),
        mode: RunMode::from(args.mode),
        profile_id: args.profile,
    };
    execute_run(opts, args.json, FailOn::from(args.fail_on)).await;
}

/// True when stdin is an interactive terminal — the gate for any prompt. Over SSH
/// in CI / a pipe / `--json`-into-jq, this is false and we never block on input.
fn is_interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// Show a numbered menu on stderr and read a 1-based choice from stdin. `None` when
/// not a TTY (caller fails fast) or the input is invalid.
fn select(title: &str, options: &[String]) -> Option<usize> {
    if !is_interactive() || options.is_empty() {
        return None;
    }
    eprintln!("{title}");
    for (i, o) in options.iter().enumerate() {
        eprintln!("  {}) {o}", i + 1);
    }
    eprint!("> ");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).ok()?;
    run::render::parse_selection(&line, options.len())
}

/// Resolve the backend: flag → qm.json → (interactive pick among runnable) → ollama.
async fn resolve_backend(flag: Option<BackendArg>, cfg: &Option<QmConfig>) -> BackendKind {
    if let Some(b) = flag {
        return b.into();
    }
    if let Some(c) = cfg {
        return c.backend;
    }
    if is_interactive() {
        let report = doctor::run_doctor(DoctorOptions { backend: None, base: None, model: None, api_key: None }).await;
        let runnable: Vec<BackendKind> = report.runnable().iter().map(|b| b.kind).collect();
        match runnable.as_slice() {
            [only] => return *only,
            [_, ..] => {
                let opts: Vec<String> = runnable.iter().map(|k| label(*k).to_string()).collect();
                if let Some(i) = select("Select a backend:", &opts) {
                    return runnable[i];
                }
            }
            [] => {}
        }
    }
    BackendKind::Ollama
}

/// Interactive model picker: probe the backend's served models and let the user
/// choose. `None` when not a TTY or nothing is served.
async fn pick_model(backend: BackendKind, base: Option<&str>, key: Option<&str>) -> Option<String> {
    if !is_interactive() {
        return None;
    }
    let bd = doctor::probe::probe_backend(backend, base, None, key).await;
    let i = select(&format!("Select a model for {}:", label(backend)), &bd.models)?;
    bd.models.get(i).cloned()
}

async fn run_init(args: InitArgs) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let Some(cfg) = quantamind_lib::cli::init::detect(resolve_key(None)).await else {
        eprintln!("[QM-NO-RUNNABLE] no runnable backend found — run `qm doctor` to see what to fix.");
        std::process::exit(run::render::EXIT_UNREACHABLE);
    };
    match cfg.save(&cwd) {
        Ok(path) => eprintln!("wrote {} (backend={}, model={})", path.display(), label(cfg.backend), cfg.model),
        Err(e) => {
            eprintln!("[QM-INTERNAL] could not write qm.json: {}", redact_path(&e.to_string()));
            std::process::exit(1);
        }
    }
    let opts = RunOptions {
        backend: cfg.backend,
        model: cfg.model,
        collection: cfg.collection,
        base: cfg.base,
        api_key: resolve_key(Some(cfg.backend)),
        k: None,
        tier: None,
        think: ThinkPreset::Lean,
        mode: RunMode::PromptBased,
        profile_id: cfg.profile,
    };
    execute_run(opts, args.json, FailOn::Conditional).await;
}

/// Run a suite and exit on the verdict — shared by `run` and `init`.
async fn execute_run(opts: RunOptions, json: bool, fail_on: FailOn) {
    let outcome = match run::run_suite(opts).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[QM-INTERNAL] run failed: {}", redact_path(&e.to_string()));
            std::process::exit(1);
        }
    };

    match outcome {
        RunOutcome::Unreachable { backend, endpoint } => {
            eprintln!("[QM-BACKEND-UNREACHABLE] {} not reachable at {endpoint} — run `qm doctor --backend {}` to diagnose.", label(backend), label(backend));
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        RunOutcome::ModelNotFound { backend, model, available } => {
            eprintln!("[QM-MODEL-NOT-FOUND] {} has no model '{model}' — available: {}", label(backend), if available.is_empty() { "(none)".into() } else { available.join(", ") });
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        RunOutcome::CredentialError { backend, report } => {
            // The same distinctions doctor makes — a credential problem is not a missing model.
            if report.insecure_key {
                eprintln!("[QM-INSECURE-KEY] {} — a key is set but the URL isn't https; the key was withheld. Use https or drop the key.", report.host);
            } else {
                match report.status {
                    RemoteAuthStatus::Unauthorized => eprintln!("[QM-UNAUTHORIZED] {} rejected the API key — check QM_API_KEY.", report.host),
                    RemoteAuthStatus::NotFound => eprintln!("[QM-NOT-OPENAI] {} has no /v1/models — check the URL is an OpenAI-compatible server.", report.host),
                    _ => eprintln!("[QM-SERVER-ERROR] {} returned an error — check the server ({}).", report.host, label(backend)),
                }
            }
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        RunOutcome::UnknownCollection { id } => {
            eprintln!("[QM-BAD-COLLECTION] unknown collection '{id}'.");
            std::process::exit(2);
        }
        RunOutcome::UnknownProfile { id } => {
            eprintln!("[QM-BAD-PROFILE] unknown profile '{id}' — try general-agent, rag-assistant, or coding-agent.");
            std::process::exit(2);
        }
        RunOutcome::NativeUnsupported { backend, model } => {
            eprintln!("[QM-NATIVE-UNSUPPORTED] {} has no native tool-calling for '{model}' — use --mode prompt_based or both.", label(backend));
            std::process::exit(2);
        }
        RunOutcome::ThinkingUnsupported { backend, model } => {
            // A backend-specific, actionable fix — reasoning fails for different reasons per engine.
            let hint = match backend {
                BackendKind::Ollama => "this model has no reasoning capability — use --thinking lean, or pick a reasoning model (e.g. one whose `ollama show` lists \"thinking\")",
                BackendKind::LlamaCpp | BackendKind::Mlx => "the server returned no reasoning — relaunch it with `--jinja --reasoning-format deepseek` and use a reasoning model, or use --thinking lean",
                _ => "the server returned no reasoning — enable its reasoning parser, or use --thinking lean",
            };
            eprintln!("[QM-THINKING-UNSUPPORTED] --thinking won't take effect for '{model}' on {}: {hint}.", label(backend));
            std::process::exit(2);
        }
        RunOutcome::Inconclusive { reason } => {
            eprintln!("[QM-INCONCLUSIVE] the run errored before it could measure anything — retry. ({reason})");
            std::process::exit(run::render::EXIT_INCONCLUSIVE);
        }
        RunOutcome::Ran(report) => {
            let status = report.worst_status();
            if json {
                match serde_json::to_string_pretty(&report) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("[QM-INTERNAL] failed to serialize report: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                print!("{}", run::render_human(&report));
            }
            let code = run::exit_code(status, fail_on);
            // Note when a soft policy downgraded a non-Ready verdict to a pass.
            if code == 0 && status != quantamind_lib::inference::eval::readiness::types::Readiness::Ready {
                eprintln!("[QM-NOTE] verdict is {status:?} but --fail-on let it pass (exit 0).");
            }
            std::process::exit(code);
        }
    }
}

async fn run_doctor(args: DoctorArgs) {
    let backend = args.backend.map(BackendKind::from);
    let opts = DoctorOptions {
        backend,
        base: args.base,
        model: args.model,
        api_key: resolve_key(backend),
    };
    let report = doctor::run_doctor(opts).await;

    if args.json {
        match serde_json::to_string_pretty(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("[QM-INTERNAL] failed to serialize report: {e}");
                std::process::exit(1);
            }
        }
    } else {
        print!("{}", doctor::render_human(&report));
    }
    // Fix lines are diagnostics — always stderr, in both modes.
    for line in doctor::error_lines(&report) {
        eprintln!("{line}");
    }
    std::process::exit(report.exit_code());
}

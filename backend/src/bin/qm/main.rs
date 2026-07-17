//! `qm` — QuantaMind's headless CLI. Thin by design: parse args, call the pure
//! `commands::doctor` engine in `quantamind_lib`, render, and map the documented
//! exit-code contract. No logic lives here (architecture: thin command, pure core).
//!
//! Stream discipline: the report (data) → stdout; every `[QM-CODE]` fix line
//! (diagnostics) → stderr. So `qm doctor --json | jq` is never polluted by prose.

use clap::{Parser, Subcommand, ValueEnum};
use quantamind_lib::commands::doctor::render::label;
use quantamind_lib::commands::doctor::{self, DoctorOptions};
use quantamind_lib::commands::run::config::QmConfig;
use quantamind_lib::commands::run::{self, FailOn, RunOptions, RunOutcome};
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::secrets;

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
    /// Treat as a reasoning model (raises the token budget, strips <think>).
    #[arg(long)]
    thinking: bool,
    /// Which verdicts fail the process exit (CI gate).
    #[arg(long, value_enum, default_value = "conditional")]
    fail_on: FailOnArg,
    /// Emit the machine-readable report as JSON on stdout (progress/errors to stderr).
    #[arg(long)]
    json: bool,
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
    // Flags win; otherwise fall back to qm.json (written by `qm init`), then defaults.
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = QmConfig::load(&cwd);
    let backend = args
        .backend
        .map(BackendKind::from)
        .or_else(|| cfg.as_ref().map(|c| c.backend))
        .unwrap_or(BackendKind::Ollama);
    let model = args.model.or_else(|| cfg.as_ref().map(|c| c.model.clone()));
    let Some(model) = model else {
        eprintln!("[QM-NO-MODEL] no model — pass --model, or run `qm init` first.");
        std::process::exit(2);
    };
    let base = args.base.or_else(|| cfg.as_ref().and_then(|c| c.base.clone()));

    let opts = RunOptions {
        backend,
        model,
        collection: args.collection,
        base,
        api_key: resolve_key(Some(backend)),
        k: args.k,
        is_thinking: args.thinking,
        profile_id: args.profile,
    };
    execute_run(opts, args.json, FailOn::from(args.fail_on)).await;
}

async fn run_init(args: InitArgs) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let Some(cfg) = quantamind_lib::commands::init::detect(resolve_key(None)).await else {
        eprintln!("[QM-NO-RUNNABLE] no runnable backend found — run `qm doctor` to see what to fix.");
        std::process::exit(run::render::EXIT_UNREACHABLE);
    };
    match cfg.save(&cwd) {
        Ok(path) => eprintln!("wrote {} (backend={}, model={})", path.display(), label(cfg.backend), cfg.model),
        Err(e) => {
            eprintln!("[QM-INTERNAL] could not write qm.json: {e}");
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
        is_thinking: false,
        profile_id: cfg.profile,
    };
    execute_run(opts, args.json, FailOn::Conditional).await;
}

/// Run a suite and exit on the verdict — shared by `run` and `init`.
async fn execute_run(opts: RunOptions, json: bool, fail_on: FailOn) {
    let outcome = match run::run_suite(opts).await {
        Ok(o) => o,
        Err(e) => {
            eprintln!("[QM-INTERNAL] run failed: {e}");
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
        RunOutcome::UnknownCollection { id } => {
            eprintln!("[QM-BAD-COLLECTION] unknown collection '{id}'.");
            std::process::exit(2);
        }
        RunOutcome::UnknownProfile { id } => {
            eprintln!("[QM-BAD-PROFILE] unknown profile '{id}' — try general-agent, rag-assistant, or coding-agent.");
            std::process::exit(2);
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

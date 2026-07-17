//! `qm` — QuantaMind's headless CLI. Thin by design: parse args, call the pure
//! `commands::doctor` engine in `quantamind_lib`, render, and map the documented
//! exit-code contract. No logic lives here (architecture: thin command, pure core).
//!
//! Stream discipline: the report (data) → stdout; every `[QM-CODE]` fix line
//! (diagnostics) → stderr. So `qm doctor --json | jq` is never polluted by prose.

use clap::{Parser, Subcommand, ValueEnum};
use quantamind_lib::commands::doctor::{self, DoctorOptions};
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

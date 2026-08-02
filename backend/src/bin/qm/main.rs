//! `qm` — QuantaMind's headless CLI. Thin by design: parse args, call the pure
//! `cli::{doctor,run,init}` engines in `quantamind_lib`, render, and map the
//! documented exit-code contract. No logic here (architecture: thin command, pure core).
//!
//! Stream discipline: the report (data) → stdout; every `[QM-CODE]` fix line
//! (diagnostics) → stderr. So `qm doctor --json | jq` is never polluted by prose.

use clap::{Parser, Subcommand, ValueEnum};
use quantamind_lib::cli::cliff::{self, cliff_exit, render_cliff, CliffOptions, CliffOutcome};
use quantamind_lib::cli::doctor::render::label;
use quantamind_lib::cli::doctor::{self, DoctorOptions};
use quantamind_lib::cli::run::config::QmConfig;
use quantamind_lib::cli::run::{self, FailOn, ReportOutcome, RunMode, RunOptions, RunOutcome};
use quantamind_lib::commands::eval::toolcall_cmd::list_builtin_collections;
use quantamind_lib::inference::eval::cliff::{CliffPreset, CliffSource};
use quantamind_lib::commands::remote::remote_health::RemoteAuthStatus;
use quantamind_lib::commands::prompt::prompt_options::validate_params;
use quantamind_lib::persistence::prompts::schema::InferenceParams;
use quantamind_lib::inference::backend::backend_kind::BackendKind;
use quantamind_lib::inference::eval::agentic::difficulty::passk::ThinkPreset;
use quantamind_lib::inference::eval::agentic::spec::Tier;
use quantamind_lib::redact::redact_path;
use quantamind_lib::secrets;
use std::io::{IsTerminal, Write};
use std::path::PathBuf;

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
    /// Last persisted run's per-task costs (latency/cache/memory), read from the
    /// desktop app's stores — no model run. Run costs live with `run/test --costs`.
    Costs(CostsArgs),
    /// Run a custom collection FILE → a per-mode scoreboard + verdict.
    Test(TestArgs),
    /// Re-assess a saved run against a readiness profile, offline (no backend).
    Report(ReportArgs),
    /// Context Stress Test: ramp prompt depth and find where tool-calling collapses.
    Cliff(CliffArgs),
    /// Prove a collection/world is a reliable test BEFORE running it (the same gate
    /// `run`/`test` apply automatically to uploaded files).
    Validate(ValidateArgs),
    /// Free-form generation: a system+user prompt with params, streamed to stdout
    /// (the headless twin of the Workspace Run).
    Prompt(PromptArgs),
    /// Gate a deploy on YOUR OWN agent: seed a world, run your command against it,
    /// grade the real end state, k times. QuantaMind issues no model call.
    Certify(CertifyArgs),
}

#[derive(clap::Args)]
struct PromptArgs {
    /// Backend to generate against (falls back to qm.json, then the server).
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Model to run. Env: QM_MODEL.
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Endpoint override. Env: QM_BASE.
    #[arg(long, env = "QM_BASE")]
    base: Option<String>,
    /// Optional system prompt.
    #[arg(long)]
    system: Option<String>,
    /// The user prompt. Omit to read it from stdin (pipe or type, then Ctrl-D).
    #[arg(long)]
    user: Option<String>,
    #[command(flatten)]
    params: ParamArgs,
}

#[derive(clap::Args)]
struct CertifyArgs {
    /// The suite file: a JSON array of world tasks (the shape the desktop MCP
    /// builder authors).
    #[arg(long)]
    suite: std::path::PathBuf,
    /// Override every task's k. Strict pass^k: all k attempts must pass.
    #[arg(long)]
    k: Option<u32>,
    /// Per-attempt wall-clock cap.
    #[arg(long, default_value_t = 300)]
    timeout: u64,
    /// Grace between the group TERM and the KILL on timeout.
    #[arg(long, default_value_t = 5)]
    kill_grace: u64,
    /// Allowlist the child's environment instead of inheriting it. `QM_*` is
    /// stripped in both modes.
    #[arg(long)]
    clean_env: bool,
    /// Pass one inherited variable through by NAME (repeatable). Values are never
    /// logged.
    #[arg(long = "env")]
    env: Vec<String>,
    /// Don't echo the agent's output; the stderr tail is still kept for failures.
    #[arg(long)]
    quiet_agent: bool,
    /// Skip the anti-vacuity precheck. Prints a loud note; not settable in qm.json.
    #[arg(long)]
    no_precheck: bool,
    /// Which verdicts fail the process.
    #[arg(long, value_enum, default_value_t = FailOnArg::Conditional)]
    fail_on: FailOnArg,
    /// The agent command, after `--`. Never shell-interpreted.
    #[arg(last = true, num_args = 1..)]
    agent: Vec<String>,
}

#[derive(clap::Args)]
struct ValidateArgs {
    /// Built-in collection id or a collection/world .json file.
    #[arg(long)]
    collection: Option<String>,
    /// Spawn each MCP world and run the do-nothing check (default on; needs npx).
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    live_world: bool,
    /// Emit the machine-readable CollectionValidation as JSON on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct CliffArgs {
    /// Backend to probe against.
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Model to probe. Env: QM_MODEL.
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Built-in collection id or a collection file. Omit in a terminal to pick from a list.
    #[arg(long)]
    collection: Option<String>,
    /// Endpoint override. Env: QM_BASE.
    #[arg(long, env = "QM_BASE")]
    base: Option<String>,
    /// Ceiling for the padding ladder (deepest rung's target tokens).
    #[arg(long, default_value_t = 4096)]
    max_tokens: u32,
    /// Ladder rungs (baseline + deeper rungs).
    #[arg(long, default_value_t = 4)]
    steps: u32,
    /// Padding corpus preset.
    #[arg(long, value_enum, default_value = "corporate_policy")]
    source: SourceArg,
    /// Calling path: prompt_based (default) or native tool-calling.
    #[arg(long, value_enum, default_value = "prompt_based")]
    mode: ModeArg,
    /// Thinking budget: lean (reasoning off) / standard / deep. The scratchpad scales
    /// with each rung's depth (≤4k Easy-band … >16k Extreme-band), mirroring the GUI.
    #[arg(long, value_enum, default_value = "lean")]
    thinking: ThinkingArg,
    /// Flat per-turn output cap applied at EVERY rung (experimental control) —
    /// overrides the depth-banded --thinking budget so depth is the only variable.
    /// A cap that rises with the padding can mask a growing output cost.
    #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
    cap: Option<u32>,
    #[command(flatten)]
    params: ParamArgs,
    /// Emit the machine-readable CliffReport as JSON on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum SourceArg {
    #[value(name = "corporate_policy", alias = "corporate-policy")]
    CorporatePolicy,
    #[value(name = "system_logs", alias = "system-logs")]
    SystemLogs,
    #[value(name = "financial_ledger", alias = "financial-ledger")]
    FinancialLedger,
}

impl From<SourceArg> for CliffSource {
    fn from(s: SourceArg) -> Self {
        let preset = match s {
            SourceArg::CorporatePolicy => CliffPreset::CorporatePolicy,
            SourceArg::SystemLogs => CliffPreset::SystemLogs,
            SourceArg::FinancialLedger => CliffPreset::FinancialLedger,
        };
        CliffSource::Preset { preset }
    }
}

#[derive(clap::Args)]
struct CostsArgs {
    /// The collection id whose last run to read (as shown in the app / `qm run`).
    collection: String,
    /// Override the desktop app's data directory (default: the platform app-config dir).
    #[arg(long)]
    data_dir: Option<PathBuf>,
    /// Emit the machine-readable costs as JSON on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct ReportArgs {
    /// A saved run report (from `run`/`test --save-report`).
    #[arg(long)]
    report: PathBuf,
    /// Readiness profile: a built-in id (general-agent/rag-assistant/coding-agent) or a .json file.
    #[arg(long, default_value = "general-agent")]
    profile: String,
    /// Which verdicts fail the process exit.
    #[arg(long, value_enum, default_value = "conditional")]
    fail_on: FailOnArg,
    /// Also write a JUnit XML report here.
    #[arg(long)]
    junit: Option<PathBuf>,
    /// Emit the machine-readable report as JSON on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct TestArgs {
    /// Collection file to run (JSON: a ToolTask array or a v2 collection object).
    #[arg(long)]
    collection: PathBuf,
    /// Backend to run against.
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Model to run. Env: QM_MODEL.
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Endpoint override (remote backends). Env: QM_BASE.
    #[arg(long, env = "QM_BASE")]
    base: Option<String>,
    /// Readiness profile id.
    #[arg(long, default_value = "general-agent")]
    profile: String,
    /// Calling path(s): defaults to `both` (native + prompt) for a full scoreboard.
    #[arg(long, value_enum, default_value = "both")]
    mode: ModeArg,
    /// Difficulty-tier override.
    #[arg(long, value_enum)]
    tier: Option<TierArg>,
    /// Reasoning-scratchpad budget.
    #[arg(long, value_enum, default_value = "lean")]
    thinking: ThinkingArg,
    /// pass^k override.
    #[arg(long)]
    k: Option<u32>,
    /// Per-turn step cap (UI "Max Steps"). Default: each task's authored cap.
    #[arg(long)]
    max_steps: Option<u32>,
    /// Decoy tools injected per task (UI "Decoy Tools"). Default: the task's own.
    #[arg(long)]
    decoy: Option<u32>,
    #[command(flatten)]
    params: ParamArgs,
    /// Which verdicts fail the process exit (CI gate).
    #[arg(long, value_enum, default_value = "conditional")]
    fail_on: FailOnArg,
    /// Also write a JUnit XML report here.
    #[arg(long)]
    junit: Option<PathBuf>,
    /// Write the raw run report here for offline re-assessment (`qm report --report`).
    #[arg(long)]
    save_report: Option<PathBuf>,
    /// Persist per-step task trajectories (raw model output, injections, timings) as
    /// JSONL files in this directory — the GUI trace store's format, for post-mortems.
    #[arg(long)]
    save_transcripts: Option<PathBuf>,
    /// Also report per-task run costs (prefill/decode split, thinking split, cache
    /// hits, peak context, step-end RSS, KV-at-peak) — the CLI twin of the Latency
    /// tab's Test-run view. Costs ride the JSON output too.
    #[arg(long)]
    costs: bool,
    /// Emit the machine-readable report as JSON on stdout.
    #[arg(long)]
    json: bool,
}

#[derive(clap::Args)]
struct InitArgs {
    /// Emit the machine-readable report as JSON on stdout (progress/errors to stderr).
    #[arg(long)]
    json: bool,
}

/// The 7 global inference params, shared across run/test/cliff/prompt via
/// `#[command(flatten)]`. All optional — unset ones keep the command's default
/// (greedy temp-0 for eval; the model's own defaults for `prompt`).
#[derive(clap::Args, Clone)]
struct ParamArgs {
    /// Sampling temperature 0.0–2.0 (eval defaults to 0.0 greedy; set to sample).
    #[arg(long)]
    temperature: Option<f32>,
    /// Nucleus sampling top-p 0.0–1.0.
    #[arg(long)]
    top_p: Option<f32>,
    /// Top-k sampling.
    #[arg(long)]
    top_k: Option<u32>,
    /// Max generated tokens (the server `num_predict`). (Distinct from `cliff --max-tokens`,
    /// which is the padding-ladder ceiling.)
    #[arg(long)]
    num_predict: Option<u32>,
    /// Repetition penalty 0.0–2.0.
    #[arg(long)]
    repeat_penalty: Option<f32>,
    /// RNG seed for reproducible sampling.
    #[arg(long)]
    seed: Option<i64>,
    /// Context window size (≥1).
    #[arg(long)]
    num_ctx: Option<u32>,
}

impl ParamArgs {
    /// Validated `InferenceParams`, or `None` when the user set nothing (keep the
    /// command default). Exits 2 with `[QM-BAD-PARAM]` on an out-of-range value.
    fn resolve(&self) -> Option<InferenceParams> {
        let p = InferenceParams {
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            max_tokens: self.num_predict,
            repeat_penalty: self.repeat_penalty,
            seed: self.seed,
            num_ctx: self.num_ctx,
        };
        if p == InferenceParams::default() {
            return None;
        }
        if let Err(e) = validate_params(&p) {
            eprintln!("[QM-BAD-PARAM] {}", redact_path(&e.to_string()));
            std::process::exit(2);
        }
        Some(p)
    }
}

#[derive(clap::Args)]
struct RunArgs {
    /// Backend to run against (falls back to qm.json, then the server).
    #[arg(long, value_enum)]
    backend: Option<BackendArg>,
    /// Model to run (falls back to qm.json; env QM_MODEL).
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Built-in collection id or a collection file. Omit in a terminal to pick from a list.
    #[arg(long)]
    collection: Option<String>,
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
    /// Reasoning-scratchpad budget: lean (off) / standard / deep. Omit in a terminal to pick.
    #[arg(long, value_enum)]
    thinking: Option<ThinkingArg>,
    /// Per-turn step cap (UI "Max Steps"). Default: each task's authored cap.
    #[arg(long)]
    max_steps: Option<u32>,
    /// Decoy tools injected per task (UI "Decoy Tools"). Default: the task's own.
    #[arg(long)]
    decoy: Option<u32>,
    #[command(flatten)]
    params: ParamArgs,
    /// Which verdicts fail the process exit (CI gate).
    #[arg(long, value_enum, default_value = "conditional")]
    fail_on: FailOnArg,
    /// Also write a JUnit XML report here (for a CI test panel).
    #[arg(long)]
    junit: Option<PathBuf>,
    /// Write the raw run report here for offline re-assessment (`qm report --report`).
    #[arg(long)]
    save_report: Option<PathBuf>,
    /// Persist per-step task trajectories (raw model output, injections, timings) as
    /// JSONL files in this directory — the GUI trace store's format, for post-mortems.
    #[arg(long)]
    save_transcripts: Option<PathBuf>,
    /// Also report per-task run costs (prefill/decode split, thinking split, cache
    /// hits, peak context, step-end RSS, KV-at-peak) — the CLI twin of the Latency
    /// tab's Test-run view. Costs ride the JSON output too.
    #[arg(long)]
    costs: bool,
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
    /// Model to check native tool-calling against . Env: QM_MODEL.
    #[arg(long, env = "QM_MODEL")]
    model: Option<String>,
    /// Emit the machine-readable report as JSON on stdout (fixes still go to stderr).
    #[arg(long)]
    json: bool,
}

/// The `--backend` values, mapped 1:1 onto `BackendKind`'s wire strings.
#[derive(Clone, Copy, ValueEnum)]
enum BackendArg {
    #[value(name = "llama_cpp", alias = "llama-cpp", alias = "llamacpp")]
    LlamaCpp,
    Vllm,
}

impl From<BackendArg> for BackendKind {
    fn from(b: BackendArg) -> Self {
        match b {
            BackendArg::LlamaCpp => BackendKind::LlamaCpp,
            BackendArg::Vllm => BackendKind::VLlm,
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
        Command::Test(args) => run_test(args).await,
        Command::Report(args) => run_report(args),
        Command::Costs(args) => run_costs_cmd(args),
        Command::Cliff(args) => run_cliff_cmd(args).await,
        Command::Validate(args) => run_validate_cmd(args).await,
        Command::Prompt(args) => run_prompt_cmd(args).await,
        Command::Certify(args) => run_certify_cmd(args),
    }
}

/// `qm costs` — the last persisted run's per-task costs, read from the desktop app's
/// stores (latest-batch retention). Labels are the SANITIZED transcript stems — the
/// original ids aren't recorded in the filenames, and we never "un-sanitize" by guess.
fn run_costs_cmd(args: CostsArgs) {
    let Some(dir) = args.data_dir.or_else(quantamind_lib::cli::costs::default_data_dir) else {
        eprintln!("[QM-NO-DATA-DIR] couldn't resolve the app data dir — pass --data-dir");
        std::process::exit(2);
    };
    match quantamind_lib::cli::costs::load_collection_costs(&dir, &args.collection) {
        Ok(runs) => {
            if args.json {
                match serde_json::to_string_pretty(&runs) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("[QM-INTERNAL] {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                eprintln!("(last persisted run of '{}'; names are sanitized transcript stems)", args.collection);
                for r in &runs {
                    print!("{}", run::render::render_costs(r));
                }
            }
        }
        Err(e) => {
            eprintln!("[QM-NO-RUN] {}", redact_path(&e.to_string()));
            std::process::exit(2);
        }
    }
}

/// Free-form generation → streamed stdout. tokens=stdout, [QM-*]=stderr.
async fn run_prompt_cmd(args: PromptArgs) {
    use quantamind_lib::cli::prompt::{run_prompt, PromptOptions, PromptOutcome};
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = QmConfig::load(&cwd);
    let backend = resolve_backend(args.backend, &cfg).await;
    let api_key = resolve_key(Some(backend));
    let base = args.base.or_else(|| cfg.as_ref().and_then(|c| c.base.clone()));
    let model = match args.model.or_else(|| cfg.as_ref().map(|c| c.model.clone())) {
        Some(m) => m,
        None => match pick_model(backend, base.as_deref(), api_key.as_deref()).await {
            Some(m) => m,
            None => {
                eprintln!("[QM-NO-MODEL] no model — pass --model, run `qm init`, or run in a terminal to pick one.");
                std::process::exit(2);
            }
        },
    };
    // Resolve the user prompt: --user wins; else read stdin.
    let user = match args.user {
        Some(u) => u,
        None => {
            use std::io::Read;
            let mut buf = String::new();
            if std::io::stdin().read_to_string(&mut buf).is_err() || buf.trim().is_empty() {
                eprintln!("[QM-NO-PROMPT] no prompt — pass --user \"…\" or pipe it on stdin.");
                std::process::exit(2);
            }
            buf
        }
    };
    let opts = PromptOptions { backend, model, base, api_key, system: args.system, user, params: args.params.resolve() };
    match run_prompt(opts).await {
        Err(e) => {
            eprintln!("[QM-INTERNAL] generation failed: {}", redact_path(&e.to_string()));
            std::process::exit(1);
        }
        Ok(PromptOutcome::Unreachable { backend, endpoint }) => {
            eprintln!("[QM-BACKEND-UNREACHABLE] {} not reachable at {endpoint} — run `qm doctor` to diagnose.", label(backend));
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        Ok(PromptOutcome::ModelNotFound { backend, model, available }) => {
            eprintln!("[QM-MODEL-NOT-FOUND] {} has no model '{model}' — available: {}", label(backend), if available.is_empty() { "(none)".into() } else { available.join(", ") });
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        Ok(PromptOutcome::Done { tokens }) => {
            eprintln!("[QM-DONE] {tokens} tokens");
            std::process::exit(0);
        }
    }
}

/// Validate a collection/world — the detailed-report form of the run gate.
async fn run_validate_cmd(args: ValidateArgs) {
    use quantamind_lib::cli::validate::{render_validation, run_validate, validate_exit, ValidateOutcome};
    let collection = resolve_collection(args.collection);
    match run_validate(&collection, args.live_world).await {
        Err(e) => {
            eprintln!("[QM-INTERNAL] validation failed: {}", redact_path(&e.to_string()));
            std::process::exit(1);
        }
        Ok(ValidateOutcome::UnknownCollection { id }) => {
            eprintln!("[QM-BAD-COLLECTION] unknown collection '{id}'.");
            std::process::exit(2);
        }
        Ok(ValidateOutcome::BadFile { path, reason }) => {
            eprintln!("[QM-BAD-COLLECTION] could not load '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        Ok(ValidateOutcome::DepsMissing { fix, validation }) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&validation).unwrap_or_default());
            } else {
                print!("{}", render_validation(&validation));
            }
            eprintln!("[QM-WORLD-DEPS] world tasks could not be live-checked — {fix}. Install, then re-run `qm validate`.");
            std::process::exit(11); // inconclusive: the worlds were NOT proven
        }
        Ok(ValidateOutcome::Done(v)) => {
            if args.json {
                println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
            } else {
                print!("{}", render_validation(&v));
            }
            std::process::exit(validate_exit(&v));
        }
    }
}

/// Offline re-assessment of a saved run against a profile — no backend.
fn run_report(args: ReportArgs) {
    match run::assess_saved(&args.report.to_string_lossy(), &args.profile) {
        ReportOutcome::BadReportFile { path, reason } => {
            eprintln!("[QM-BAD-REPORT] could not load report '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        ReportOutcome::UnknownProfile { id } => {
            eprintln!("[QM-BAD-PROFILE] unknown profile '{id}' — try general-agent, rag-assistant, coding-agent, or a .json file.");
            std::process::exit(2);
        }
        ReportOutcome::BadProfileFile { path, reason } => {
            eprintln!("[QM-BAD-PROFILE] could not load profile '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        ReportOutcome::Ran(report) => finish_ran(&report, args.json, FailOn::from(args.fail_on), args.junit, Render::Verdict),
    }
}

async fn run_test(args: TestArgs) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = QmConfig::load(&cwd);
    let backend = resolve_backend(args.backend, &cfg).await;
    let api_key = resolve_key(Some(backend));
    let base = args.base.or_else(|| cfg.as_ref().and_then(|c| c.base.clone()));
    let model = match args.model.or_else(|| cfg.as_ref().map(|c| c.model.clone())) {
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
        // No default price: absent `costs` in qm.json means every USD figure is n/a.
        cost_config: cfg.as_ref().and_then(|c| c.costs).unwrap_or_default(),
        // The engine's loader treats a path with a separator / `.json` as a file.
        collection: args.collection.to_string_lossy().into_owned(),
        base,
        api_key,
        k: args.k,
        tier: args.tier.map(Tier::from),
        think: ThinkPreset::from(args.thinking),
        mode: RunMode::from(args.mode),
        profile_id: args.profile,
        save_report: args.save_report,
        save_transcripts: args.save_transcripts,
        max_steps: args.max_steps,
        decoy_tools: args.decoy,
        params: args.params.resolve(),
        costs: args.costs,
    };
    execute(opts, args.json, FailOn::from(args.fail_on), args.junit, Render::Scoreboard).await;
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
        // No default price: absent `costs` in qm.json means every USD figure is n/a.
        cost_config: cfg.as_ref().and_then(|c| c.costs).unwrap_or_default(),
        collection: resolve_collection(args.collection),
        base,
        api_key,
        k: args.k,
        tier: args.tier.map(Tier::from),
        think: resolve_thinking(args.thinking),
        mode: RunMode::from(args.mode),
        profile_id: args.profile,
        save_report: args.save_report,
        save_transcripts: args.save_transcripts,
        max_steps: args.max_steps,
        decoy_tools: args.decoy,
        params: args.params.resolve(),
        costs: args.costs,
    };
    execute(opts, args.json, FailOn::from(args.fail_on), args.junit, Render::Verdict).await;
}

/// Context Stress Test — prompt-based, greedy; exit no-cliff 0 / collapsed 10 /
/// inconclusive 11 / broken 20 (the documented contract).
async fn run_cliff_cmd(args: CliffArgs) {
    let cwd = std::env::current_dir().unwrap_or_default();
    let cfg = QmConfig::load(&cwd);
    let backend = resolve_backend(args.backend, &cfg).await;
    let api_key = resolve_key(Some(backend));
    let base = args.base.or_else(|| cfg.as_ref().and_then(|c| c.base.clone()));
    let model = match args.model.or_else(|| cfg.as_ref().map(|c| c.model.clone())) {
        Some(m) => m,
        None => match pick_model(backend, base.as_deref(), api_key.as_deref()).await {
            Some(m) => m,
            None => {
                eprintln!("[QM-NO-MODEL] no model — pass --model, run `qm init`, or run in a terminal to pick one.");
                std::process::exit(2);
            }
        },
    };
    let opts = CliffOptions {
        run: RunOptions {
            backend,
            model,
            // `qm cliff` doesn't emit the dollar block; no price basis needed.
            cost_config: Default::default(),
            collection: resolve_collection(args.collection),
            base,
            api_key,
            k: None,
            tier: None,
            think: ThinkPreset::from(args.thinking),
            mode: RunMode::PromptBased, // ignored by the cliff engine; `native` below picks the path
            profile_id: "general-agent".into(),
            save_report: None,
        save_transcripts: None,
            max_steps: None,
            decoy_tools: None,
            params: None, // cliff params flow via CliffOptions below, not RunOptions
            costs: false, // the cliff probe reports its own ladder, not run costs
        },
        max_tokens: args.max_tokens,
        steps: args.steps,
        source: CliffSource::from(args.source),
        native: matches!(args.mode, ModeArg::Native),
        params: args.params.resolve(),
        cap: args.cap,
    };
    match cliff::run_cliff_probe(opts).await {
        Err(e) => {
            eprintln!("[QM-INTERNAL] cliff probe failed: {}", redact_path(&e.to_string()));
            std::process::exit(1);
        }
        Ok(CliffOutcome::Unreachable { backend, endpoint }) => {
            eprintln!("[QM-BACKEND-UNREACHABLE] {} not reachable at {endpoint} — run `qm doctor` to diagnose.", label(backend));
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        Ok(CliffOutcome::ModelNotFound { backend, model, available }) => {
            eprintln!("[QM-MODEL-NOT-FOUND] {} has no model '{model}' — available: {}", label(backend), if available.is_empty() { "(none)".into() } else { available.join(", ") });
            std::process::exit(run::render::EXIT_UNREACHABLE);
        }
        Ok(CliffOutcome::UnknownCollection { id }) => {
            eprintln!("[QM-BAD-COLLECTION] unknown collection '{id}'.");
            std::process::exit(2);
        }
        Ok(CliffOutcome::BadCollectionFile { path, reason }) => {
            eprintln!("[QM-BAD-COLLECTION] could not load collection file '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        Ok(CliffOutcome::WindowTooSmall { running_ctx, needed_ctx, usable_max_tokens }) => {
            eprintln!(
                "[QM-WINDOW-TOO-SMALL] the running llama-server has a {running_ctx}-token context window, \
                 but this ladder needs about {needed_ctx} (max-tokens + headroom incl. any thinking budget). \
                 Either relaunch llama-server with a larger window (-c {needed_ctx} or more), or reduce \
                 --max-tokens to {usable_max_tokens} or less."
            );
            std::process::exit(2);
        }
        Ok(CliffOutcome::NativeUnsupported { backend, model }) => {
            let hint = match backend {
                _ => "the server can't run native tool-calling here — relaunch llama-server with --jinja and a tool-capable model, or use --mode prompt_based",
            };
            eprintln!("[QM-NATIVE-UNSUPPORTED] --mode native won't run for '{model}' on {}: {hint}.", label(backend));
            std::process::exit(2);
        }
        Ok(CliffOutcome::ThinkingUnsupported { backend, model }) => {
            // Same hints as `qm run` — reasoning fails for different reasons per engine.
            let hint = match backend {
                BackendKind::LlamaCpp => "the server returned no reasoning — relaunch it with `--jinja --reasoning-format deepseek` and use a reasoning model, or use --thinking lean",
                _ => "the server returned no reasoning — enable its reasoning parser, or use --thinking lean",
            };
            eprintln!("[QM-THINKING-UNSUPPORTED] --thinking won't take effect for '{model}' on {}: {hint}.", label(backend));
            std::process::exit(2);
        }
        Ok(CliffOutcome::Probed(report)) => {
            if args.json {
                match serde_json::to_string_pretty(&report) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("[QM-INTERNAL] failed to serialize report: {e}");
                        std::process::exit(1);
                    }
                }
            } else {
                print!("{}", render_cliff(&report));
            }
            std::process::exit(cliff_exit(&report.status));
        }
    }
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

/// Resolve the backend: flag → qm.json → (interactive pick among runnable) → the server.
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
    BackendKind::LlamaCpp
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

/// Resolve `--collection`: explicit value wins; omitted in a terminal → a numbered
/// picker of the BUILT-IN collections with their tier (easy/medium/hard/extreme);
/// omitted non-TTY (CI/pipe) → the `easy-coding` default, never a prompt.
fn resolve_collection(arg: Option<String>) -> String {
    if let Some(c) = arg {
        return c;
    }
    if is_interactive() {
        let infos = list_builtin_collections();
        let rows: Vec<String> = infos.iter().map(|c| format!("{:<28} [{:<7}] {}", c.id, c.tier, c.domain)).collect();
        if let Some(i) = select("Select a built-in collection (id · tier · domain):", &rows) {
            return infos[i].id.clone();
        }
    }
    "easy-coding".into()
}

/// Resolve `--thinking`: explicit wins; omitted in a terminal → pick the thinking
/// tier (lean = reasoning OFF); omitted non-TTY → lean (the safe default — standard/
/// deep are guarded per-model anyway).
fn resolve_thinking(arg: Option<ThinkingArg>) -> ThinkPreset {
    if let Some(t) = arg {
        return ThinkPreset::from(t);
    }
    if is_interactive() {
        let rows = vec![
            "lean     — reasoning OFF (any model)".to_string(),
            "standard — thinking on, 2k-scale scratchpad (reasoning models only)".to_string(),
            "deep     — thinking on, 8k-scale scratchpad (reasoning models only)".to_string(),
        ];
        if let Some(i) = select("Select the thinking tier:", &rows) {
            return match i {
                1 => ThinkPreset::Standard,
                2 => ThinkPreset::Deep,
                _ => ThinkPreset::Lean,
            };
        }
    }
    ThinkPreset::Lean
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
        cost_config: cfg.costs.unwrap_or_default(),
        collection: cfg.collection,
        base: cfg.base,
        api_key: resolve_key(Some(cfg.backend)),
        k: None,
        tier: None,
        think: ThinkPreset::Lean,
        mode: RunMode::PromptBased,
        profile_id: cfg.profile,
        save_report: None,
        save_transcripts: None,
        max_steps: None,
        decoy_tools: None,
        params: None,
        costs: false, // the init smoke run keeps its output minimal
    };
    execute(opts, args.json, FailOn::Conditional, None, Render::Verdict).await;
}

/// Human render style for a completed run.
#[derive(Clone, Copy)]
enum Render {
    Verdict,
    Scoreboard,
}

/// Run a suite and exit on the verdict — shared by `run`, `init`, and `test`.
async fn execute(opts: RunOptions, json: bool, fail_on: FailOn, junit: Option<PathBuf>, render: Render) {
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
        RunOutcome::BadCollectionFile { path, reason } => {
            eprintln!("[QM-BAD-COLLECTION] could not load collection file '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        RunOutcome::UnknownProfile { id } => {
            eprintln!("[QM-BAD-PROFILE] unknown profile '{id}' — try general-agent, rag-assistant, or coding-agent.");
            std::process::exit(2);
        }
        RunOutcome::BadProfileFile { path, reason } => {
            eprintln!("[QM-BAD-PROFILE] could not load profile file '{}': {reason}", redact_path(&path));
            std::process::exit(2);
        }
        RunOutcome::NativeUnsupported { backend, model } => {
            eprintln!("[QM-NATIVE-UNSUPPORTED] {} has no native tool-calling for '{model}' — use --mode prompt_based or both.", label(backend));
            std::process::exit(2);
        }
        RunOutcome::ThinkingUnsupported { backend, model } => {
            // A backend-specific, actionable fix — reasoning fails for different reasons per engine.
            let hint = match backend {
                BackendKind::LlamaCpp => "the server returned no reasoning — relaunch it with `--jinja --reasoning-format deepseek` and use a reasoning model, or use --thinking lean",
                _ => "the server returned no reasoning — enable its reasoning parser, or use --thinking lean",
            };
            eprintln!("[QM-THINKING-UNSUPPORTED] --thinking won't take effect for '{model}' on {}: {hint}.", label(backend));
            std::process::exit(2);
        }
        RunOutcome::Inconclusive { reason } => {
            eprintln!("[QM-INCONCLUSIVE] the run errored before it could measure anything — retry. ({reason})");
            std::process::exit(run::render::EXIT_INCONCLUSIVE);
        }
        RunOutcome::CollectionInvalid { findings } => {
            // The mandatory gate: an uploaded collection with a broken answer key must
            // never start testing — a pass^k from an invalid world would be a lie.
            eprintln!("[QM-COLLECTION-INVALID] {} finding(s) — testing not started. Fix these, or run `qm validate` for the full report:", findings.len());
            for f in &findings {
                eprintln!("  ✗ {f}");
            }
            std::process::exit(20);
        }
        RunOutcome::WorldDepsMissing { fix } => {
            eprintln!("[QM-WORLD-DEPS] {fix} — install, then re-run.");
            std::process::exit(2);
        }
        RunOutcome::Ran(report) => finish_ran(&report, json, fail_on, junit, render),
    }
}

/// Render a completed run (verdict or scoreboard), write JUnit if asked, and exit on
/// the verdict — shared by the live `execute` path and the offline `report` path.
fn finish_ran(report: &run::RunReport, json: bool, fail_on: FailOn, junit: Option<PathBuf>, render: Render) -> ! {
    let status = report.worst_status();
    if json {
        match serde_json::to_string_pretty(report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("[QM-INTERNAL] failed to serialize report: {e}");
                std::process::exit(1);
            }
        }
    } else {
        match render {
            Render::Verdict => print!("{}", run::render_human(report)),
            Render::Scoreboard => print!("{}", run::render_scoreboard(report)),
        }
    }
    // JUnit is a side artifact (data → a file), independent of --json/stdout.
    if let Some(path) = junit {
        if let Err(e) = std::fs::write(&path, run::to_junit(report)) {
            eprintln!("[QM-INTERNAL] could not write JUnit report: {}", redact_path(&e.to_string()));
        }
    }
    let code = run::exit_code(status, fail_on);
    // Note when a soft policy downgraded a non-Ready verdict to a pass.
    if code == 0 && status != quantamind_lib::inference::eval::readiness::types::Readiness::Ready {
        eprintln!("[QM-NOTE] verdict is {status:?} but --fail-on let it pass (exit 0).");
    }
    std::process::exit(code);
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

/// `qm certify` — gate a deploy on the customer's own agent.
///
/// Everything that can reject the suite runs before any agent process starts, so a
/// broken suite costs zero agent invocations (and zero of their model spend).
fn run_certify_cmd(args: CertifyArgs) {
    use quantamind_lib::cli::certify::{
        command::AgentCommand, render::render, run_certify_suite, suite, CertifyOptions,
    };

    let command = match AgentCommand::new(&args.agent, args.clean_env, args.env.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[QM-BAD-AGENT-COMMAND] {e}");
            std::process::exit(2);
        }
    };
    let tasks = match suite::load(&args.suite) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[QM-BAD-SUITE] {e}");
            std::process::exit(2);
        }
    };
    if args.no_precheck {
        eprintln!(
            "[QM-NOTE] --no-precheck: the anti-vacuity check is OFF. A task a do-nothing agent \
             passes will now run and report green."
        );
    }
    if args.k == Some(0) {
        eprintln!("[QM-BAD-PARAM] --k 0: a task that never runs cannot pass");
        std::process::exit(2);
    }

    let opts = CertifyOptions {
        command,
        timeout: std::time::Duration::from_secs(args.timeout),
        kill_grace: std::time::Duration::from_secs(args.kill_grace),
        k_override: args.k,
        fail_on: args.fail_on.into(),
        quiet_agent: args.quiet_agent,
        no_precheck: args.no_precheck,
    };
    let outcome = run_certify_suite(&tasks, &opts);
    std::process::exit(render(&outcome, args.fail_on.into()));
}

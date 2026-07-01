// The backend runs no `unsafe` of its own — all native work (HTTP, audio, process
// control) goes through safe crates. Deny it so a future `unsafe` block is a
// conscious, reviewed exception, not an accident (guide Part 7).
#![deny(unsafe_code)]

pub mod commands;
pub mod errors;
pub mod inference;
pub mod metrics;
pub mod os;
pub mod persistence;
pub mod sync;
pub mod time_iso;
pub mod validation;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(commands::prompt::prompt::RunState::default())
        .manage(commands::models::models_pull::PullState::default())
        .manage(commands::hf::hf_install::HfInstallState::default())
        .manage(commands::compare::compare::CompareRunState::default())
        .manage(commands::settings::model_settings::ModelSettingsState::default())
        .manage(commands::ollama::ollama_start::OllamaStartState::default())
        .manage(commands::llama::llama_server_types::LlamaServerState::default())
        .manage(commands::mlx::mlx_server_types::MlxServerState::default())
        .manage(commands::stt::stt_server_types::SttServerState::default())
        .manage(commands::stt::stt_download::SttInstallState::default())
        .manage(commands::audio::capture::CaptureState::default())
        .manage(commands::workspace::workspaces::WorkspaceState::default())
        .manage(commands::settings::user_settings::UserSettingsState::default())
        .manage(commands::eval::batch_cmd::BatchRunState::default())
        .manage(commands::eval::readiness_cmd::CliffRunState::default())
        .manage(commands::publish::auth_state::AuthState::default())
        .setup(|app| {
            // Reclaim any sidecars a previous instance orphaned by dying without
            // reaping (a crash or dev-restart SIGKILL), so a stale server can't
            // keep holding a port. Conservative: only our-signature processes.
            let reclaimed = commands::app_lifecycle::sweep_orphans();
            if reclaimed > 0 {
                eprintln!("[reap] reclaimed {reclaimed} orphaned server(s) at startup");
            }
            // Reap our servers on SIGINT/SIGTERM too — ExitRequested only fires on
            // a graceful quit (Cmd+Q), not when a signal kills the process.
            commands::app_lifecycle::install_signal_reaper(app.handle().clone());
            // Sweep any half-installed STT artifacts left by a prior crash, so a
            // model reads installed only when its real files are present (R3).
            let _ = commands::stt::stt_disk::reconcile_stt_dir(&commands::stt::stt_disk::stt_dir());
            // Phase 4: on Windows only, warn if legacy `~/.quantamind/*` folders
            // exist alongside the new `%LOCALAPPDATA%\QuantaMind` default. Never
            // auto-move — user weights are irreplaceable.
            commands::storage::storage_disk::warn_on_legacy_windows_paths();
            // Clear leftover recording scratch from a prior session.
            commands::stt::transcribe::clear_scratch(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // macOS: closing the window does NOT quit a Tauri app — it lingers in the
            // dock, so `RunEvent::ExitRequested` never fires and the Ollama WE spawned
            // keeps running. Reap our sidecars and quit so "close the app" actually frees
            // them. `reap_managed` is idempotent and PID-scoped (a pre-existing user
            // Ollama daemon is never touched); Cmd+Q and SIGINT/SIGTERM still reap too.
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                let app = tauri::Manager::app_handle(window);
                commands::app_lifecycle::reap_managed(app);
                app.exit(0);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::gguf::gguf_cmd::inspect_gguf,
            commands::system::hardware::get_hardware_snapshot,
            commands::system::loaded_models::get_loaded_models,
            commands::system::process_memory::get_ollama_rss,
            commands::compare::compare::run_compare,
            commands::compare::compare::stop_compare,
            commands::compare::compare_export::save_compare_report,
            commands::publish::export_cmd::save_readiness_image,
            #[cfg(not(feature = "enterprise"))]
            commands::publish::preview_cmd::preview_publish_payload,
            #[cfg(not(feature = "enterprise"))]
            commands::publish::publish_cmd::publish_to_board,
            #[cfg(not(feature = "enterprise"))]
            commands::publish::identity::login_cmd::start_login,
            commands::gguf::gguf_cmd::install_local_gguf,
            commands::hf::hf_browse::hf_search,
            commands::hf::hf_browse::hf_repo_files,
            commands::hf::hf_browse::hf_repo_all_files,
            commands::hf::hf_card::hf_model_card,
            commands::hf::hf_install::install_hf_gguf,
            commands::hf::hf_install::cancel_hf_install,
            commands::system::health::check_ollama_health,
            commands::mlx::health_mlx::check_mlx_health,
            commands::llama::llama_runtime::check_llama_health,
            commands::mlx::mlx_models::list_mlx_models,
            commands::mlx::mlx_models::delete_mlx_model,
            commands::mlx::mlx_install::install_mlx_model,
            commands::mlx::mlx_start::start_mlx_server,
            commands::mlx::mlx_start::stop_mlx_server,
            commands::mlx::mlx_start::mlx_server_status,
            commands::settings::model_settings::get_model_settings,
            commands::settings::model_settings::set_model_temperature,
            commands::settings::model_settings::set_model_thinking,
            commands::models::models::list_models,
            commands::models::model_inspect::inspect_model,
            commands::models::model_inspect::estimate_kv_cache_bytes,
            commands::models::models_pull::pull_model,
            commands::models::models_pull::cancel_pull,
            commands::ollama::ollama_start::start_ollama,
            commands::ollama::ollama_start::stop_ollama,
            commands::ollama::ollama_start::ollama_auto_start_supported,
            commands::llama::llama_start::start_llama_server,
            commands::llama::llama_start::stop_llama_server,
            commands::llama::llama_start::llama_server_info,
            commands::llama::llama_models::list_llama_models,
            commands::llama::llama_models::delete_llama_model,
            commands::stt::stt_start::start_whisper_server,
            commands::stt::stt_start::stop_whisper_server,
            commands::stt::stt_start::check_whisper_env,
            commands::stt::stt_health::check_whisper_health,
            commands::stt::stt_download::download_stt_model,
            commands::stt::stt_download::cancel_stt_install,
            commands::stt::stt_download::list_stt_catalog,
            commands::stt::stt_models::list_installed_stt_models,
            commands::stt::stt_models::delete_stt_model,
            commands::stt::transcribe::transcribe_audio,
            commands::stt::transcribe::load_transcript,
            commands::stt::eval::eval_cmd::run_stt_eval,
            commands::stt::eval::eval_cmd::list_transcripts,
            commands::stt::eval::eval_cmd::list_stt_evals,
            commands::stt::eval::eval_cmd::load_stt_eval,
            commands::stt::eval::eval_cmd::save_stt_eval,
            commands::stt::eval::eval_cmd::delete_stt_eval,
            commands::stt::eval::eval_cmd::load_stt_report,
            commands::stt::eval::readiness_cmd::assess_stt_readiness,
            commands::stt::eval::readiness_cmd::list_stt_readiness_profiles,
            commands::stt::eval::readiness_cmd::save_stt_readiness_profile,
            commands::stt::eval::readiness_cmd::delete_stt_readiness_profile,
            commands::audio::capture::start_recording,
            commands::audio::capture::stop_recording,
            commands::audio::capture::recording_level,
            commands::settings::settings::get_storage_path,
            commands::settings::settings::validate_storage_path,
            commands::storage::storage::get_installed_models_with_stats,
            commands::storage::storage::remove_model,
            commands::storage::storage_cache::clear_app_cache,
            commands::storage::storage_usage::get_disk_usage,
            commands::prompt::prompt::run_prompt,
            commands::prompt::prompt::stop_prompt,
            commands::workspace::workspaces::open_workspace,
            commands::workspace::workspaces::close_workspace,
            commands::workspace::workspaces::current_workspace,
            commands::workspace::workspaces::list_workspace_tree,
            commands::workspace::workspaces::recent_workspaces,
            commands::prompt_templates::templates::list_prompt_templates,
            commands::llama::llama_templates::list_chat_templates,
            commands::eval::evals_load::list_evals,
            commands::eval::eval_run::run_eval_task,
            commands::eval::toolcall_cmd::run_toolcall_eval,
            commands::eval::toolcall_cmd::trace_toolcall_task,
            commands::eval::toolcall_cmd::load_toolcall_trace,
            commands::eval::toolcall_cmd::list_builtin_collections,
            commands::eval::toolcall_cmd::get_builtin_collection,
            commands::eval::eval_registry::list_custom_collections,
            commands::eval::eval_registry::load_custom_collection,
            commands::eval::eval_registry::save_custom_collection,
            commands::eval::eval_registry::delete_custom_collection,
            commands::eval::eval_registry::import_custom_collection,
            commands::eval::eval_registry::read_text_capped,
            commands::eval::matrix_cmd::run_collection_matrix,
            commands::eval::matrix_cmd::load_collection_history,
            commands::eval::batch_cmd::run_batch_eval,
            commands::eval::batch_cmd::stop_batch_eval,
            commands::eval::batch_cmd::check_unfinished_run,
            commands::eval::batch_cmd::resume_batch_eval,
            commands::eval::batch_cmd::discard_run,
            commands::eval::readiness_cmd::list_readiness_profiles,
            commands::eval::readiness_cmd::save_readiness_profile,
            commands::eval::readiness_cmd::delete_readiness_profile,
            commands::eval::readiness_cmd::assess_readiness,
            commands::eval::readiness_cmd::get_hardware_tier,
            commands::eval::readiness_cmd::save_cliff_result,
            commands::eval::readiness_cmd::get_cliff_results,
            commands::eval::readiness_cmd::run_context_cliff,
            commands::eval::readiness_cmd::stop_context_cliff,
            commands::workspace::workspace_prompts::load_prompt,
            commands::workspace::workspace_prompts::save_prompt,
            commands::workspace::workspace_prompts::create_prompt,
            commands::workspace::workspace_prompts::rename_path,
            commands::workspace::workspace_prompts::delete_path,
            commands::workspace::history::history_append,
            commands::workspace::history::history_list,
            commands::workspace::history::history_get,
            commands::workspace::history::history_clear,
            commands::workspace::history::history_remove_by_path,
            commands::settings::user_settings::get_user_settings,
            commands::settings::user_settings::set_user_settings,
            commands::settings::user_settings::resolve_models_folder,
            commands::system::onboarding::scaffold_onboarding_workspace,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(commands::app_lifecycle::reap_on_exit);
}

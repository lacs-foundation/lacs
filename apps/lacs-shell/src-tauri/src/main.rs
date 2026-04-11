mod commands;
mod daemon_client;
mod events;

use commands::{approve_preview, cancel_job, get_brain_config, plan_intent, ShellCommandState};

fn main() {
    // Apply config-file defaults before the Tauri runtime spawns worker threads.
    lacs_core::config::LacsConfig::load().apply_defaults_to_env();

    tauri::Builder::default()
        .setup(|_app| {
            #[cfg(any(test, feature = "demo"))]
            eprintln!(
                "[LACS WARNING] Running with DemoStateClient — \
                 system state is fabricated. \
                 Disable the 'demo' feature to query the live lacs-daemon."
            );
            Ok(())
        })
        .manage(ShellCommandState::new())
        .invoke_handler(tauri::generate_handler![
            approve_preview,
            cancel_job,
            get_brain_config,
            plan_intent,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run lacs-shell");
}

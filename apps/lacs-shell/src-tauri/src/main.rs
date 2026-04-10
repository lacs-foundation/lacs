mod commands;
mod daemon_client;
mod events;

use commands::{
    approve_preview, plan_intent, publish_job_outcome, publish_timeline_event, ShellCommandState,
};

fn main() {
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
            plan_intent,
            publish_job_outcome,
            publish_timeline_event
        ])
        .run(tauri::generate_context!())
        .expect("failed to run lacs-shell");
}

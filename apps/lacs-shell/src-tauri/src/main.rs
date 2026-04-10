mod commands;
mod events;

use commands::{
    approve_preview, plan_intent, publish_job_outcome, publish_timeline_event, ShellCommandState,
};

fn main() {
    tauri::Builder::default()
        .setup(|_app| {
            // NOTE(task-8): System state is fabricated by DemoStateClient.
            // Plans reflect a hardcoded Silverblue fixture, not the real machine.
            // Replace DemoStateClient with a real daemon IPC client before
            // shipping to production.
            eprintln!(
                "[LACS WARNING] Running with DemoStateClient — \
                 system state is fabricated. \
                 Replace with real daemon IPC (task-8) before production use."
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

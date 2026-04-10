mod commands;
mod events;

use commands::{approve_preview, plan_intent, publish_job_outcome, publish_timeline_event};

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            approve_preview,
            plan_intent,
            publish_job_outcome,
            publish_timeline_event
        ])
        .run(tauri::generate_context!())
        .expect("failed to run lacs-shell");
}

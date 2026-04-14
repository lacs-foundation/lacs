mod approval;
mod cli;
mod client;
mod error;
mod runner;

use clap::Parser;

use crate::cli::{Cli, Command};
use crate::runner::{Logger, RunOpts};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Resolve socket path once for all subcommands.
    let socket = runner::resolve_socket();

    // Set up logger (tee to file when --log-to is present).
    let log = match Logger::new(cli.log_to.as_deref()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("lacs: {e}");
            std::process::exit(4);
        }
    };

    // Dispatch.
    let result: Result<(), crate::error::CliError> = async {
        // Wrap everything in an optional hard timeout.
        if let Some(secs) = cli.timeout {
            tokio::time::timeout(
                std::time::Duration::from_secs(secs),
                dispatch(&cli, socket, &log),
            )
            .await
            .unwrap_or_else(|_| {
                Err(crate::error::CliError::ExecutionFailed(format!(
                    "operation timed out after {secs}s"
                )))
            })
        } else {
            dispatch(&cli, socket, &log).await
        }
    }
    .await;

    if let Err(e) = result {
        eprintln!("lacs: {e}");
        std::process::exit(e.exit_code());
    }
}

async fn dispatch(
    cli: &Cli,
    socket: std::path::PathBuf,
    log: &Logger,
) -> Result<(), crate::error::CliError> {
    match &cli.command {
        // --- lacs completions <shell> ---
        Some(Command::Completions { shell }) => {
            runner::run_completions(*shell);
            Ok(())
        }

        // --- lacs doctor ---
        Some(Command::Doctor) => {
            runner::run_doctor(socket, cli.json, log).await
        }

        // --- lacs history [flags] ---
        Some(Command::History(args)) => {
            runner::run_history(args.clone(), socket, cli.json, log).await
        }

        // --- lacs <intent words ...> ---
        Some(Command::Intent(_)) => {
            let intent = cli
                .command
                .as_ref()
                .unwrap()
                .intent_string()
                .expect("Intent variant always has a string");
            let opts = build_run_opts(cli, socket);
            runner::run_intent(intent, &opts, log).await
        }

        // --- lacs  (no subcommand → REPL) ---
        None => {
            let opts = build_run_opts(cli, socket);
            runner::run_repl(&opts, log).await
        }
    }
}

fn build_run_opts(cli: &Cli, socket: std::path::PathBuf) -> RunOpts {
    RunOpts {
        socket,
        yes: cli.yes,
        max_risk: cli.max_risk.map(crate::approval::MaxRisk::from),
        non_interactive: cli.non_interactive,
        dry_run: cli.dry_run,
        json: cli.json,
        step_by_step: cli.step_by_step,
    }
}

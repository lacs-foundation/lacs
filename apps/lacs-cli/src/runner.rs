//! Top-level dispatch for all `lacs` CLI commands.
//!
//! Each public `run_*` function corresponds to one subcommand or the
//! free-form intent path.  All printed output goes through [`Logger`] so
//! that `--log-to` tee works transparently.
//!
//! ## Approval flow
//!
//! Without `--step-by-step`: [`ApprovalPolicy::decide_plan`] is called once
//! for the whole plan.  If a single confirmation is needed the user is asked
//! once, then all steps execute in sequence.
//!
//! With `--step-by-step`: [`ApprovalPolicy::decide_step`] is called before
//! each step so the user can approve or reject them individually.
//!
//! `--dry-run` short-circuits before any execution: the plan is printed and
//! the function returns `Ok(())`.

use std::io::{self, Write as _};
use std::path::PathBuf;

use clap::CommandFactory;
use lacs_brain::config::BrainConfig;
use lacs_brain::planner::{LlmPlanner, PlanRiskLevel};
use lacs_types::{PreviewEnvelope, ResultEnvelope};
use serde_json::{json, Value};

use lacs_brain::state_client::StateClient as _;

use crate::approval::{ApprovalDecision, ApprovalPolicy, MaxRisk};
use crate::cli::{Cli, HistoryArgs};
use crate::client::DaemonClient;
use crate::error::CliError;

// ---------------------------------------------------------------------------
// resolve_socket
// ---------------------------------------------------------------------------

/// Returns the daemon socket path from `$LACS_SOCKET`, falling back to the
/// system-wide default `/run/lacs/lacs.sock`.
pub fn resolve_socket() -> PathBuf {
    std::env::var_os("LACS_SOCKET")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/run/lacs/lacs.sock"))
}

// ---------------------------------------------------------------------------
// since_to_hours
// ---------------------------------------------------------------------------

/// Parse an RFC 3339 / ISO-8601 UTC datetime string and return the number of
/// whole hours that have elapsed since that moment.
///
/// Returns `None` when:
/// - the string is not a valid UTC timestamp (`Z` or `+00:00` suffix),
/// - the datetime is in the future, or
/// - the value is too large to fit in `u32`.
///
/// Sub-second precision (`.NNN`) is accepted and truncated.  Non-zero UTC
/// offsets are not supported and return `None`.
pub fn since_to_hours(s: &str) -> Option<u32> {
    let epoch = rfc3339_to_unix(s)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs() as i64;
    if epoch > now {
        return None;
    }
    u32::try_from((now - epoch) / 3600).ok()
}

/// Parse a UTC RFC 3339 string to seconds since Unix epoch (no external dep).
///
/// Supports `YYYY-MM-DDThh:mm:ssZ` and `YYYY-MM-DDThh:mm:ss+00:00`.
/// Sub-second fractions are stripped.
///
/// Uses Howard Hinnant's civil day algorithm to convert a proleptic-Gregorian
/// date to a day count, then scales to seconds.
fn rfc3339_to_unix(s: &str) -> Option<i64> {
    // Strip the UTC timezone suffix.
    let s = if let Some(prefix) = s.strip_suffix('Z') {
        prefix
    } else if let Some(prefix) = s.strip_suffix("+00:00") {
        prefix
    } else {
        return None;
    };

    // Split on the 'T' separator.
    let (date_part, time_and_frac) = s.split_once('T')?;

    // Drop sub-second fractions: keep only up to "hh:mm:ss".
    let time_part = &time_and_frac[..time_and_frac.find('.').unwrap_or(time_and_frac.len())];
    if time_part.len() < 8 {
        return None;
    }

    // Parse date components.
    let mut date_iter = date_part.splitn(4, '-');
    let y: i64 = date_iter.next()?.parse().ok()?;
    let m: i64 = date_iter.next()?.parse().ok()?;
    let d: i64 = date_iter.next()?.parse().ok()?;
    if date_iter.next().is_some() {
        return None; // extra segments → reject
    }

    // Parse time components.
    let mut time_iter = time_part.splitn(4, ':');
    let h: i64 = time_iter.next()?.parse().ok()?;
    let mn: i64 = time_iter.next()?.parse().ok()?;
    let sec: i64 = time_iter.next()?.parse().ok()?;
    if time_iter.next().is_some() {
        return None; // extra segments → reject
    }

    // Range validation.
    if !(1..=12).contains(&m)
        || !(1..=31).contains(&d)
        || h > 23
        || mn > 59
        || sec > 60 // allow leap second
    {
        return None;
    }

    // Howard Hinnant's civil_from_days: compute days since 1970-01-01.
    //
    // Reference: https://howardhinnant.github.io/date_algorithms.html
    // The civil epoch starts on 0000-03-01; shift y back by 1 for Jan/Feb so
    // Feb 29 falls at the end of its civil year.
    let z = if m > 2 { y } else { y - 1 };
    let era = (if z >= 0 { z } else { z - 399 }) / 400;
    let yoe = z - era * 400; // year-of-era [0, 399]
    let m_adj = if m > 2 { m - 3 } else { m + 9 }; // month-of-civil-year [0, 11]
    let doy = (153 * m_adj + 2) / 5 + d - 1; // day-of-year from Mar 1
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // day-of-era
    let days = era * 146097 + doe - 719468; // days since 1970-01-01

    Some(days * 86_400 + h * 3600 + mn * 60 + sec)
}

// ---------------------------------------------------------------------------
// highest_risk
// ---------------------------------------------------------------------------

/// Return the highest risk level present in `plan`, or `None` when the plan
/// has no steps.
pub fn highest_risk(plan: &lacs_brain::planner::Plan) -> Option<&PlanRiskLevel> {
    plan.steps()
        .iter()
        .max_by_key(|s| match s.risk_level() {
            PlanRiskLevel::Low => 0u8,
            PlanRiskLevel::Medium => 1,
            PlanRiskLevel::High => 2,
        })
        .map(|s| s.risk_level())
}

// ---------------------------------------------------------------------------
// build_history_params (private helper)
// ---------------------------------------------------------------------------

fn build_history_params(
    limit: u32,
    status: Option<&str>,
    action: Option<&str>,
    since_hours: Option<u32>,
) -> Value {
    let mut params = json!({ "limit": limit });
    if let Some(s) = status {
        params["status_filter"] = json!(s);
    }
    if let Some(a) = action {
        params["action_filter"] = json!(a);
    }
    if let Some(h) = since_hours {
        params["since_hours"] = json!(h);
    }
    params
}

// ---------------------------------------------------------------------------
// Logger
// ---------------------------------------------------------------------------

/// Tees all output to stdout and optionally to a log file.
///
/// `Mutex` makes `Logger` `Send + Sync` so it can be shared across the async
/// executor boundary without requiring a separate Arc.
pub struct Logger {
    file: std::sync::Mutex<Option<std::fs::File>>,
}

impl Logger {
    /// Construct.  Pass `None` to disable file tee.
    pub fn new(path: Option<&std::path::Path>) -> Result<Self, CliError> {
        let file = match path {
            None => None,
            Some(p) => Some(
                std::fs::OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(p)
                    .map_err(|e| CliError::ConfigOrDaemon(format!("open log file: {e}")))?,
            ),
        };
        Ok(Self { file: std::sync::Mutex::new(file) })
    }

    /// Print `line` to stdout and, if a log file is configured, also append it
    /// to that file.
    pub fn println(&self, line: &str) {
        println!("{line}");
        let mut guard = self.file.lock().expect("Logger mutex poisoned");
        if let Some(f) = guard.as_mut() {
            let _ = writeln!(f, "{line}");
        }
    }

    /// Print `line` to stderr only (not teed — errors belong on stderr).
    pub fn eprintln_err(&self, line: &str) {
        eprintln!("{line}");
    }
}

// ---------------------------------------------------------------------------
// run_completions
// ---------------------------------------------------------------------------

/// Write a shell completion script for `shell` to stdout.
pub fn run_completions(shell: clap_complete::Shell) {
    clap_complete::generate(shell, &mut Cli::command(), "lacs", &mut io::stdout());
}

// ---------------------------------------------------------------------------
// run_doctor
// ---------------------------------------------------------------------------

/// Check daemon connectivity and print configuration summary.
pub async fn run_doctor(
    socket: PathBuf,
    json_out: bool,
    log: &Logger,
) -> Result<(), CliError> {
    let config = BrainConfig::from_env()
        .map_err(|e| CliError::ConfigOrDaemon(e.to_string()))?;

    let client = DaemonClient::new(socket.clone());

    // `curated_state` is a blocking sync call; use spawn_blocking so the
    // multi-threaded runtime is not blocked on one thread indefinitely.
    let state_result = tokio::task::spawn_blocking(move || client.curated_state())
        .await
        .map_err(|e| CliError::ConfigOrDaemon(format!("join: {e}")))?;

    match state_result {
        Ok(state) => {
            if json_out {
                let out = json!({
                    "ok": true,
                    "socket": socket.display().to_string(),
                    "host": state.host_name(),
                    "provider": config.provider_name(),
                    "model": config.model_name(),
                });
                log.println(&serde_json::to_string(&out).expect("static JSON"));
            } else {
                log.println("daemon ok");
                log.println(&format!("  socket   {}", socket.display()));
                log.println(&format!("  host     {}", state.host_name()));
                log.println(&format!("  provider {}", config.provider_name()));
                log.println(&format!("  model    {}", config.model_name()));
            }
            Ok(())
        }
        Err(e) => {
            if json_out {
                let out = json!({ "ok": false, "error": e.to_string() });
                log.println(&serde_json::to_string(&out).expect("static JSON"));
            }
            Err(CliError::ConfigOrDaemon(e.to_string()))
        }
    }
}

// ---------------------------------------------------------------------------
// run_history
// ---------------------------------------------------------------------------

/// Query past LACS execution history via `ListJobHistory`.
pub async fn run_history(
    args: HistoryArgs,
    socket: PathBuf,
    global_json: bool,
    log: &Logger,
) -> Result<(), CliError> {
    let since_hours = match args.since.as_deref() {
        None => None,
        Some(s) => match since_to_hours(s) {
            Some(h) => Some(h),
            None => {
                return Err(CliError::ConfigOrDaemon(format!(
                    "--since: cannot parse or is a future datetime: {s:?}"
                )));
            }
        },
    };

    let params =
        build_history_params(args.limit, args.status.as_deref(), args.action.as_deref(), since_hours);

    let client = DaemonClient::new(socket);
    let output = tokio::task::spawn_blocking(move || client.query_action("ListJobHistory", &params))
        .await
        .map_err(|e| CliError::ConfigOrDaemon(format!("join: {e}")))?
        .map_err(|e| CliError::ConfigOrDaemon(e.to_string()))?;

    // The daemon formats history output; emit as-is.  The --json flag on either
    // the global Cli or HistoryArgs does not change what is printed here because
    // the formatting is the daemon's responsibility.
    let _ = global_json; // accepted to silence unused-variable lint
    let _ = args.json;
    log.println(&output);
    Ok(())
}

// ---------------------------------------------------------------------------
// RunOpts
// ---------------------------------------------------------------------------

/// Options derived from global CLI flags; threaded into `run_intent` and
/// `run_repl` so callers do not have to pass each flag individually.
pub struct RunOpts {
    pub socket: PathBuf,
    pub yes: bool,
    pub max_risk: Option<MaxRisk>,
    pub non_interactive: bool,
    pub dry_run: bool,
    pub json: bool,
    pub step_by_step: bool,
}

// ---------------------------------------------------------------------------
// run_intent
// ---------------------------------------------------------------------------

/// Plan and (optionally) execute a single natural-language intent.
pub async fn run_intent(intent: String, opts: &RunOpts, log: &Logger) -> Result<(), CliError> {
    let config = BrainConfig::from_env()
        .map_err(|e| CliError::ConfigOrDaemon(e.to_string()))?;

    let plan_client = DaemonClient::new(opts.socket.clone());
    let planner = LlmPlanner::from_config(config, Box::new(plan_client))
        .map_err(CliError::ConfigOrDaemon)?
        .with_prefs_path(prefs_path());

    // `plan_intent` is async and calls `StateClient::curated_state()` (a blocking
    // sync call) inside the future.  The multi-threaded runtime allows this without
    // stalling the executor, so we await directly.
    let plan = planner
        .plan_intent(&intent)
        .await
        .map_err(|e| CliError::PlanningFailed(e.to_string()))?;

    // ---- print plan --------------------------------------------------------

    if opts.json {
        let steps: Vec<Value> = plan
            .steps()
            .iter()
            .map(|s| {
                json!({
                    "action": s.action_name(),
                    "summary": s.summary(),
                    "risk": s.risk_level().as_str(),
                })
            })
            .collect();
        log.println(
            &serde_json::to_string(&json!({
                "plan": { "intent": plan.intent(), "summary": plan.summary(), "steps": steps }
            }))
            .expect("static JSON"),
        );
    } else {
        log.println(&format!("Plan: {}", plan.summary()));
        for (i, step) in plan.steps().iter().enumerate() {
            log.println(&format!(
                "  {}. [{}] {} — {}",
                i + 1,
                step.risk_level().as_str(),
                step.action_name(),
                step.summary(),
            ));
        }
    }

    if opts.dry_run {
        return Ok(());
    }

    // ---- plan-level approval (non-step-by-step) ----------------------------

    let policy = ApprovalPolicy::new(opts.yes, opts.max_risk, opts.non_interactive, opts.dry_run);

    if !opts.step_by_step {
        match policy.decide_plan(&plan) {
            ApprovalDecision::AutoApproved => {}
            ApprovalDecision::RequiresPrompt => {
                if !prompt_confirm("Execute this plan?") {
                    return Err(CliError::Rejected);
                }
            }
            ApprovalDecision::RequiresInteraction => return Err(CliError::NonInteractive),
            ApprovalDecision::ExceedsCeiling => {
                let highest = highest_risk(&plan).expect("ExceedsCeiling implies at least one step");
                return Err(CliError::RiskCeilingExceeded {
                    highest: highest.clone(),
                    ceiling: opts.max_risk.expect("ExceedsCeiling implies --max-risk was set"),
                });
            }
        }
    }

    // ---- execute steps -----------------------------------------------------

    let exec_client = DaemonClient::new(opts.socket.clone());

    for step in plan.steps() {
        // Step-by-step: approve each step before previewing it.
        if opts.step_by_step {
            match policy.decide_step(step.risk_level()) {
                ApprovalDecision::AutoApproved => {}
                ApprovalDecision::RequiresPrompt => {
                    if !prompt_confirm(&format!(
                        "Execute {} ({})? [y/N]",
                        step.action_name(),
                        step.summary()
                    )) {
                        return Err(CliError::Rejected);
                    }
                }
                ApprovalDecision::RequiresInteraction => return Err(CliError::NonInteractive),
                ApprovalDecision::ExceedsCeiling => {
                    return Err(CliError::RiskCeilingExceeded {
                        highest: step.risk_level().clone(),
                        ceiling: opts.max_risk.expect("ExceedsCeiling implies --max-risk was set"),
                    });
                }
            }
        }

        // Preview the step.
        let (preview, _tx_id) = exec_client.preview(step.action_name(), step.params()).await?;
        print_preview(&preview, opts.json, log);

        // Execute the step.
        exec_client
            .execute(step.action_name(), step.params(), &preview.request_hash, |line| {
                log.println(line);
            })
            .await
            .map(|result| print_result(&result, opts.json, log))?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// run_repl
// ---------------------------------------------------------------------------

/// Interactive REPL — reads intents from stdin until EOF or "exit" / "quit".
///
/// Uses `tokio::io` for non-blocking reads so the async executor is not
/// occupied by a blocking `stdin.read_line` call.
pub async fn run_repl(opts: &RunOpts, log: &Logger) -> Result<(), CliError> {
    use tokio::io::AsyncBufReadExt as _;

    let stdin = tokio::io::stdin();
    let mut reader = tokio::io::BufReader::new(stdin);
    let mut line_buf = String::new();

    loop {
        // Prompt on stderr so it does not pollute piped stdout.
        eprint!("> ");
        let _ = io::stderr().flush();

        line_buf.clear();
        match reader.read_line(&mut line_buf).await {
            Ok(0) => break, // EOF
            Ok(_) => {
                let intent = line_buf.trim().to_string();
                if intent.is_empty() {
                    continue;
                }
                if intent == "exit" || intent == "quit" {
                    break;
                }
                if let Err(e) = run_intent(intent, opts, log).await {
                    log.eprintln_err(&format!("error: {e}"));
                }
            }
            Err(e) => {
                log.eprintln_err(&format!("stdin read error: {e}"));
                break;
            }
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Path to the user preferences file (`~/.config/lacs/prefs.md`).
fn prefs_path() -> PathBuf {
    let mut base = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/root"));
    base.push(".config/lacs/prefs.md");
    base
}

/// Ask the user a yes/no question on stderr; return `true` iff they answer "y"
/// or "yes" (case-insensitive).
fn prompt_confirm(msg: &str) -> bool {
    eprint!("{msg} [y/N] ");
    let _ = io::stderr().flush();
    let mut buf = String::new();
    io::stdin().read_line(&mut buf).unwrap_or(0);
    matches!(buf.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

fn print_preview(env: &PreviewEnvelope, json_out: bool, log: &Logger) {
    if json_out {
        log.println(&serde_json::to_string(env).expect("PreviewEnvelope is Serialize"));
    } else {
        let risk = format!("{:?}", env.risk_level).to_lowercase();
        log.println(&format!("  preview  {}", env.summary));
        log.println(&format!("  risk     {risk}"));
        if env.reboot_required {
            log.println("  ! reboot required after this step");
        }
        for w in &env.warnings {
            log.println(&format!("  ! {w}"));
        }
    }
}

fn print_result(env: &ResultEnvelope, json_out: bool, log: &Logger) {
    if json_out {
        log.println(&serde_json::to_string(env).expect("ResultEnvelope is Serialize"));
    } else {
        let status = format!("{:?}", env.status).to_lowercase();
        log.println(&format!("  result   {status}"));
        log.println(&format!("  {}", env.summary));
        if env.needs_reboot {
            log.println("  ! reboot required");
        }
        if let Some(ref id) = env.job_id {
            log.println(&format!("  job      {id}"));
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use lacs_brain::action_name::ActionName;
    use lacs_brain::planner::{Plan, PlanStep};

    // -----------------------------------------------------------------------
    // rfc3339_to_unix — pure function, tests against known epoch values
    // -----------------------------------------------------------------------

    #[test]
    fn rfc3339_unix_epoch_z() {
        assert_eq!(rfc3339_to_unix("1970-01-01T00:00:00Z"), Some(0));
    }

    #[test]
    fn rfc3339_unix_epoch_plus00() {
        assert_eq!(rfc3339_to_unix("1970-01-01T00:00:00+00:00"), Some(0));
    }

    #[test]
    fn rfc3339_unix_one_day() {
        assert_eq!(rfc3339_to_unix("1970-01-02T00:00:00Z"), Some(86_400));
    }

    #[test]
    fn rfc3339_unix_y2k() {
        // 2000-01-01T00:00:00Z = 946684800
        assert_eq!(rfc3339_to_unix("2000-01-01T00:00:00Z"), Some(946_684_800));
    }

    #[test]
    fn rfc3339_unix_leap_day_2000() {
        // 2000-02-29: Jan has 31 days, then 28 more days = 59 days from 2000-01-01.
        // 946684800 + 59 * 86400 = 946684800 + 5097600 = 951782400
        assert_eq!(rfc3339_to_unix("2000-02-29T00:00:00Z"), Some(951_782_400));
    }

    #[test]
    fn rfc3339_unix_with_subseconds() {
        // Sub-second fraction should be stripped.
        assert_eq!(
            rfc3339_to_unix("2000-01-01T00:00:00.123456Z"),
            Some(946_684_800)
        );
    }

    #[test]
    fn rfc3339_unix_non_utc_returns_none() {
        assert!(rfc3339_to_unix("2000-01-01T00:00:00+05:00").is_none());
    }

    #[test]
    fn rfc3339_unix_no_suffix_returns_none() {
        assert!(rfc3339_to_unix("2000-01-01T00:00:00").is_none());
    }

    #[test]
    fn rfc3339_unix_garbage_returns_none() {
        assert!(rfc3339_to_unix("not-a-date").is_none());
        assert!(rfc3339_to_unix("").is_none());
    }

    #[test]
    fn rfc3339_unix_invalid_month_returns_none() {
        assert!(rfc3339_to_unix("2000-13-01T00:00:00Z").is_none());
    }

    #[test]
    fn rfc3339_unix_invalid_hour_returns_none() {
        assert!(rfc3339_to_unix("2000-01-01T25:00:00Z").is_none());
    }

    // -----------------------------------------------------------------------
    // since_to_hours
    // -----------------------------------------------------------------------

    #[test]
    fn since_to_hours_y2k_is_many_hours_ago() {
        // Y2K was well over 200_000 hours ago (as of 2026).
        let h = since_to_hours("2000-01-01T00:00:00Z").expect("should parse");
        assert!(h > 200_000, "expected >200000 hours, got {h}");
    }

    #[test]
    fn since_to_hours_far_future_returns_none() {
        // Year 9999 is in the future.
        assert!(since_to_hours("9999-12-31T23:59:59Z").is_none());
    }

    #[test]
    fn since_to_hours_garbage_returns_none() {
        assert!(since_to_hours("not-a-date").is_none());
    }

    // -----------------------------------------------------------------------
    // highest_risk
    // -----------------------------------------------------------------------

    fn make_step(risk: PlanRiskLevel) -> PlanStep {
        PlanStep::new(
            ActionName::parse("GetDiskUsage").unwrap(),
            "test".into(),
            risk,
            serde_json::json!({}),
        )
        .unwrap()
    }

    fn make_plan(risks: &[PlanRiskLevel]) -> Plan {
        Plan::new(
            "test".into(),
            "test plan".into(),
            "explanation".into(),
            risks.iter().map(|r| make_step(r.clone())).collect(),
        )
        .unwrap()
    }

    // Note: Plan::new rejects empty step lists (PlanValidationError), so
    // `highest_risk` is never called on an empty plan in practice.  The return
    // type is `Option<_>` purely for type-safety against future API changes.

    #[test]
    fn highest_risk_single_low() {
        let plan = make_plan(&[PlanRiskLevel::Low]);
        assert_eq!(highest_risk(&plan), Some(&PlanRiskLevel::Low));
    }

    #[test]
    fn highest_risk_all_high() {
        let plan = make_plan(&[PlanRiskLevel::High, PlanRiskLevel::High]);
        assert_eq!(highest_risk(&plan), Some(&PlanRiskLevel::High));
    }

    #[test]
    fn highest_risk_mixed_picks_highest() {
        let plan = make_plan(&[PlanRiskLevel::Low, PlanRiskLevel::High, PlanRiskLevel::Medium]);
        assert_eq!(highest_risk(&plan), Some(&PlanRiskLevel::High));
    }

    #[test]
    fn highest_risk_low_medium_picks_medium() {
        let plan = make_plan(&[PlanRiskLevel::Low, PlanRiskLevel::Medium]);
        assert_eq!(highest_risk(&plan), Some(&PlanRiskLevel::Medium));
    }

    // -----------------------------------------------------------------------
    // build_history_params
    // -----------------------------------------------------------------------

    #[test]
    fn build_history_params_minimal() {
        let p = build_history_params(20, None, None, None);
        assert_eq!(p["limit"], json!(20));
        assert!(p.get("status_filter").is_none());
        assert!(p.get("action_filter").is_none());
        assert!(p.get("since_hours").is_none());
    }

    #[test]
    fn build_history_params_all_fields() {
        let p = build_history_params(5, Some("succeeded"), Some("InstallPackages"), Some(48));
        assert_eq!(p["limit"], json!(5));
        assert_eq!(p["status_filter"], json!("succeeded"));
        assert_eq!(p["action_filter"], json!("InstallPackages"));
        assert_eq!(p["since_hours"], json!(48));
    }

    #[test]
    fn build_history_params_status_only() {
        let p = build_history_params(10, Some("failed"), None, None);
        assert_eq!(p["limit"], json!(10));
        assert_eq!(p["status_filter"], json!("failed"));
        assert!(p.get("action_filter").is_none());
        assert!(p.get("since_hours").is_none());
    }

    // -----------------------------------------------------------------------
    // resolve_socket uses LACS_SOCKET env var
    // -----------------------------------------------------------------------

    #[test]
    fn resolve_socket_uses_env_var() {
        // Safety: env mutation is process-wide; keep scope minimal.
        // SAFETY: single-threaded test runner is assumed for env mutation.
        unsafe { std::env::set_var("LACS_SOCKET", "/tmp/test.sock") };
        let p = resolve_socket();
        unsafe { std::env::remove_var("LACS_SOCKET") };
        assert_eq!(p, PathBuf::from("/tmp/test.sock"));
    }

    #[test]
    fn resolve_socket_default_without_env_var() {
        unsafe { std::env::remove_var("LACS_SOCKET") };
        let p = resolve_socket();
        assert_eq!(p, PathBuf::from("/run/lacs/lacs.sock"));
    }
}

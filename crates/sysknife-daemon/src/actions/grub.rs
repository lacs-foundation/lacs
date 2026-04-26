//! GRUB kernel argument actions (Ubuntu).
//!
//! ## GrubGetKargs
//!
//! Read-only inspection of `GRUB_CMDLINE_LINUX` in `/etc/default/grub`.
//! No system changes are made.
//!
//! ## GrubSetKargs
//!
//! Modifies `GRUB_CMDLINE_LINUX_DEFAULT` in `/etc/default/grub`, then runs
//! `sudo update-grub` to regenerate the GRUB config.
//!
//! **Backup:** before the edit, the current `/etc/default/grub` is copied to
//! `/etc/default/grub.sysknife.bak`.  On `update-grub` failure the backup is
//! restored via a shell `||` expression so the original file is never lost.
//!
//! **Shell pipeline:** the entire operation is expressed as a single `bash -c`
//! fragment so that the backup, edit, and grub-update are atomic from the
//! executor's perspective (one `ActionSpec` / one process).
//!
//! **Reboot required:** kernel argument changes do not take effect until the
//! next boot.
//!
//! **Params:**
//! - `append`: `Vec<String>` — args to add (merged into the existing line).
//! - `delete`: `Vec<String>` — args to remove from the existing line.
//!   Either list may be empty; at least one must be non-empty.

use super::{command_mechanism, ActionSpec};
use sysknife_types::RiskLevel;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// GRUB configuration file path.
const GRUB_DEFAULT: &str = "/etc/default/grub";

/// Backup path written before any modification.
const GRUB_BACKUP: &str = "/etc/default/grub.sysknife.bak";

// ---------------------------------------------------------------------------
// specs() — for action_consistency tests
// ---------------------------------------------------------------------------

/// Return one representative `ActionSpec` per GRUB action name.
pub fn specs() -> Vec<ActionSpec> {
    vec![
        grub_get_kargs(),
        grub_set_kargs(&["quiet"], &["splash"]).expect("non-empty kargs"),
    ]
}

// ---------------------------------------------------------------------------
// Action constructors
// ---------------------------------------------------------------------------

/// Read the `GRUB_CMDLINE_LINUX` line from `/etc/default/grub`.
///
/// Risk: Low. Read-only file inspection; no system changes.
pub fn grub_get_kargs() -> ActionSpec {
    ActionSpec {
        action_name: "GrubGetKargs",
        mechanism: command_mechanism("grep", ["-E", r"^GRUB_CMDLINE_LINUX", GRUB_DEFAULT]),
        risk_level: RiskLevel::Low,
        reboot_required: false,
        rollback_available: false,
    }
}

/// Modify kernel arguments in `GRUB_CMDLINE_LINUX_DEFAULT`, back up the
/// original file, then run `update-grub`.
///
/// Risk: High. Kernel argument changes affect every boot. Incorrect arguments
/// can prevent the system from booting. A backup is written to
/// `/etc/default/grub.sysknife.bak` and restored on `update-grub` failure.
///
/// `append` — arguments to add (shell-safe strings, validated before call).
/// `delete` — arguments to remove (shell-safe strings, validated before call).
///
/// At least one of `append` / `delete` MUST be non-empty — calling with both
/// empty is a no-op that still rewrites the GRUB config and runs `update-grub`,
/// which is wasteful and misleading. The constructor returns
/// `Err(KargsError::BothEmpty)` in that case.
///
/// The implementation uses a Python one-liner to perform the in-line edit so
/// the operation is portable across `sed` variants and handles quoting
/// correctly.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum KargsError {
    #[error("at least one of append or delete must be non-empty")]
    BothEmpty,
}

pub fn grub_set_kargs(append: &[&str], delete: &[&str]) -> Result<ActionSpec, KargsError> {
    if append.is_empty() && delete.is_empty() {
        return Err(KargsError::BothEmpty);
    }
    let append_str = append.join(" ");
    let delete_args: Vec<String> = delete
        .iter()
        .map(|d| {
            format!(
                r#"line = re.sub(r'(?<!\S){re}(?!\S)\s*', '', line)"#,
                re = regex_escape(d)
            )
        })
        .collect();
    let delete_block = delete_args.join("; ");

    // Python script that:
    // 1. Reads /etc/default/grub
    // 2. Finds GRUB_CMDLINE_LINUX_DEFAULT="..."
    // 3. Removes requested args from the value
    // 4. Appends new args
    // 5. Writes the file back in-place
    let python_script = format!(
        r#"import re, sys
with open('{grub}', 'r') as f: lines = f.readlines()
out = []
for line in lines:
    if line.startswith('GRUB_CMDLINE_LINUX_DEFAULT='):
        m = re.match(r'^(GRUB_CMDLINE_LINUX_DEFAULT=")([^"]*)(".*)', line)
        if m:
            line = m.group(2).strip()
            {delete_block}
            line = (line + ' {append_str}').strip()
            line = 'GRUB_CMDLINE_LINUX_DEFAULT="' + line + '"\n'
        out.append(line)
    else:
        out.append(line)
with open('{grub}', 'w') as f: f.writelines(out)
"#,
        grub = GRUB_DEFAULT,
        delete_block = if delete_block.is_empty() {
            "pass".to_string()
        } else {
            delete_block
        },
        append_str = append_str,
    );

    // Full shell command:
    // 1. Back up the original file.
    // 2. Run the Python edit.
    // 3. Run update-grub; on failure, restore backup.
    let shell_cmd = format!(
        "sudo cp {backup} {grub}.tmp 2>/dev/null || true && \
         sudo cp {grub} {backup} && \
         sudo python3 -c {script:?} && \
         sudo update-grub || (sudo cp {backup} {grub} && exit 1)",
        grub = GRUB_DEFAULT,
        backup = GRUB_BACKUP,
        script = python_script,
    );

    Ok(ActionSpec {
        action_name: "GrubSetKargs",
        mechanism: command_mechanism("bash", ["-c", &shell_cmd]),
        risk_level: RiskLevel::High,
        reboot_required: true,
        rollback_available: true,
    })
}

/// Minimal regex escaping for literal argument strings.
///
/// Only escapes characters that are special in Python `re` patterns and that
/// could plausibly appear in kernel argument values.  Full escaping is not
/// needed because `validated_safe_arg` in the executor already rejects
/// shell-special characters.
fn regex_escape(s: &str) -> String {
    s.chars()
        .flat_map(|c| match c {
            '.' | '^' | '$' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '\\' => {
                vec!['\\', c]
            }
            other => vec![other],
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::ActionMechanism;

    fn extract_cmd(spec: &ActionSpec) -> (&'static str, Vec<String>) {
        match &spec.mechanism {
            ActionMechanism::Command { program, args } => (*program, args.clone()),
            _ => panic!("expected Command mechanism"),
        }
    }

    // ── grub_get_kargs ────────────────────────────────────────────────────────

    #[test]
    fn grub_get_kargs_action_name() {
        assert_eq!(grub_get_kargs().action_name, "GrubGetKargs");
    }

    #[test]
    fn grub_get_kargs_uses_grep_on_grub_default() {
        let spec = grub_get_kargs();
        let (prog, args) = extract_cmd(&spec);
        assert_eq!(prog, "grep");
        let joined = args.join(" ");
        assert!(joined.contains(GRUB_DEFAULT), "missing grub path: {joined}");
        assert!(
            joined.contains("GRUB_CMDLINE_LINUX"),
            "missing pattern: {joined}"
        );
    }

    #[test]
    fn grub_get_kargs_risk_is_low() {
        assert_eq!(grub_get_kargs().risk_level, RiskLevel::Low);
    }

    #[test]
    fn grub_get_kargs_no_reboot_no_rollback() {
        let spec = grub_get_kargs();
        assert!(!spec.reboot_required);
        assert!(!spec.rollback_available);
    }

    // ── grub_set_kargs ────────────────────────────────────────────────────────

    #[test]
    fn grub_set_kargs_action_name() {
        assert_eq!(
            grub_set_kargs(&["quiet"], &[]).unwrap().action_name,
            "GrubSetKargs"
        );
    }

    #[test]
    fn grub_set_kargs_uses_bash() {
        let spec = grub_set_kargs(&["quiet"], &["splash"]).unwrap();
        let (prog, _) = extract_cmd(&spec);
        assert_eq!(prog, "bash");
    }

    #[test]
    fn grub_set_kargs_shell_fragment_contains_backup_path() {
        let spec = grub_set_kargs(&["quiet"], &[]).unwrap();
        let (_, args) = extract_cmd(&spec);
        let joined = args.join(" ");
        assert!(
            joined.contains(GRUB_BACKUP),
            "missing backup path: {joined}"
        );
    }

    #[test]
    fn grub_set_kargs_shell_fragment_contains_update_grub() {
        let spec = grub_set_kargs(&["quiet"], &[]).unwrap();
        let (_, args) = extract_cmd(&spec);
        let joined = args.join(" ");
        assert!(
            joined.contains("update-grub"),
            "missing update-grub: {joined}"
        );
    }

    #[test]
    fn grub_set_kargs_append_args_appear_in_script() {
        let spec = grub_set_kargs(&["nomodeset", "quiet"], &[]).unwrap();
        let (_, args) = extract_cmd(&spec);
        let joined = args.join(" ");
        assert!(joined.contains("nomodeset"), "missing nomodeset: {joined}");
        assert!(joined.contains("quiet"), "missing quiet: {joined}");
    }

    #[test]
    fn grub_set_kargs_delete_args_appear_in_script() {
        let spec = grub_set_kargs(&[], &["splash"]).unwrap();
        let (_, args) = extract_cmd(&spec);
        let joined = args.join(" ");
        assert!(joined.contains("splash"), "missing splash: {joined}");
    }

    #[test]
    fn grub_set_kargs_risk_is_high() {
        assert_eq!(
            grub_set_kargs(&["quiet"], &[]).unwrap().risk_level,
            RiskLevel::High
        );
    }

    #[test]
    fn grub_set_kargs_reboot_required() {
        assert!(grub_set_kargs(&["quiet"], &[]).unwrap().reboot_required);
    }

    #[test]
    fn grub_set_kargs_rollback_available() {
        assert!(grub_set_kargs(&["quiet"], &[]).unwrap().rollback_available);
    }

    #[test]
    fn grub_set_kargs_rejects_both_empty() {
        // The constructor — not just the executor — enforces "at least one of
        // append/delete must be non-empty". A direct Rust caller can't bypass.
        let err = grub_set_kargs(&[], &[]).unwrap_err();
        assert_eq!(err, KargsError::BothEmpty);
    }

    // ── regex_escape ──────────────────────────────────────────────────────────

    #[test]
    fn regex_escape_plain_string_unchanged() {
        assert_eq!(regex_escape("quiet"), "quiet");
    }

    #[test]
    fn regex_escape_dot_is_escaped() {
        assert_eq!(regex_escape("ro.dm"), r"ro\.dm");
    }

    // ── specs() completeness ─────────────────────────────────────────────────

    #[test]
    fn specs_covers_all_action_names() {
        let expected = ["GrubGetKargs", "GrubSetKargs"];
        let spec_names: Vec<&str> = specs().iter().map(|s| s.action_name).collect();
        for name in &expected {
            assert!(spec_names.contains(name), "specs() missing {name}");
        }
    }
}

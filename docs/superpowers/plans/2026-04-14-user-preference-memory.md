# User Preference Memory Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let LACS remember user preferences across sessions via a `~/.config/lacs/prefs.md` file that is injected into the planning prompt, with `remember` and `forget` planning tools.

**Architecture:** A flat markdown file of user preferences (`prefs.md`) stored alongside `config.toml`. The planner reads it at the start of each `plan_intent()` call and appends its contents to the system prompt. Two new planning tools (`remember`, `forget`) let the LLM add or remove entries during planning. All file writes use atomic temp-file-then-rename. A sensitive-data filter rejects secrets. A 10 KB size limit prevents runaway growth.

**Tech Stack:** Rust (lacs-brain, lacs-core), `tempfile` (moved from dev-only to runtime dep in lacs-brain), serde_json, existing `ToolDefinition` type.

**Branch:** `feat/user-preference-memory` in a dedicated worktree.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `crates/lacs-brain/src/prefs.rs` | **Create** | Read/write `prefs.md`, atomic writes, size limit, sensitive-data filter |
| `crates/lacs-brain/src/planning_tools/preferences.rs` | **Create** | `remember_tool_def()`, `forget_tool_def()`, parse tool input |
| `crates/lacs-brain/src/planning_tools/mod.rs` | Modify | Add `pub(crate) mod preferences;` |
| `crates/lacs-brain/src/planner.rs` | Modify | Register tools, handle `remember`/`forget` in planning loop, inject prefs into prompt |
| `crates/lacs-brain/src/prompt.rs` | Modify | Accept optional prefs string, add `## Your preferences` section + `remember`/`forget` docs |
| `crates/lacs-brain/src/lib.rs` | Modify | Add `pub mod prefs;` |
| `crates/lacs-brain/Cargo.toml` | Modify | Move `tempfile` from `[dev-dependencies]` to `[dependencies]` |
| `crates/lacs-brain/tests/planner.rs` | Modify | Add integration tests for `remember`/`forget` in planning loop |
| `crates/lacs-core/src/config.rs` | Modify | Add `pub fn prefs_path() -> PathBuf` helper |

---

### Task 1: `lacs-core` — `prefs_path()` helper

**Files:**
- Modify: `crates/lacs-core/src/config.rs`

- [ ] **Step 1: Write the failing test**

In `crates/lacs-core/src/config.rs`, add to the `mod tests` block:

```rust
#[test]
fn prefs_path_lives_alongside_config() {
    // prefs_path() must return ~/.config/lacs/prefs.md (same dir as config.toml)
    let prefs = prefs_path();
    let config = config_path();
    assert_eq!(prefs.parent(), config.parent());
    assert_eq!(prefs.file_name().unwrap(), "prefs.md");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p lacs-core -- prefs_path_lives_alongside_config`
Expected: FAIL with "cannot find function `prefs_path`"

- [ ] **Step 3: Write minimal implementation**

In `crates/lacs-core/src/config.rs`, after the existing `pub fn config_path() -> PathBuf` function (line 176), add:

```rust
/// Returns the path to `~/.config/lacs/prefs.md`, respecting
/// `XDG_CONFIG_HOME` if set. Same directory as `config.toml`.
pub fn prefs_path() -> PathBuf {
    let config_dir = std::env::var("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
            PathBuf::from(home).join(".config")
        });
    config_dir.join("lacs").join("prefs.md")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p lacs-core -- prefs_path_lives_alongside_config`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/lacs-core/src/config.rs
git commit -m "feat(core): add prefs_path() helper for user preference file location"
```

---

### Task 2: `lacs-brain` — `prefs.rs` core file operations

**Files:**
- Create: `crates/lacs-brain/src/prefs.rs`
- Modify: `crates/lacs-brain/src/lib.rs`
- Modify: `crates/lacs-brain/Cargo.toml`

- [ ] **Step 1: Add `tempfile` as a runtime dependency**

In `crates/lacs-brain/Cargo.toml`, add to the `[dependencies]` section (NOT dev-dependencies):

```toml
tempfile = "3"
```

- [ ] **Step 2: Write the failing tests**

Create `crates/lacs-brain/src/prefs.rs` with tests only:

```rust
//! User preference file operations.
//!
//! Preferences are stored as a flat markdown file at `~/.config/lacs/prefs.md`.
//! Each preference is a single line prefixed with `- `. The file is read by the
//! planner at the start of each `plan_intent()` call and injected into the
//! system prompt.

use std::io;
use std::path::Path;

/// Maximum size of the preferences file in bytes. Prevents runaway growth
/// from a misbehaving LLM that calls `remember` in a loop. 10 KB is roughly
/// 200 preferences — well beyond any practical use.
pub const PREFS_MAX_BYTES: u64 = 10_240;

/// Substrings that indicate sensitive data. If any of these appear
/// (case-insensitive) in a preference, it is rejected.
const SENSITIVE_PATTERNS: &[&str] = &[
    "password",
    "passwd",
    "secret",
    "api_key",
    "apikey",
    "private_key",
    "token",
    "credential",
    "-----begin",
];

/// Key prefixes that indicate API keys or tokens.
const SENSITIVE_PREFIXES: &[&str] = &[
    "sk-",         // Anthropic / OpenAI
    "ghp_",        // GitHub personal access token
    "github_pat_", // GitHub fine-grained PAT
    "gho_",        // GitHub OAuth token
    "xoxb-",       // Slack bot token
    "xoxp-",       // Slack user token
];

pub fn read_prefs(path: &Path) -> Option<String> {
    todo!()
}

pub fn append_pref(path: &Path, fact: &str) -> Result<(), io::Error> {
    todo!()
}

pub fn remove_pref(path: &Path, fact: &str) -> Result<bool, io::Error> {
    todo!()
}

pub fn contains_sensitive(fact: &str) -> bool {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn read_prefs_returns_none_when_file_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        assert!(read_prefs(&path).is_none());
    }

    #[test]
    fn read_prefs_returns_none_when_file_empty() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        std::fs::write(&path, "").unwrap();
        assert!(read_prefs(&path).is_none());
    }

    #[test]
    fn append_pref_creates_file_and_writes_entry() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "prefer vim-enhanced over vim").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "- prefer vim-enhanced over vim\n");
    }

    #[test]
    fn append_pref_appends_to_existing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "first preference").unwrap();
        append_pref(&path, "second preference").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "- first preference\n- second preference\n");
    }

    #[test]
    fn append_pref_rejects_when_file_exceeds_size_limit() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        // Write a file that is just under the limit.
        let big_content = "- ".to_string() + &"x".repeat(PREFS_MAX_BYTES as usize - 3) + "\n";
        std::fs::write(&path, &big_content).unwrap();
        let result = append_pref(&path, "one more");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("size limit"));
    }

    #[test]
    fn append_pref_deduplicates() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "prefer vim-enhanced").unwrap();
        append_pref(&path, "prefer vim-enhanced").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content.matches("vim-enhanced").count(), 1);
    }

    #[test]
    fn remove_pref_removes_matching_line() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "first pref").unwrap();
        append_pref(&path, "second pref").unwrap();
        let removed = remove_pref(&path, "first pref").unwrap();
        assert!(removed);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("first pref"));
        assert!(content.contains("second pref"));
    }

    #[test]
    fn remove_pref_returns_false_when_not_found() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        append_pref(&path, "some pref").unwrap();
        let removed = remove_pref(&path, "nonexistent").unwrap();
        assert!(!removed);
    }

    #[test]
    fn remove_pref_returns_false_when_file_absent() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prefs.md");
        let removed = remove_pref(&path, "anything").unwrap();
        assert!(!removed);
    }

    #[test]
    fn contains_sensitive_detects_password() {
        assert!(contains_sensitive("my password is hunter2"));
        assert!(contains_sensitive("ANTHROPIC_API_KEY=sk-abc123"));
    }

    #[test]
    fn contains_sensitive_detects_key_prefixes() {
        assert!(contains_sensitive("use key sk-ant-abc123 for anthropic"));
        assert!(contains_sensitive("github token ghp_abcdef1234567890"));
    }

    #[test]
    fn contains_sensitive_allows_normal_preferences() {
        assert!(!contains_sensitive("prefer vim-enhanced over vim"));
        assert!(!contains_sensitive("always use flathub remote"));
        assert!(!contains_sensitive("skip large downloads on metered connections"));
    }
}
```

- [ ] **Step 3: Register the module in `lib.rs`**

In `crates/lacs-brain/src/lib.rs`, add:

```rust
pub mod prefs;
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p lacs-brain -- prefs::`
Expected: FAIL — 11 tests fail with "not yet implemented"

- [ ] **Step 5: Implement `contains_sensitive`**

Replace the `todo!()` in `contains_sensitive`:

```rust
pub fn contains_sensitive(fact: &str) -> bool {
    let lower = fact.to_lowercase();
    if SENSITIVE_PATTERNS.iter().any(|p| lower.contains(p)) {
        return true;
    }
    SENSITIVE_PREFIXES.iter().any(|p| fact.contains(p))
}
```

- [ ] **Step 6: Implement `read_prefs`**

Replace the `todo!()` in `read_prefs`:

```rust
pub fn read_prefs(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    if content.trim().is_empty() {
        return None;
    }
    Some(content)
}
```

- [ ] **Step 7: Implement `append_pref`**

Replace the `todo!()` in `append_pref`:

```rust
pub fn append_pref(path: &Path, fact: &str) -> Result<(), io::Error> {
    // Create parent directories if needed.
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Check size limit.
    if let Ok(meta) = std::fs::metadata(path) {
        if meta.len() >= PREFS_MAX_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "preferences file exceeds size limit ({} bytes); \
                     remove unused preferences before adding new ones",
                    PREFS_MAX_BYTES
                ),
            ));
        }
    }

    // Check for duplicates.
    if let Some(existing) = read_prefs(path) {
        if existing.lines().any(|line| {
            line.strip_prefix("- ")
                .is_some_and(|stripped| stripped == fact)
        }) {
            return Ok(()); // Already present, no-op.
        }
    }

    let new_line = format!("- {fact}\n");

    // Atomic write: read existing content, append, write to temp, rename.
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let combined = format!("{existing}{new_line}");

    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, combined.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
```

- [ ] **Step 8: Implement `remove_pref`**

Replace the `todo!()` in `remove_pref`:

```rust
pub fn remove_pref(path: &Path, fact: &str) -> Result<bool, io::Error> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };

    let target = format!("- {fact}");
    let mut found = false;
    let filtered: Vec<&str> = content
        .lines()
        .filter(|line| {
            if *line == target {
                found = true;
                false
            } else {
                true
            }
        })
        .collect();

    if !found {
        return Ok(false);
    }

    let new_content = if filtered.is_empty() {
        String::new()
    } else {
        filtered.join("\n") + "\n"
    };

    let dir = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, new_content.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(true)
}
```

- [ ] **Step 9: Run all tests to verify they pass**

Run: `cargo test -p lacs-brain -- prefs::`
Expected: 11 tests PASS

- [ ] **Step 10: Commit**

```bash
git add crates/lacs-brain/Cargo.toml crates/lacs-brain/src/prefs.rs crates/lacs-brain/src/lib.rs
git commit -m "feat(brain): add prefs.rs — user preference file operations with atomic writes"
```

---

### Task 3: `remember` and `forget` tool definitions

**Files:**
- Create: `crates/lacs-brain/src/planning_tools/preferences.rs`
- Modify: `crates/lacs-brain/src/planning_tools/mod.rs`

- [ ] **Step 1: Write the failing tests**

Create `crates/lacs-brain/src/planning_tools/preferences.rs`:

```rust
//! `remember` and `forget` planning tools.
//!
//! These tools let the LLM save or remove user preferences during planning.
//! They are brain-side-only — they write directly to `~/.config/lacs/prefs.md`
//! and never touch the daemon.

use crate::provider::ToolDefinition;

pub fn remember_tool_def() -> ToolDefinition {
    todo!()
}

pub fn forget_tool_def() -> ToolDefinition {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_tool_has_fact_param() {
        let def = remember_tool_def();
        assert_eq!(def.name, "remember");
        let props = def.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("fact"));
        let required = def.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "fact"));
    }

    #[test]
    fn forget_tool_has_fact_param() {
        let def = forget_tool_def();
        assert_eq!(def.name, "forget");
        let props = def.input_schema["properties"].as_object().unwrap();
        assert!(props.contains_key("fact"));
        let required = def.input_schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "fact"));
    }
}
```

- [ ] **Step 2: Register the module**

In `crates/lacs-brain/src/planning_tools/mod.rs`, add:

```rust
pub(crate) mod preferences;
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p lacs-brain -- planning_tools::preferences`
Expected: FAIL with "not yet implemented"

- [ ] **Step 4: Implement tool definitions**

Replace the `todo!()` stubs:

```rust
pub fn remember_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "remember".into(),
        description: "Save a user preference that should apply to all future planning sessions. \
                       Use this when the user explicitly asks you to remember something about how \
                       they want their system managed. Do NOT use this to store system facts — only \
                       user preferences and stated intentions."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "fact": {
                    "type": "string",
                    "description": "The preference to save, in plain language. Be specific and actionable. Example: 'Prefer vim-enhanced over vim for package layering requests'."
                }
            },
            "required": ["fact"]
        }),
    }
}

pub fn forget_tool_def() -> ToolDefinition {
    ToolDefinition {
        name: "forget".into(),
        description: "Remove a previously saved user preference. Use this when the user asks \
                       you to forget or stop applying a preference. The fact string must match \
                       an existing preference exactly."
            .into(),
        input_schema: serde_json::json!({
            "type": "object",
            "additionalProperties": false,
            "properties": {
                "fact": {
                    "type": "string",
                    "description": "The preference to remove. Must match an existing entry exactly."
                }
            },
            "required": ["fact"]
        }),
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p lacs-brain -- planning_tools::preferences`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add crates/lacs-brain/src/planning_tools/preferences.rs crates/lacs-brain/src/planning_tools/mod.rs
git commit -m "feat(brain): add remember and forget planning tool definitions"
```

---

### Task 4: System prompt — add preferences section and tool docs

**Files:**
- Modify: `crates/lacs-brain/src/prompt.rs`

- [ ] **Step 1: Write the failing test**

In the `mod tests` block of `crates/lacs-brain/src/prompt.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_prompt_without_prefs_does_not_contain_preferences_section() {
        let prompt = build_system_prompt(None);
        assert!(!prompt.contains("## Your saved preferences"));
    }

    #[test]
    fn system_prompt_with_prefs_contains_preferences_section() {
        let prefs = "- prefer vim-enhanced over vim\n- skip large downloads\n";
        let prompt = build_system_prompt(Some(prefs));
        assert!(prompt.contains("## Your saved preferences"));
        assert!(prompt.contains("prefer vim-enhanced over vim"));
        assert!(prompt.contains("skip large downloads"));
    }

    #[test]
    fn system_prompt_documents_remember_and_forget_tools() {
        let prompt = build_system_prompt(None);
        assert!(prompt.contains("`remember`"));
        assert!(prompt.contains("`forget`"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lacs-brain -- prompt::tests`
Expected: FAIL — `build_system_prompt` does not accept an argument

- [ ] **Step 3: Change `build_system_prompt` signature**

Change the function signature from:

```rust
pub fn build_system_prompt() -> String {
```

to:

```rust
pub fn build_system_prompt(user_prefs: Option<&str>) -> String {
```

At the end of the function, before `.to_string()`, append the preference-related sections. The full change at the end of the string literal, just before the closing `"#`:

```rust
    // After the existing prompt text, append preference tools and prefs:
    let mut prompt = r#"... existing prompt ..."#.to_string();

    // Append the remember/forget tool documentation.
    prompt.push_str(
        r#"

## Preference tools — `remember` and `forget`

Two additional tools let you manage user preferences:

- `remember(fact)` — save a user preference. Call this when the user explicitly
  asks "remember that I ...", "always do X", or "I prefer Y over Z". Only save
  user preferences, not system facts (those are queryable live).
- `forget(fact)` — remove a previously saved preference. The fact must match
  an existing entry exactly.

After calling `remember` or `forget`, you must still call `propose_plan` to
finish. If the user's only intent was to save/remove a preference, propose a
single `GetSystemState` low-risk step with a summary confirming the preference
change.
"#,
    );

    if let Some(prefs) = user_prefs {
        prompt.push_str(&format!(
            r#"
## Your saved preferences

These are preferences the user has explicitly asked you to remember.
Apply them when relevant — they reflect the user's stated intentions.

{prefs}"#
        ));
    }

    prompt
```

- [ ] **Step 4: Update all callers**

In `crates/lacs-brain/src/planner.rs`, update `LlmPlanner::new()` (line 354):

```rust
system_prompt: build_system_prompt(None),
```

This is a temporary fix — Task 5 will inject real prefs. The `None` maintains current behavior.

- [ ] **Step 5: Run all tests**

Run: `cargo test -p lacs-brain`
Expected: All tests PASS (existing tests still use `build_system_prompt(None)`)

- [ ] **Step 6: Commit**

```bash
git add crates/lacs-brain/src/prompt.rs crates/lacs-brain/src/planner.rs
git commit -m "feat(prompt): add preferences section and remember/forget tool docs to system prompt"
```

---

### Task 5: Planner integration — register tools, handle calls, inject prefs

**Files:**
- Modify: `crates/lacs-brain/src/planner.rs`
- Modify: `crates/lacs-brain/tests/planner.rs`

- [ ] **Step 1: Write the failing integration tests**

In `crates/lacs-brain/tests/planner.rs`, add:

```rust
// ---------------------------------------------------------------------------
// remember / forget tool calls
// ---------------------------------------------------------------------------

#[tokio::test]
async fn remember_tool_saves_preference_and_planner_continues() {
    // Turn 1: LLM calls remember("prefer vim-enhanced")
    // Turn 2: LLM calls propose_plan
    let dir = tempfile::tempdir().unwrap();
    let prefs_path = dir.path().join("prefs.md");

    let planner = LlmPlanner::new(
        Box::new(MockProvider::new([
            Ok(Completion {
                content: vec![ContentBlock::ToolUse {
                    id: "tu_rem".into(),
                    call_id: None,
                    name: "remember".into(),
                    input: serde_json::json!({"fact": "prefer vim-enhanced over vim"}),
                }],
                stop_reason: StopReason::ToolUse,
            }),
            propose_plan(
                "Confirm preference saved",
                &[("GetSystemState", "Confirm system is accessible", "low")],
            ),
        ])),
        Box::new(MockStateClient::default()),
        5,
    )
    .with_prefs_path(prefs_path.clone());

    let plan = planner.plan_intent("remember that I prefer vim-enhanced over vim").await.unwrap();
    assert_eq!(plan.steps()[0].action_name(), "GetSystemState");

    // Verify the preference was written.
    let content = std::fs::read_to_string(&prefs_path).unwrap();
    assert!(content.contains("prefer vim-enhanced over vim"));
}

#[tokio::test]
async fn forget_tool_removes_preference() {
    let dir = tempfile::tempdir().unwrap();
    let prefs_path = dir.path().join("prefs.md");
    // Pre-seed a preference.
    std::fs::write(&prefs_path, "- prefer vim-enhanced over vim\n").unwrap();

    let planner = LlmPlanner::new(
        Box::new(MockProvider::new([
            Ok(Completion {
                content: vec![ContentBlock::ToolUse {
                    id: "tu_fgt".into(),
                    call_id: None,
                    name: "forget".into(),
                    input: serde_json::json!({"fact": "prefer vim-enhanced over vim"}),
                }],
                stop_reason: StopReason::ToolUse,
            }),
            propose_plan(
                "Preference removed",
                &[("GetSystemState", "Confirm system", "low")],
            ),
        ])),
        Box::new(MockStateClient::default()),
        5,
    )
    .with_prefs_path(prefs_path.clone());

    planner.plan_intent("forget my vim preference").await.unwrap();

    let content = std::fs::read_to_string(&prefs_path).unwrap();
    assert!(!content.contains("vim-enhanced"));
}

#[tokio::test]
async fn remember_rejects_sensitive_data() {
    let dir = tempfile::tempdir().unwrap();
    let prefs_path = dir.path().join("prefs.md");

    let planner = LlmPlanner::new(
        Box::new(MockProvider::new([
            Ok(Completion {
                content: vec![ContentBlock::ToolUse {
                    id: "tu_rem".into(),
                    call_id: None,
                    name: "remember".into(),
                    input: serde_json::json!({"fact": "my password is hunter2"}),
                }],
                stop_reason: StopReason::ToolUse,
            }),
            propose_plan(
                "Cannot save sensitive data",
                &[("GetSystemState", "System check", "low")],
            ),
        ])),
        Box::new(MockStateClient::default()),
        5,
    )
    .with_prefs_path(prefs_path.clone());

    planner.plan_intent("remember my password").await.unwrap();

    // File should not exist or should be empty — the sensitive fact was rejected.
    assert!(!prefs_path.exists() || std::fs::read_to_string(&prefs_path).unwrap().is_empty());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p lacs-brain -- remember_tool_saves`
Expected: FAIL — `with_prefs_path` method not found

- [ ] **Step 3: Add `prefs_path` field and `with_prefs_path` to `LlmPlanner`**

In `crates/lacs-brain/src/planner.rs`, add to the `LlmPlanner` struct (after `audit_log` field):

```rust
    prefs_path: Option<std::path::PathBuf>,
```

In `LlmPlanner::new()`, set it to `None`:

```rust
    prefs_path: None,
```

Add a builder method after `with_audit_log`:

```rust
    /// Set the path to the user preferences file.
    ///
    /// When set, preferences are read at the start of each `plan_intent()`
    /// call and injected into the system prompt. The `remember` and `forget`
    /// tools write to this file.
    pub fn with_prefs_path(mut self, path: std::path::PathBuf) -> Self {
        self.prefs_path = Some(path);
        self
    }
```

In `from_config`, set the prefs path:

```rust
    let mut planner = Self::new(provider, state_client, config.max_turns);
    planner.prefs_path = Some(lacs_core::config::prefs_path());
    Ok(planner)
```

- [ ] **Step 4: Register `remember` and `forget` tools**

In `LlmPlanner::new()`, add the tool registrations to the `tools` vec (line ~356):

```rust
tools: {
    let mut t = vec![get_state_tool_def()];
    t.extend(query_tools());
    t.push(crate::planning_tools::preferences::remember_tool_def());
    t.push(crate::planning_tools::preferences::forget_tool_def());
    t.push(propose_plan_tool_def());
    t
},
```

- [ ] **Step 5: Handle `remember` and `forget` in the planning loop**

In the `plan_intent` method, in the `for (id, call_id, name, input) in &tool_calls` loop, add match arms for `"remember"` and `"forget"` BEFORE the `other_name` catch-all:

```rust
"remember" => {
    let fact = input
        .get("fact")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let result_text = if fact.is_empty() {
        "Error: 'fact' parameter must not be empty.".to_string()
    } else if crate::prefs::contains_sensitive(fact) {
        "Error: preference rejected — it appears to contain \
         sensitive data (passwords, tokens, keys). Preferences \
         must not store secrets."
            .to_string()
    } else if let Some(ref prefs_path) = self.prefs_path {
        match crate::prefs::append_pref(prefs_path, fact) {
            Ok(()) => format!("Preference saved: {fact}"),
            Err(e) => format!("Error saving preference: {e}"),
        }
    } else {
        "Error: preference storage is not configured.".to_string()
    };
    tool_results.push(ToolResultBlock {
        tool_use_id: id.clone(),
        call_id: call_id.clone(),
        content: result_text,
        is_error: false,
    });
}
"forget" => {
    let fact = input
        .get("fact")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let result_text = if fact.is_empty() {
        "Error: 'fact' parameter must not be empty.".to_string()
    } else if let Some(ref prefs_path) = self.prefs_path {
        match crate::prefs::remove_pref(prefs_path, fact) {
            Ok(true) => format!("Preference removed: {fact}"),
            Ok(false) => format!("Preference not found: {fact}"),
            Err(e) => format!("Error removing preference: {e}"),
        }
    } else {
        "Error: preference storage is not configured.".to_string()
    };
    tool_results.push(ToolResultBlock {
        tool_use_id: id.clone(),
        call_id: call_id.clone(),
        content: result_text,
        is_error: false,
    });
}
```

- [ ] **Step 6: Inject prefs into system prompt per call**

In `plan_intent`, before the `for turn` loop, rebuild the system prompt with current prefs:

```rust
let effective_prompt = {
    let prefs_content = self
        .prefs_path
        .as_ref()
        .and_then(|p| crate::prefs::read_prefs(p));
    build_system_prompt(prefs_content.as_deref())
};
```

Then change the `self.provider.complete(...)` call to use `&effective_prompt` instead of `&self.system_prompt`:

```rust
let completion = self
    .provider
    .complete(&effective_prompt, &messages, &self.tools, 4096)
    .await
    .map_err(PlanningError::from)?;
```

Note: `self.system_prompt` field can be removed now, but keeping it for backward compatibility with callers that don't set `prefs_path` is fine. The effective prompt is rebuilt per call. Remove the `system_prompt` field and use `build_system_prompt(None)` as baseline in the `effective_prompt` block.

- [ ] **Step 7: Run all tests**

Run: `cargo test -p lacs-brain`
Expected: ALL tests PASS (including the 3 new integration tests)

- [ ] **Step 8: Run clippy and fmt**

Run: `cargo fmt --all && cargo clippy -p lacs-brain --all-features --locked -- -D warnings`
Expected: Clean

- [ ] **Step 9: Commit**

```bash
git add crates/lacs-brain/src/planner.rs crates/lacs-brain/tests/planner.rs
git commit -m "feat(planner): integrate remember/forget tools and per-call prefs injection"
```

---

### Task 6: Documentation update

**Files:**
- Modify: `docs/developer-guide.md`
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update developer guide**

In `docs/developer-guide.md`, in the Configuration section (after the `config.toml` example), add:

```markdown
### User preferences

LACS remembers user preferences in `~/.config/lacs/prefs.md`. When
you tell LACS "remember that I prefer vim-enhanced over vim", the
preference is saved and applied to all future planning sessions.

Preferences are plain text, one per line, max 10 KB. Sensitive data
(passwords, API keys, tokens) is rejected automatically.

Manage preferences through natural language:
- "Remember that I always prefer vim-enhanced"
- "Forget my vim preference"

Or edit `~/.config/lacs/prefs.md` directly.
```

- [ ] **Step 2: Update CLAUDE.md**

In `CLAUDE.md`, add a new section after "Prompt Engineering — System Prompt Rules":

```markdown
## User Preferences — `prefs.md`

User preferences live in `~/.config/lacs/prefs.md` and are injected into the
system prompt at the start of each `plan_intent()` call. The `remember` and
`forget` planning tools modify this file during planning.

Preferences are NOT system state. They are user-stated intentions that inform
planning decisions. Do not store system facts as preferences — those are
queryable live via `query_*` tools.
```

- [ ] **Step 3: Commit**

```bash
git add docs/developer-guide.md CLAUDE.md
git commit -m "docs: document user preference memory feature"
```

---

### Task 7: E2E validation

- [ ] **Step 1: Run existing E2E stories to verify no regressions**

Run: `ANTHROPIC_API_KEY=sk-... tests/e2e/dev-stories.sh`
Expected: Stories 1-7 PASS (the prompt change must not break existing stories)

- [ ] **Step 2: Manual test of remember/forget flow**

Start the daemon and test CLI, then verify:
1. Send intent "remember that I prefer vim-enhanced over vim" — plan should include `GetSystemState`, prefs file should be created.
2. Send intent "install vim" — plan should propose `AddLayeredPackage` with `vim-enhanced` if prefs are working.
3. Send intent "forget my vim preference" — prefs file should be empty after.

- [ ] **Step 3: Commit any fixes found during E2E**

---

## Self-Review

**Spec coverage:** ✅ MEMORY.md pattern → `prefs.md`, ✅ atomic writes → `tempfile::NamedTempFile`, ✅ remember/forget tools → planning tools in brain, ✅ size limit → `PREFS_MAX_BYTES = 10_240`, ✅ sensitive data filter → `contains_sensitive()`.

**Placeholder scan:** No TBDs, TODOs, or "fill in later" found.

**Type consistency:** `build_system_prompt(Option<&str>)` signature used consistently in tasks 4 and 5. `with_prefs_path(PathBuf)` used consistently in planner and tests. `append_pref`/`remove_pref` signatures match between task 2 implementation and task 5 usage.

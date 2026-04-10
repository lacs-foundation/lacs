# LACS Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build the first production-grade LACS control plane for Fedora Silverblue, with typed privileged actions, approvals, previews, job execution, and OSS-ready project hygiene.

**Architecture:** Use a Rust workspace with a shared protocol crate, a privileged `lacs-daemon`, an unprivileged `zeroclaw-brain`, and a Tauri shell. The daemon owns authorization, policy, execution, transactions, and rollback metadata. The shell is a client and the brain is a planner only.

**Tech Stack:** Rust, Tokio, Tonic, Prost, Serde, SQLite, Tauri, TypeScript, Vite, pnpm, GitHub Actions, Fedora Silverblue tooling.

---

## Task 1: OSS Foundation and Repo Hygiene

**Files:**
- Create: `LICENSE`
- Create: `README.md`
- Create: `CONTRIBUTING.md`
- Create: `SECURITY.md`
- Create: `CODE_OF_CONDUCT.md`
- Create: `ROADMAP.md`
- Create: `.github/PULL_REQUEST_TEMPLATE.md`
- Create: `.github/ISSUE_TEMPLATE/bug_report.yml`
- Create: `.github/ISSUE_TEMPLATE/feature_request.yml`
- Create: `docs/architecture.md`
- Create: `docs/developer-guide.md`
- Create: `docs/adr/0001-system-boundaries.md`
- Create: `.github/workflows/ci.yml`

**Step 1: Write the failing checks**

- Add markdown linting and link validation to CI.
- Add a basic repository completeness check that fails if the required OSS files are missing.

**Step 2: Run checks to verify failure**

- Run: `markdownlint .`
- Run: `yamllint .github/ISSUE_TEMPLATE/*.yml`
- Run: `git ls-files | rg '^(LICENSE|README.md|CONTRIBUTING.md|SECURITY.md|CODE_OF_CONDUCT.md|ROADMAP.md|docs/|\.github/)'`
- Expected: missing files and lint errors until the templates exist.

**Step 3: Write minimal implementation**

- Fill in the OSS files with concise but complete project metadata.
- Keep the README focused on purpose, architecture, quick start, and safety model.
- Keep the contributing/security docs short and direct.

**Step 4: Run checks to verify pass**

- Run: `markdownlint .`
- Run: `yamllint .github/ISSUE_TEMPLATE/*.yml`
- Run: `git ls-files | rg '^(LICENSE|README.md|CONTRIBUTING.md|SECURITY.md|CODE_OF_CONDUCT.md|ROADMAP.md|docs/|\.github/)'`
- Expected: all required files present and lint clean.

**Step 5: Commit**

- `git add LICENSE README.md CONTRIBUTING.md SECURITY.md CODE_OF_CONDUCT.md ROADMAP.md docs/ .github/`
- `git commit -m "docs: add OSS foundation for LACS"`

---

## Task 2: Workspace and Shared Protocol Crate

**Files:**
- Create: `Cargo.toml`
- Create: `crates/lacs-core/Cargo.toml`
- Create: `crates/lacs-core/src/lib.rs`
- Create: `crates/lacs-proto/Cargo.toml`
- Create: `crates/lacs-proto/build.rs`
- Create: `crates/lacs-proto/proto/lacs/v1/lacs.proto`
- Create: `crates/lacs-proto/src/lib.rs`
- Create: `crates/lacs-types/Cargo.toml`
- Create: `crates/lacs-types/src/lib.rs`
- Create: `crates/lacs-types/tests/schema_roundtrip.rs`

**Step 1: Write the failing tests**

- Add round-trip tests for the shared request, preview, result, transaction, and error types.
- Add compile-time tests that the workspace builds with the generated protobuf types.

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-types`
- Run: `cargo test -p lacs-proto`
- Expected: fail because the workspace and generated types do not exist yet.

**Step 3: Write minimal implementation**

- Define the shared protobuf messages and enums for:
  - request envelope
  - preview envelope
  - result envelope
  - transaction record
  - failure categories
  - job states
- Expose ergonomic Rust types in `lacs-types`.

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-types`
- Run: `cargo test -p lacs-proto`
- Run: `cargo test`
- Expected: workspace builds and the type round trips pass.

**Step 5: Commit**

- `git add Cargo.toml crates/lacs-core crates/lacs-proto crates/lacs-types`
- `git commit -m "feat: add LACS workspace and shared protocol"`

---

## Task 3: Daemon Skeleton, Authorization, and Transaction Store

**Files:**
- Create: `crates/lacs-daemon/Cargo.toml`
- Create: `crates/lacs-daemon/src/main.rs`
- Create: `crates/lacs-daemon/src/lib.rs`
- Create: `crates/lacs-daemon/src/auth.rs`
- Create: `crates/lacs-daemon/src/policy.rs`
- Create: `crates/lacs-daemon/src/preview.rs`
- Create: `crates/lacs-daemon/src/jobs.rs`
- Create: `crates/lacs-daemon/src/transactions.rs`
- Create: `crates/lacs-daemon/src/state.rs`
- Create: `crates/lacs-daemon/src/transport/grpc.rs`
- Create: `crates/lacs-daemon/tests/bootstrap.rs`

**Step 1: Write the failing tests**

- Add tests for:
  - Unix-socket-only startup
  - role resolution from local groups
  - approval hash binding
  - transaction creation
  - job state transitions

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-daemon`
- Expected: fail until the daemon skeleton and store exist.

**Step 3: Write minimal implementation**

- Stand up the daemon with:
  - local socket listener
  - role lookup
  - transaction persistence in SQLite
  - job table
  - request hash/approval validation

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-daemon`
- Run: `cargo test`
- Expected: bootstrap, auth, and transaction tests pass.

**Step 5: Commit**

- `git add crates/lacs-daemon`
- `git commit -m "feat: add LACS daemon skeleton"`

---

## Task 4: Preview, Approval, and Job Lifecycle Engine

**Files:**
- Modify: `crates/lacs-daemon/src/policy.rs`
- Modify: `crates/lacs-daemon/src/preview.rs`
- Modify: `crates/lacs-daemon/src/jobs.rs`
- Modify: `crates/lacs-daemon/src/transactions.rs`
- Create: `crates/lacs-daemon/tests/preview_approval.rs`

**Step 1: Write the failing tests**

- Add tests for:
  - preview generation for low, medium, and high risk actions
  - stale approval rejection
  - job state transitions
  - cancellation behavior
  - `needs_reboot` handling

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-daemon preview_approval`
- Expected: fail until the engine exists.

**Step 3: Write minimal implementation**

- Implement deterministic preview generation.
- Bind approvals to the request hash.
- Model the job lifecycle as queued/running/succeeded/failed/canceled/rolled_back/needs_reboot.

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-daemon preview_approval`
- Run: `cargo test -p lacs-daemon`
- Expected: preview and approval tests pass.

**Step 5: Commit**

- `git add crates/lacs-daemon/src/policy.rs crates/lacs-daemon/src/preview.rs crates/lacs-daemon/src/jobs.rs crates/lacs-daemon/src/transactions.rs crates/lacs-daemon/tests/preview_approval.rs`
- `git commit -m "feat: add preview and approval engine"`

---

## Task 5: Core Action Families, Batch 1

**Files:**
- Create: `crates/lacs-daemon/src/actions/deployment.rs`
- Create: `crates/lacs-daemon/src/actions/flatpak.rs`
- Create: `crates/lacs-daemon/src/actions/toolbox.rs`
- Create: `crates/lacs-daemon/src/actions/layering.rs`
- Create: `crates/lacs-daemon/src/actions/package_repos.rs`
- Create: `crates/lacs-daemon/src/actions/containers.rs`
- Create: `crates/lacs-daemon/tests/actions_batch1.rs`

**Step 1: Write the failing tests**

- Add tests for the primary Silverblue workflows:
  - `GetSystemState`
  - `CollectDiagnostics`
  - `GetDeploymentHistory`
  - `PinDeployment`
  - `RebaseSystem`
  - `CleanupDeployments`
  - `RebootSystem`
  - `RollbackDeployment`
  - Flatpak install/remove/search
  - toolbox create/list/enter/remove
  - layered package add/remove/list
  - package repo add/remove/list/enable/disable
  - container list/create/start/stop/remove/info

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-daemon actions_batch1`
- Expected: fail until action adapters and schema validation exist.

**Step 3: Write minimal implementation**

- Implement typed adapters that call host tools safely.
- Keep execution shell-free and bounded.
- Preserve the transaction/preview/result record for every action.

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-daemon actions_batch1`
- Run: `cargo test -p lacs-daemon`
- Expected: core action tests pass.

**Step 5: Commit**

- `git add crates/lacs-daemon/src/actions crates/lacs-daemon/tests/actions_batch1.rs`
- `git commit -m "feat: implement core Silverblue actions"`

---

## Task 6: Core Action Families, Batch 2

**Files:**
- Create: `crates/lacs-daemon/src/actions/services.rs`
- Create: `crates/lacs-daemon/src/actions/network.rs`
- Create: `crates/lacs-daemon/src/actions/identity.rs`
- Create: `crates/lacs-daemon/src/actions/users.rs`
- Create: `crates/lacs-daemon/tests/actions_batch2.rs`

**Step 1: Write the failing tests**

- Add tests for:
  - service start/stop/restart/enable/mask/unmask/list/logs
  - wifi and DNS configuration
  - firewall preview and apply
  - hostname, timezone, locale, and NTP changes
  - user and group management

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-daemon actions_batch2`
- Expected: fail until the second action batch exists.

**Step 3: Write minimal implementation**

- Implement the remaining typed adapters.
- Ensure every action obeys the same approval, preview, and logging model.

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-daemon actions_batch2`
- Run: `cargo test -p lacs-daemon`
- Expected: all batch 2 tests pass.

**Step 5: Commit**

- `git add crates/lacs-daemon/src/actions crates/lacs-daemon/tests/actions_batch2.rs`
- `git commit -m "feat: implement admin action families"`

---

## Task 7: Brain Runtime and Shell UI

**Files:**
- Create: `crates/lacs-brain/Cargo.toml`
- Create: `crates/lacs-brain/src/lib.rs`
- Create: `crates/lacs-brain/src/planner.rs`
- Create: `crates/lacs-brain/src/state_client.rs`
- Create: `crates/lacs-brain/tests/planner.rs`
- Create: `apps/lacs-shell/package.json`
- Create: `apps/lacs-shell/src-tauri/Cargo.toml`
- Create: `apps/lacs-shell/src-tauri/src/main.rs`
- Create: `apps/lacs-shell/src-tauri/src/commands.rs`
- Create: `apps/lacs-shell/src-tauri/src/events.rs`
- Create: `apps/lacs-shell/src/App.tsx`
- Create: `apps/lacs-shell/src/components/IntentPane.tsx`
- Create: `apps/lacs-shell/src/components/PlanPane.tsx`
- Create: `apps/lacs-shell/src/components/ExecutionPane.tsx`
- Create: `apps/lacs-shell/src/components/TimelinePane.tsx`
- Create: `apps/lacs-shell/src/styles.css`

**Step 1: Write the failing tests**

- Add planner tests that verify:
  - a user intent yields a typed plan
  - the planner can read only curated state
  - the planner cannot generate mutating actions without daemon-mediated approval
- Add UI smoke tests for the four-pane layout and main state transitions.

**Step 2: Run tests to verify failure**

- Run: `cargo test -p lacs-brain`
- Run: `cargo test -p lacs-shell`
- Expected: fail until the planner and shell exist.

**Step 3: Write minimal implementation**

- Implement the brain as a planning client only.
- Implement the shell as a sparse, opinionated control surface.
- Wire preview, approval, progress, and timeline events to the daemon.

**Step 4: Run tests to verify pass**

- Run: `cargo test -p lacs-brain`
- Run: `cargo test -p lacs-shell`
- Expected: planner and UI smoke tests pass.

**Step 5: Commit**

- `git add crates/lacs-brain apps/lacs-shell`
- `git commit -m "feat: add brain runtime and shell UI"`

---

## Task 8: CI, Packaging, Release, and Launch Readiness

**Files:**
- Modify: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`
- Create: `.github/workflows/security.yml`
- Create: `packaging/fedora/lacs-daemon.service`
- Create: `packaging/fedora/lacs-shell.desktop`
- Create: `packaging/fedora/lacs.spec`
- Create: `scripts/dev.sh`
- Create: `scripts/test.sh`
- Create: `docs/release-process.md`
- Create: `docs/security-model.md`

**Step 1: Write the failing checks**

- Add CI gates for:
  - formatting
  - unit tests
  - integration tests
  - linting
  - docs checks
  - security metadata checks
- Add packaging validation for Fedora artifacts.

**Step 2: Run checks to verify failure**

- Run: `cargo fmt --check`
- Run: `cargo clippy --all-targets --all-features -- -D warnings`
- Run: `cargo test`
- Run: `markdownlint .`
- Expected: fail until CI and packaging are wired.

**Step 3: Write minimal implementation**

- Add GitHub Actions workflows.
- Add Fedora packaging stubs.
- Add release and security process docs.

**Step 4: Run checks to verify pass**

- Run: `cargo fmt --check`
- Run: `cargo clippy --all-targets --all-features -- -D warnings`
- Run: `cargo test`
- Run: `markdownlint .`
- Expected: all gates pass.

**Step 5: Commit**

- `git add .github/workflows packaging scripts docs/release-process.md docs/security-model.md`
- `git commit -m "chore: add CI and release foundation"`

---

## OSS Launch Notes

This project should be treated as a large open source project from day one.

That means:

- keep a clear README and onboarding path
- keep the contributing and security docs short and visible
- use branch protection and required checks
- keep changes small and reviewable
- cut releases with notes
- maintain a roadmap and an ADR trail
- preserve a strict safety model even when feature pressure increases

This is not a hobby repo. The implementation needs to look and behave like a project that other maintainers can safely join.

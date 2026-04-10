# Roadmap

This roadmap is intentionally high level. It is meant to show
contributors where the project is going and what matters next.

## Phase 1: Foundation

- publish the OSS project files
- keep the spec and implementation plan current
- establish CI, linting, and repository hygiene
- keep the architecture boundary explicit

## Phase 2: Protocol and Daemon

- implement the shared protocol crate
- implement the privileged daemon skeleton
- persist transactions and approvals
- generate previews and job state

## Phase 3: Core Actions

- deployment and boot controls
- Flatpak app lifecycle
- toolbox workflows
- layered package management
- package repository management
- container and runtime management
- services, network, identity, and user management

## Phase 4: Brain and Shell

- ~~implement the planner runtime~~ — done; `lacs-brain` has Anthropic
  and Ollama providers, a tool-use loop, plan validation, and risk
  classification
- implement the sparse shell UI
- wire previews, approvals, jobs, and timeline
- replace `DemoStateClient` with real daemon IPC

## Phase 5: Release Quality

- harden the test matrix
- improve packaging
- improve docs for contributors and users
- cut public releases
- stabilize the API surface

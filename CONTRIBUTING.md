# Contributing to LACS

Thanks for contributing. LACS is an open source project focused on
safe, auditable AI-driven Linux system management.

The full contributing guide is at
**[docs/contributing/CONTRIBUTING.md](docs/contributing/CONTRIBUTING.md)**.

It covers:

- How to find a good first issue
- Setting up your development environment
- The PR workflow (branch → implement → test → review → merge)
- How to add a new daemon action
- How to add an E2E user story
- Commit style, code standards, and the quality bar

The short version:

```sh
# Clone and set up
git clone https://github.com/lacs-foundation/lacs
cd lacs
pip install pre-commit && pre-commit install
cd apps/lacs-shell && pnpm install && cd ../..

# Run all tests
cargo test --workspace --locked
cd apps/lacs-shell && pnpm test && pnpm exec tsc --noEmit && cd ../..

# Open an issue, branch off main, implement with TDD, then open a PR
```

For security-sensitive issues (auth bypass, privilege escalation,
data exposure), follow [SECURITY.md](SECURITY.md) instead of opening
a public issue.

Questions? Open a [GitHub Discussion](https://github.com/lacs-foundation/lacs/discussions).

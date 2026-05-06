# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rtk (Rust Token Killer)** is a high-performance CLI proxy that minimizes LLM token consumption by filtering and compressing command outputs. It achieves 60-90% token savings on common development operations through smart filtering, grouping, truncation, and deduplication.

This is a fork with critical fixes for git argument parsing and modern JavaScript stack support (pnpm, vitest, Next.js, TypeScript, Playwright, Prisma).

### Name Collision Warning

**Two different "rtk" projects exist:**
- This project: Rust Token Killer (algolia/rtk, fork of rtk-ai/rtk)
- reachingforthejack/rtk: Rust Type Kit (DIFFERENT - generates Rust types)

**Verify correct installation:**
```bash
rtk --version  # Should show the version from Cargo.toml (rtk 0.38.0-algolia.1 or newer)
rtk gain       # Should show token savings stats (NOT "command not found")
```

If `rtk gain` fails, you have the wrong package installed.

## Development Commands

> **Note**: If rtk is installed, prefer `rtk <cmd>` over raw commands for token-optimized output.
> All commands work with passthrough support even for subcommands rtk doesn't specifically handle.

### Build & Run
```bash
cargo build                   # raw
rtk cargo build               # preferred (token-optimized)
cargo build --release         # release build (optimized)
cargo run -- <command>        # run directly
cargo install --path .        # install locally
```

### Testing
```bash
cargo test                    # all tests
rtk cargo test                # preferred (token-optimized)
cargo test <test_name>        # specific test
cargo test <module_name>::    # module tests
cargo test -- --nocapture     # with stdout
bash scripts/test-all.sh      # smoke tests (installed binary required)
```

### Linting & Quality
```bash
cargo check                   # check without building
cargo fmt                     # format code
cargo clippy --all-targets    # all clippy lints
rtk cargo clippy --all-targets # preferred
```

### Pre-commit Gate
```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all
```

### Package Building
```bash
cargo deb                     # DEB package (needs cargo-deb)
cargo generate-rpm            # RPM package (needs cargo-generate-rpm, after release build)
```

## Architecture

rtk uses a **command proxy architecture**: `main.rs` routes CLI commands via a Clap `Commands` enum to specialized filter modules in `src/cmds/*/`, each of which executes the underlying command and compresses its output. Token savings are tracked in SQLite via `src/core/tracking.rs`.

For the full architecture, component details, and module development patterns, see:
- [ARCHITECTURE.md](docs/contributing/ARCHITECTURE.md) — System design, module organization, filtering strategies, error handling
- [docs/contributing/TECHNICAL.md](docs/contributing/TECHNICAL.md) — End-to-end flow, folder map, hook system, filter pipeline

Module responsibilities are documented in each folder's `README.md` and each file's `//!` doc header. Browse `src/cmds/*/` to discover available filters.

Supported ecosystems: git/gh/gt, cargo, go/golangci-lint, npm/pnpm/npx, ruff/pytest/pip/mypy, rspec/rubocop/rake, dotnet, playwright/vitest/jest, docker/kubectl/aws.

### Proxy Mode

**Purpose**: Execute commands without filtering but track usage for metrics.

**Usage**: `rtk proxy <command> [args...]`

**Benefits**:
- **Bypass RTK filtering**: Workaround bugs or get full unfiltered output
- **Track usage metrics**: Measure which commands Claude uses most (visible in `rtk gain --history`)
- **Guaranteed compatibility**: Always works even if RTK doesn't implement the command

**Examples**:
```bash
rtk proxy git log --oneline -20    # Full git log output (no truncation)
rtk proxy npm install express      # Raw npm output (no filtering)
rtk proxy curl https://api.example.com/data  # Any command works
```

All proxy commands appear in `rtk gain --history` with 0% savings (input = output).

## Coding Rules

Rust patterns, error handling, and anti-patterns are defined in `.claude/rules/rust-patterns.md` (auto-loaded into context). Key points:

- **anyhow::Result** everywhere, always `.context("description")?`
- **No unwrap()** in production code
- **lazy_static!** for all regex (never compile inside a function)
- **Fallback pattern**: if filter fails, execute raw command unchanged
- **No async**: single-threaded by design (startup <10ms)
- **Exit code propagation**: `std::process::exit(code)` on child failure

Testing strategy and performance targets are defined in `.claude/rules/cli-testing.md` (auto-loaded). Key targets: <10ms startup, <5MB memory, 60-90% token savings.

For contribution workflow and design philosophy, see [CONTRIBUTING.md](CONTRIBUTING.md). For the step-by-step filter implementation checklist, see [src/cmds/README.md](src/cmds/README.md#adding-a-new-command-filter).

## Build Verification (Mandatory)

**CRITICAL**: After ANY Rust file edits, ALWAYS run the full quality check pipeline before committing:

```bash
cargo fmt --all && cargo clippy --all-targets && cargo test --all
```

**Rules**:
- Never commit code that hasn't passed all 3 checks
- Fix ALL clippy warnings before moving on (zero tolerance)
- If build fails, fix it immediately before continuing to next task

**Performance verification** (for filter changes):
```bash
hyperfine 'rtk git log -10' --warmup 3          # before
cargo build --release
hyperfine 'target/release/rtk git log -10' --warmup 3  # after (should be <10ms)
```

## Working Directory Confirmation

**ALWAYS confirm working directory before starting any work**:

```bash
pwd  # Verify you're in the rtk project root
git branch  # Verify correct branch (main, feature/*, etc.)
```

**Never assume** which project to work in. Always verify before file operations.

## Avoiding Rabbit Holes

**Stay focused on the task**. Do not make excessive operations to verify external APIs, documentation, or edge cases unless explicitly asked.

**Rule**: If verification requires more than 3-4 exploratory commands, STOP and ask the user whether to continue or trust available info.

**Examples of rabbit holes to avoid**:
- Excessive regex pattern testing (trust snapshot tests, don't manually verify 20 edge cases)
- Deep diving into external command documentation (use fixtures, don't research git/cargo internals)
- Over-testing cross-platform behavior (test macOS + Linux, trust CI for Windows)
- Verifying API signatures across multiple crate versions (use docs.rs if needed, don't clone repos)

**When to stop and ask**:
- "Should I research X external API behavior?" → ASK if it requires >3 commands
- "Should I test Y edge case?" → ASK if not mentioned in requirements
- "Should I verify Z across N platforms?" → ASK if N > 2

## Fork Policy (algolia/rtk)

This repository is a **fork** of [rtk-ai/rtk](https://github.com/rtk-ai/rtk).
The fork stays as thin as possible: upstream is authoritative, we add the
minimum needed for Algolia.

### What we change from upstream
- **Telemetry stripped**: no phone-home, no `ureq` / `hostname` deps,
  no opt-out config, no `RTK_TELEMETRY_*` env vars
- **No Homebrew tap**: install via `cargo install --git` or `install.sh`
- **Branding**: all install URLs and repo references point to `algolia/rtk`
- **Targeted bug fixes**: only when upstream hasn't addressed the issue.
  Each lives under `bug-reports/` with a reproducer.

### What we DON'T change
- All upstream features, filters, commands, TOML DSL, rewrite engine
- Test suite (must pass)
- Architecture and module structure

### Upstream Catchup Procedure (re-fork strategy)

When upstream has new releases to absorb, **do not merge** — re-fork.
Merging accumulates conflict-resolution noise and can silently re-apply
patches that upstream has fixed. The re-fork procedure forces patch
re-validation at every catchup:

```bash
# 1. Fetch upstream
git remote add upstream https://github.com/rtk-ai/rtk.git  # one-time
git fetch upstream

# 2. Branch off upstream/master directly
git checkout -b fork/upstream-realign-vX.Y.Z upstream/master

# 3. Strip telemetry (always required)
#    - Delete src/core/telemetry.rs, src/core/telemetry_cmd.rs
#    - Remove `pub mod telemetry;` and `pub mod telemetry_cmd;` from
#      src/core/mod.rs
#    - Remove `core::telemetry::maybe_ping();` from main.rs
#    - Remove the `Telemetry` Commands enum variant + match arm in main.rs
#    - Remove TelemetryConfig + tests from src/core/config.rs
#    - Remove prompt_telemetry_consent / save_telemetry_consent from
#      src/hooks/init.rs and the call site
#    - Remove `ureq` (and `hostname` if present) from Cargo.toml
#    - Remove RTK_TELEMETRY_URL/TOKEN env vars from .github/workflows/release.yml
#    - Delete docs/TELEMETRY.md, docs/guide/resources/telemetry.md
#    - Strip Privacy & Telemetry sections from README*.md and other docs

# 4. Fork hygiene rebrand (see below)
#    sed-based rewrite of rtk-ai/rtk → algolia/rtk in install instructions,
#    rtk-ai.app links, master → main where appropriate

# 5. Re-test bug-reports/*.md against the new base.
#    Upstream fixes most issues over time — re-apply ONLY patches still
#    needed. Each preserved patch should reference its bug-report file
#    in the commit message.

# 6. Bump version in Cargo.toml: 0.X.Y → 0.X.Y-algolia.1
#    Also update .release-please-manifest.json

# 7. Build + test
cargo fmt --all && cargo clippy --all-targets && cargo test --all

# 8. Open a PR from fork/upstream-realign-vX.Y.Z against main.
#    The PR diff against main is large (300+ commits absorbed), but the
#    diff against upstream/master is small (telemetry strip + 2-3
#    targeted patches + rebrand). Reviewers should review it the second
#    way: `git diff upstream/master..fork/upstream-realign-vX.Y.Z`.

# 9. Once approved, the merge replaces main. Force-push if your team's
#    convention requires linear history; merge commit if not.
```

**Key principle**: every catchup re-validates every patch. Patches that
upstream silently fixed get dropped automatically because the re-fork
starts from a clean upstream tree.

## Fork Hygiene (Mandatory)

This is the **Algolia fork** (`algolia/rtk`), not the upstream (`rtk-ai/rtk`).
Upstream references leak in during rebases and releases. **Every rebase
and every release MUST include a fork hygiene check.**

### Pre-commit Checklist (after rebase or before release)

Run this grep to catch upstream leaks:

```bash
rg -i 'brew install rtk[^-]|rtk-ai\.app|contact@rtk|"rtk 0\.\d+\.\d+"' --glob '*.md' --glob '*.rb'
```

**Zero matches required.** Any hit must be fixed before commit. Historical
references in `CHANGELOG.md` (auto-generated from upstream commits) are
the one exception.

### Banned Patterns in User-Facing Docs

| Pattern | Why | Replace With |
|---------|-----|--------------|
| `brew install rtk` | No Homebrew tap for fork | `cargo install --git https://github.com/algolia/rtk` or `curl \| sh` |
| `https://www.rtk-ai.app` | Upstream website | Remove or use `https://github.com/algolia/rtk` |
| `contact@rtk-ai.app` | Upstream email | `#proj-internal-skills` on Slack |
| `rtk-ai/rtk` in install instructions | Upstream repo | `algolia/rtk` |
| `brew uninstall rtk` | No Homebrew install exists | `cargo uninstall rtk` |
| Hardcoded version strings (`"rtk 0.28.2"`) | Goes stale on every release | Use current `Cargo.toml` version |

### Where to Check

All `README*.md`, `INSTALL.md`, `CLAUDE.md`, `openclaw/README.md`,
`Formula/rtk.rb`, GitHub repo metadata (`gh repo edit --homepage`).

### On Release

1. Run the grep above — fix any matches
2. Update version strings in docs to match `Cargo.toml`
3. Verify `gh repo view --json homepageUrl` returns the algolia URL
   (not upstream)

## Plan Execution Protocol

When user provides a numbered plan (QW1-QW4, Phase 1-5, sprint tasks, etc.):

1. **Execute sequentially**: Follow plan order unless explicitly told otherwise
2. **Commit after each logical step**: One commit per completed phase/task
3. **Never skip or reorder**: If a step is blocked, report it and ask before proceeding
4. **Track progress**: Use task list (TaskCreate/TaskUpdate) for plans with 3+ steps
5. **Validate assumptions**: Before starting, verify all referenced file paths exist and working directory is correct

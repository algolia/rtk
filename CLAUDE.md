# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**rtk (Rust Token Killer)** is a high-performance CLI proxy that minimizes LLM token consumption by filtering and compressing command outputs. It achieves 60-90% token savings on common development operations through smart filtering, grouping, truncation, and deduplication.

This is a fork with critical fixes for git argument parsing and modern JavaScript stack support (pnpm, vitest, Next.js, TypeScript, Playwright, Prisma).

### Name Collision Warning

**Two different "rtk" projects exist:**
- This project: Rust Token Killer (algolia/rtk)
- reachingforthejack/rtk: Rust Type Kit (DIFFERENT - generates Rust types)

**Verify correct installation:**
```bash
rtk --version  # Should show "rtk 0.42.x-algolia.y" (or newer)
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
- [docs/TECHNICAL.md](docs/contributing/TECHNICAL.md) — End-to-end flow, folder map, hook system, filter pipeline

Module responsibilities are documented in each folder's `README.md` and each file's `//!` doc header. Browse `src/cmds/*/` to discover available filters.

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

## Fork Hygiene (Mandatory)

This is the **Algolia fork** (`algolia/rtk`), not the upstream (`rtk-ai/rtk`). Upstream references leak in during rebases and releases. **Every rebase and every release MUST include a fork hygiene check.**

### Pre-commit Checklist (after rebase or before release)

Run the hygiene gate — it catches every known leak class (repo slug, website,
email, Homebrew, stale version strings, telemetry residue in docs/source, dead
links to deleted telemetry docs):

```bash
scripts/fork-hygiene.sh          # CHECK only — exit 1 on any leak
scripts/fork-hygiene.sh --fix    # auto-fix the deterministic ones, then CHECK
```

**Zero matches required.** `--fix` handles repo/website/email/brew/version
mechanically; telemetry scrub, legal text (LICENSE/CLA), and code-comment
provenance are left for manual judgment (see the script header).

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

All `README*.md`, `INSTALL.md`, `CLAUDE.md`, `openclaw/README.md`, `Formula/rtk.rb`, GitHub repo metadata (`gh repo edit --homepage`).

### On Release

Use the driver — it encodes every step and can't forget the asset publish:

```bash
scripts/ship.sh <X.Y.Z-algolia.N>      # gate → hygiene → bump → commit → tag → push → dispatch
```

It runs `gh workflow run release.yml` itself. **This dispatch is mandatory and easy to
forget**: release-please/CD is disabled on the fork, so a tag push alone publishes NO
binaries (that is why `v0.42.0-algolia.2` has a tag but no GitHub release/assets).

Manual fallback / what the driver does:
1. `scripts/fork-hygiene.sh` — fix any matches
2. Bump `Cargo.toml` (+`cargo update -p rtk`); keep `.release-please-manifest.json` at base; hand-add the `CHANGELOG.md` entry
3. Commit `chore(release): X.Y.Z-algolia.N` (no AI-fingerprint trailers), tag `vX.Y.Z-algolia.N`, push branch + tag
4. **`gh workflow run release.yml -f tag=vX.Y.Z-algolia.N -f prerelease=false`** ← publishes the 5-platform assets
5. Verify: `gh release view vX.Y.Z-algolia.N` shows assets; `gh repo view --json homepageUrl` empty (not upstream URL)

## Upstream Catchup Procedure

Full realignment, **not** cherry-picks. Default target is `upstream/master`
(latest stable tag), not `develop`. Result is one squashed commit on top of the
upstream tag (see `git log` for prior `fork: upstream catchup ...` commits).

1. **Fetch + measure**: `git fetch upstream --tags`; compare `main..upstream/master`.
2. **Branch from the tag**: `git checkout -b fork/upstream-realign-vX.Y.Z upstream/master`.
3. **Toolchain**: upstream needs a modern cargo (edition2024 deps). If system
   `cargo` is old, use the rustup shim: `~/.cargo/bin/cargo` (run `rustup update stable` first).
4. **Strip telemetry** (hard rule): delete `src/core/telemetry*.rs` + telemetry docs;
   remove the `[telemetry]` config, `rtk telemetry` command + consent flow, `maybe_ping()`,
   and the `ureq` dep. Then `cargo check` and delete whatever stat helpers it reports as
   newly-dead (they only fed telemetry). Keep local SQLite tracking (`gain`/`discover`).
5. **Re-apply fork code fixes**: diff our patches in isolation with
   `git diff <prev-base-tag>..main -- src/` to see what's ours; re-apply anything
   upstream hasn't absorbed (currently: `registry.rs` shell-function + curl/wget pipe skips).
6. **Identity + fork artifacts**: `scripts/fork-hygiene.sh --fix`, then scrub any
   telemetry residue from docs by hand. Restore fork-only files absent from the upstream
   base — `scripts/fork-hygiene.sh`, `scripts/ship.sh`, the fork `CLAUDE.md`,
   `docs/bugs/` — from the old `main` if the branch switch dropped them; fix doc-links
   to the current layout. (`git checkout main -- scripts/ship.sh scripts/fork-hygiene.sh`.)
7. **Rationalize `.claude/` skills**: skills are checked into the repo, so upstream
   ones land on every catchup carrying upstream assumptions. Audit `.claude/skills/*`
   (and `.claude/commands/*`) against fork reality — the worst offender is **`/ship`**
   (`.claude/skills/ship/SKILL.md`): it assumes plain semver (we use `X.Y.Z-algolia.N`),
   release-please-generated CHANGELOG (disabled on fork — we hand-edit), `git push origin main`,
   crates.io/Homebrew publish, and — critically — `Co-Authored-By: Claude` commit
   trailers (**hard-banned**, no AI fingerprints). Fix or fork-annotate any skill whose
   steps don't match `### On Release` / `## Fork Hygiene` here before trusting it.
8. **Re-apply CI guards**: release-asset verification + `main`-branch triggers (see `release.yml`/`cd.yml`).
9. **Version**: `Cargo.toml` → `X.Y.Z-algolia.N`; keep `.release-please-manifest.json`
   at the upstream base `X.Y.Z`; add a `CHANGELOG.md` fork entry.
10. **Gate**: `cargo fmt --all && cargo clippy --all-targets && cargo test --all`
   and `scripts/fork-hygiene.sh`. All green → squash-commit → tag `vX.Y.Z-algolia.N`.

## Plan Execution Protocol

When user provides a numbered plan (QW1-QW4, Phase 1-5, sprint tasks, etc.):

1. **Execute sequentially**: Follow plan order unless explicitly told otherwise
2. **Commit after each logical step**: One commit per completed phase/task
3. **Never skip or reorder**: If a step is blocked, report it and ask before proceeding
4. **Track progress**: Use task list (TaskCreate/TaskUpdate) for plans with 3+ steps
5. **Validate assumptions**: Before starting, verify all referenced file paths exist and working directory is correct

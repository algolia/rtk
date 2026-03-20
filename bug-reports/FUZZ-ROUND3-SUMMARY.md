# Fuzzer Round 3: Expanded Coverage Results

**Date**: 2026-03-20
**Fuzzer version**: v3 (35 families, 139 static tests, 7 heuristics)
**Branch**: `feature/agentic-fuzzing`

## Results Summary

| Metric | Value |
|--------|-------|
| Total tests | 139 |
| Executed | 129 |
| Passed | 82 |
| Warnings | 9 |
| **Failures** | **38** |
| Skipped | 10 |
| **Failure rate** | **29%** |

## Issue Distribution

| Issue Type | Count | Description |
|------------|-------|-------------|
| DATA_LOSS | 29 | Output content missing or mangled |
| EXIT_CODE_MISMATCH | 28 | RTK returns different exit code than raw command |
| STDERR_LOSS | 8 | Stderr content consumed by filter, not passed through |
| FORMAT_ALTERED | 3 | Machine-readable output format changed |
| JSON_MANGLED | 1 | Valid JSON input produces invalid JSON output |

## New Bug Classes Discovered

### FUZZ-006: Docker — All flags rejected by Clap (12 failures)

**Severity**: HIGH
**Status**: OPEN
**Affected**: `docker ps`, `docker images` — every flag beyond the bare command

RTK's Docker Clap schema defines zero extra args for `ps` and `images`. Any flag (`-a`, `-q`, `--format`, `--no-trunc`, `--filter`, `--digests`) causes Clap rejection (exit 2).

```bash
docker ps -a                    # works
rtk docker ps -a                # "error: unexpected argument '-a'"

docker images --format json     # works
rtk docker images --format json # "error: unexpected argument '--format'"
```

**Root cause**: `DockerCommands::Ps` and `DockerCommands::Images` have no `#[arg(trailing_var_arg)]` or extra flag definitions. Fixed-shape command only.

**Impact**: Docker users can only use bare `docker ps` and `docker images` through RTK. All customization flags rejected.

---

### FUZZ-007: pip — Format override + show/list broken (7 failures)

**Severity**: HIGH
**Status**: OPEN
**Affected**: `pip list`, `pip show`, `pip list --outdated`, `pip list --not-required`

Two distinct issues:

**7a.** `pip list` forces `--format=json` internally, ignoring user's `--format=columns` or `--format=freeze`:
```bash
pip list --format=freeze        # absl-py==2.3.1\n...
rtk pip list --format=freeze    # error: unexpected argument '--format=freeze'
```

**7b.** `pip show <package>` exits 1 instead of 0:
```bash
pip show requests               # Name: requests\nVersion: 2.32.3\n...  (exit 0)
rtk pip show requests           # (nothing or error)  (exit 1)
```

**7c.** `pip list --not-required` rejected by Clap:
```bash
pip list --not-required         # works (exit 0)
rtk pip list --not-required     # error: unexpected argument (exit 2)
```

**Root cause**: pip module takes raw `args: &[String]` but list/outdated always force `--format=json`. Other subcommand args not validated against Clap properly.

---

### FUZZ-008: npm — Hardcoded to `npm run` (3 failures)

**Severity**: MEDIUM
**Status**: OPEN
**Affected**: `npm list`, `npm config`, and any npm subcommand that isn't `run`

```bash
npm list --depth=0              # lists dependencies (exit 0)
rtk npm list --depth=0          # "npm ERR! Missing script: list" (exit 1)

npm list --depth=0 --json       # valid JSON output
rtk npm list --depth=0 --json   # "npm ERR! Missing script: list" + mangled JSON
```

**Root cause**: `npm_cmd.rs` hardcodes `npm run` as the base command. ALL args are appended after `run`. So `rtk npm list` becomes `npm run list` which fails.

**Impact**: Only `npm run <script>` works through RTK. `npm list`, `npm outdated`, `npm view`, `npm config` all broken.

---

### FUZZ-009: find — Clap misinterprets flags (3 failures)

**Severity**: MEDIUM
**Status**: OPEN (previously identified in round 2)
**Affected**: `find . -name`, `find . -type`, all find predicates

```bash
find . -maxdepth 2 -name '*.rs'    # lists .rs files (exit 0)
rtk find . -maxdepth 2 -name '*.rs' # "error: unexpected argument '-name'" (exit 2)
```

**Root cause**: RTK's `find` command uses its own glob-based implementation, not system find. The Clap args don't accept find-style predicates (`-name`, `-type`, `-iname`).

**Impact**: Any non-trivial find command fails. Only basic `rtk find .` works.

---

### FUZZ-010: cargo clippy/test — stderr consumed (5 failures)

**Severity**: MEDIUM
**Status**: OPEN (previously identified in round 2)
**Affected**: `cargo clippy`, `cargo test` with `--` separator

Cargo clippy produces warnings on stderr. RTK's filter reads stderr, summarizes it to stdout, but loses content in the process:

```bash
cargo clippy -q 2>&1           # warning[E0123]: unused variable...
rtk cargo clippy -q 2>&1       # (summarized, loses individual warnings)
```

Cargo test with `--` separator:
```bash
cargo test -- --test-threads=1  # runs tests single-threaded (exit 101 if failures)
rtk cargo test -- --test-threads=1  # different exit code (1 vs 101)
```

---

### FUZZ-011: git branch -a — Remote branches lost (1 failure)

**Severity**: LOW
**Status**: OPEN
**Affected**: `git branch -a` (all branches including remotes)

```bash
git branch -a                   # lists local + remote branches
rtk git branch -a               # lists only local branches (remote refs missing)
```

**Root cause**: Branch filter strips remote branch lines (starting with `remotes/`).

---

### FUZZ-012: diff — Standard diff output mangled (2 failures)

**Severity**: LOW
**Status**: OPEN (related to existing diff implementation)

Standard diff and unified diff both lose content:

```bash
diff src/main.rs Cargo.toml     # standard diff output with line ranges
rtk diff src/main.rs Cargo.toml # ultra-condensed, loses line range markers

diff -u src/main.rs Cargo.toml  # unified diff with --- +++ markers
rtk diff -u src/main.rs Cargo.toml # loses --- +++ headers and context
```

**Note**: This may be intentional compression behavior. Worth classifying as acceptable or not.

---

## Warnings (non-critical, may be intentional)

| Family | Command | Issue |
|--------|---------|-------|
| grep | `rg 'fn ' . --json` | LINE_EXPANSION (JSON is verbose) |
| grep | `rg 'fn ' . -A 2`, `-B 1` | LINE_EXPANSION (context lines) |
| git-log | `git log --graph --oneline -10` | MARKER_INJECTION (graph decorations) |
| large-output | `git log --stat -50` | MARKER_INJECTION |
| large-output | `git log --oneline -100` | MARKER_INJECTION |

## Progression

| Round | Tests | Families | Failure Rate | New Bugs |
|-------|-------|----------|-------------|----------|
| Round 1 | 64 | 17 | 47% | 5 (FUZZ-001 to 005) |
| Round 2 | 85 | 23 | 20% → 17% | 3 (separator, stderr, find) |
| **Round 3** | **139** | **35** | **29%** | **7 (FUZZ-006 to 012)** |

Note: Round 3 failure rate increased because we added tests targeting known-weak modules (docker, pip, npm) — the NEW tests exposed more bugs while previously-fixed families still pass.

## Bug Priority for Fixes

### HIGH (user-facing, common commands)
1. **FUZZ-006**: Docker flags rejected — 12 failures, blocks all Docker flag usage
2. **FUZZ-007**: pip format/show broken — 7 failures, blocks pip customization

### MEDIUM (functional but lossy)
3. **FUZZ-008**: npm hardcoded to `npm run` — 3 failures, `npm list`/`npm config` broken
4. **FUZZ-009**: find predicates rejected — 3 failures, only basic find works
5. **FUZZ-010**: cargo stderr consumed — 5 failures, exit code + content loss

### LOW (compression artifacts, may be acceptable)
6. **FUZZ-011**: git branch -a data loss — remote branches filtered
7. **FUZZ-012**: diff data loss — intentional compression vs. data loss debate

## Fix Strategy

All HIGH/MEDIUM bugs share the same pattern: **Clap schema too narrow, or format override without escape hatch.**

The fix is always one of:
1. **Add `trailing_var_arg = true`** to accept unknown flags (docker, find)
2. **Detect format-changing flags and passthrough** (pip --format, npm subcommands)
3. **Route to correct subcommand** (npm list vs npm run)
4. **Preserve exit codes** (cargo test -- separator)

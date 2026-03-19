# FUZZ-003: git diff --name-only/--name-status filtered incorrectly

**Severity**: MEDIUM
**Status**: FIXED (round 2 — 3553b33)
**Discovered by**: Agentic fuzzer, 2026-03-19
**Affected modules**: `src/git.rs` (run_diff)

## Summary

RTK's git diff filter expected unified diff format (`+`/`-` lines, `@@` hunks), but `--name-only` produces bare filenames and `--name-status` produces tab-separated status+filename lines. The filter produced garbled output or silently dropped lines.

## Reproduction

```bash
# Raw git diff lists changed files
git diff --name-only HEAD~3
# src/main.rs
# src/git.rs

# RTK tried to parse as unified diff
rtk git diff --name-only HEAD~3
# (garbled or empty — filter couldn't find +/- markers)
```

## Root cause

`run_diff()` checked for `--stat` to decide passthrough, but didn't account for `--name-only` or `--name-status` which also produce non-unified-diff output.

## Fix

Extended the passthrough condition in `run_diff()` to detect `--name-only` and `--name-status` flags alongside `--stat`.

```rust
let wants_name_only = args
    .iter()
    .any(|arg| arg == "--name-only" || arg == "--name-status");

if wants_stat || wants_name_only || !wants_compact {
    // passthrough
}
```

## Impact

Broke `git diff --name-only | xargs` patterns common in pre-commit hooks, CI scripts, and code review tooling.

## Heuristics triggered

- DATA_LOSS (filenames missing from output)
- FORMAT_ALTERED (bare filename list mangled into diff-like format)

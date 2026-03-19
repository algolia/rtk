# FUZZ-001: git log/show filter applied on custom format flags

**Severity**: HIGH
**Status**: FIXED (round 1 — 9ece88c)
**Discovered by**: Agentic fuzzer, 2026-03-19
**Affected modules**: `src/git.rs` (run_log, run_show)

## Summary

RTK applied its condensing filter and injected `--no-merges` even when the user requested a custom output format via `--format`, `--pretty`, `--oneline`, `--raw`, or `--graph`. This silently altered machine-readable output that scripts and tooling depend on.

## Reproduction

```bash
# Raw git produces custom format
git log --format='%H' -5
# a1b2c3d4...
# e5f6g7h8...

# RTK mangled it — injected --no-merges, applied filter
rtk git log --format='%H' -5
# (filtered output, missing merge commits)
```

Same pattern for `git show`:
```bash
git show --name-only HEAD    # lists changed files
rtk git show --name-only HEAD  # filter garbled the output
```

## Root cause

`run_log()` and `run_show()` unconditionally applied RTK's git filter regardless of whether format-changing flags were present. The filter assumes standard `git log` output format.

## Fix

Detect format-changing flags (`--format`, `--pretty`, `--oneline`, `--raw`, `--graph`, `--name-only`, `--name-status`, `--no-patch`, `-p`) and passthrough verbatim — no filter, no `--no-merges` injection.

## Impact

Broke scripting workflows that parse git output (CI/CD, commit hooks, release tools). Any `git log --format=...` piped to `awk`/`sed`/`jq` would receive unexpected input.

## Heuristics triggered

- FORMAT_ALTERED (custom format mangled)
- DATA_LOSS (merge commits dropped by --no-merges injection)

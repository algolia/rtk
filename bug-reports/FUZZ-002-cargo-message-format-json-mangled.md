# FUZZ-002: cargo --message-format=json output mangled by filter

**Severity**: HIGH
**Status**: FIXED (round 2 — 3553b33)
**Discovered by**: Agentic fuzzer, 2026-03-19
**Affected modules**: `src/cargo_cmd.rs` (run_build, run_clippy, run_check)

## Summary

RTK's cargo filter assumed human-readable compiler output on stderr, but `--message-format=json` (and `--message-format=short`) produce machine-readable output. The filter parsed and mangled JSON lines, breaking downstream consumers.

## Reproduction

```bash
# Raw cargo produces NDJSON on stdout
cargo build --message-format=json 2>/dev/null
# {"reason":"compiler-artifact","package_id":"...","target":{...},...}

# RTK filtered it — broke JSON structure
rtk cargo build --message-format=json 2>/dev/null
# (mangled output, invalid JSON)
```

Same for `cargo clippy --message-format=json` and `cargo check --message-format=json`.

## Root cause

`run_build()`, `run_clippy()`, and `run_check()` unconditionally applied `filter_cargo_build()` which expects human-readable `error[E0xxx]` / `warning:` lines. When `--message-format=json` is present, cargo emits structured JSON instead.

## Fix

Added `has_message_format_flag()` detection and `run_cargo_passthrough()` function. When `--message-format` is present in args, bypass filtering entirely and print raw output.

## Impact

Broke IDE integrations (rust-analyzer, IntelliJ Rust) and CI pipelines that consume cargo's JSON output for error reporting, artifact tracking, and dependency resolution.

## Heuristics triggered

- JSON_MANGLED (valid JSON in, invalid JSON out)
- FORMAT_ALTERED (machine-readable flag present, output altered)

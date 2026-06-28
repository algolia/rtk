# RTK panics (Rust backtrace) on broken pipe when output is piped to `head`

- **Date:** 2026-06-26
- **Severity:** low-medium (cosmetic when output is already captured; alarming backtrace, and a non-zero/abnormal exit can mislead callers)
- **Affected component:** output writer / stdout handling (SIGPIPE not handled)
- **`rtk --version`:** rtk 0.42.0-algolia.4

## Summary (root cause)

When an rtk-proxied command's stdout is closed early by a downstream consumer
(classic `... | head -N`), rtk's Rust process panics on the failed write instead
of treating `EPIPE`/`SIGPIPE` as a normal end-of-consumer. The actual command
output is produced correctly; the panic is appended after it.

This is distinct from the existing head/pipe reports (which are about output
*reordering*/*summarization*): here the issue is an unhandled broken-pipe write
that produces a Rust panic + backtrace note.

## Observed

Command:

```
git diff pnpm-lock.yaml | head -40
```

Output (diff printed correctly for ~40 lines, then):

```
thread 'main' (1820333) panicked at /rustc/<hash>/library/std/src/io/stdio.rs:1165:9:
failed printing to stdout: Broken pipe (os error 32)
note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
```

## Expected

When stdout is closed by the downstream consumer, rtk should exit quietly
(conventional Unix behavior: reset SIGPIPE to default, or swallow `ErrorKind::BrokenPipe`
on stdout writes and exit 0). No Rust panic, no backtrace note.

## Workaround

- Avoid piping rtk output through `head`/`tail -n`; redirect to a file or read
  the whole output and slice in-tool.
- Or `export RTK_DISABLE=1` / use `rtk proxy <cmd>` for the specific invocation.

## Minimal reproduction

```
# any rtk-proxied command that emits more lines than head consumes
seq 1 100000 | rtk proxy cat | head -5      # if cat is proxied
# or, as observed:
git diff <large-file> | head -40
```

Expect: 5 (resp. 40) lines, clean exit. Actual: lines, then a Rust broken-pipe panic.

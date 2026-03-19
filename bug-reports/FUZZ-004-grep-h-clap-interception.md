# FUZZ-004: grep -h intercepted by Clap as help flag

**Severity**: MEDIUM
**Status**: FIXED (round 2 — 3553b33)
**Discovered by**: Agentic fuzzer, 2026-03-19
**Affected modules**: `src/main.rs` (Grep command definition)

## Summary

Running `rtk grep pattern . -h` showed RTK's help text instead of passing `-h` through to ripgrep. In GNU grep, `-h` means "suppress filename in output" (`--no-filename`). In rg, `-h` is rg's own help. Either way, Clap intercepted it before the flag reached the underlying tool.

## Reproduction

```bash
# Expected: rg interprets -h
rg 'fn ' src/ -h
# (rg's help output)

# Actual: Clap intercepted -h as RTK help
rtk grep 'fn ' src/ -h
# "Compact grep - strips whitespace, truncates, groups by file
#  Usage: rtk grep [OPTIONS] <PATTERN> [PATH] [EXTRA_ARGS]..."
```

## Root cause

Clap's default behavior reserves `-h` for `--help` on every subcommand. Since RTK's Grep subcommand didn't opt out, `-h` was consumed by Clap before reaching the `extra_args` trailing var arg.

## Fix

Added `disable_help_flag = true` to the Grep command definition and re-added `--help` as a long-only flag:

```rust
#[command(disable_help_flag = true)]
Grep {
    /// Print help (use --help; -h is reserved for rg's --no-filename)
    #[arg(long, action = clap::ArgAction::Help)]
    help: Option<bool>,
    // ...
}
```

## Impact

Subtle UX bug — users seeing help text would think they mistyped the command rather than understanding that Clap ate their flag. Particularly confusing for users migrating from GNU grep where `-h` is a commonly used flag.

## Heuristics triggered

- EXIT_CODE_MISMATCH (RTK exits 0 with help text, raw command exits differently)

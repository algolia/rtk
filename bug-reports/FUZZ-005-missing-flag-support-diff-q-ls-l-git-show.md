# FUZZ-005: Missing flag support — diff -q, ls -l, git branch --format/--sort

**Severity**: MEDIUM
**Status**: FIXED (rounds 1-3 — 9ece88c, e1e168e)
**Discovered by**: Agentic fuzzer, 2026-03-19
**Affected modules**: `src/main.rs`, `src/diff_cmd.rs`, `src/ls.rs`, `src/git.rs`

## Summary

Several RTK command definitions had Clap schemas narrower than the real tool's interface. Valid flags were rejected or filtered when they should have been accepted or passed through. This is a **bug class**, not an individual bug — the fuzzer found multiple instances of the same pattern.

## Instances

### 5a. `diff -q` — rejected by Clap

```bash
diff -q src/main.rs Cargo.toml      # "Files differ"
rtk diff -q src/main.rs Cargo.toml  # "error: unexpected argument '-q'"
```

**Root cause**: RTK's Diff command only defined `file1` and `file2` positional args, no `-q`/`--brief` flag.
**Fix**: Added `-q`/`--brief` flag mapped to `run_brief()` passthrough to system diff.

### 5b. `ls -l` — filtered when it should passthrough

```bash
ls -l           # detailed listing with permissions, sizes, dates
rtk ls -l       # RTK's tree-style output (lost all metadata)
```

**Root cause**: `ls` filter ran unconditionally regardless of flags.
**Fix**: Detect `-l` flag and passthrough to system ls.

### 5c. `git branch --format/--sort` — filtered custom output

```bash
git branch --format='%(refname:short) %(objectname:short)'
# feature/main a1b2c3d

rtk git branch --format='%(refname:short) %(objectname:short)'
# (RTK's condensed branch list, ignoring user's format)
```

**Root cause**: `run_branch()` didn't check for format-changing flags.
**Fix**: Detect `--format` and `--sort` flags and passthrough.

## Pattern

RTK's Clap schema was designed for the common case but didn't account for the long tail of valid flags. The fuzzer systematically explored this long tail by generating commands with format-changing flags.

**The fix pattern is always the same**: detect the flag, bypass the filter, passthrough raw output.

## Impact

Users encounter cryptic Clap errors or silently wrong output when using less-common but perfectly valid flags. Particularly frustrating because the raw command works fine — only the RTK proxy breaks.

## Heuristics triggered

- EXIT_CODE_MISMATCH (Clap rejection → exit 2, real tool → exit 0 or 1)
- DATA_LOSS (filter drops content from format-changed output)
- FORMAT_ALTERED (machine-readable flags present, output altered)

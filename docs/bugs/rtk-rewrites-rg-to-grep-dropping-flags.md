# RTK rewrites `rg` invocations to `grep`, dropping rg-only flags

**Date:** 2026-06-03
**Severity:** Medium (breaks ripgrep usage; silent semantic change of the command run)
**Component:** command rewrite / hook (input side, not output)
**rtk version:** 0.42.0-algolia.2

## Summary

When a `rg` (ripgrep) command is invoked, RTK appears to execute it as `grep`
instead. Any ripgrep-specific flags then hit GNU `grep`, which rejects them and
the command fails. This is distinct from the output-mangling bug — here the
*command itself* is substituted before execution.

## Observed

Three consecutive `rg` calls failed, each error coming from `/usr/bin/grep`:

```
$ rg -li "PATTERN" --type-add 'py:*.py' -g '!tests/' -l
/usr/bin/grep: unrecognized option '--type-add'

$ rg -li "PATTERN" -g '*.py' -g '!tests/**'
/usr/bin/grep: invalid option -- 'g'

$ command rg -li "PATTERN" --glob '*.py' --glob '!tests/**'
/usr/bin/grep: unrecognized option '--glob'
```

Note the third case: even `command rg` (which should bypass shell
aliases/functions) was still routed to `grep`, suggesting the rewrite happens in
the RTK hook layer matching the `rg` token, not a shell alias.

## Expected

`rg ...` should run ripgrep with its own flag set (`--glob`/`-g`, `--type-add`,
`--type`, etc.), or — if RTK intentionally proxies `rg` through `grep` for token
savings — it must translate ripgrep flags to grep equivalents rather than
passing them through verbatim. Silently downgrading `rg` to `grep` changes
semantics (recursion, gitignore awareness, glob handling) even when flags happen
to overlap.

## Workaround

Invoke the resolved binary by absolute path, which the hook does not rewrite:

```
RG=$(command -v rg); "$RG" -li "PATTERN" --glob '*.py' --glob '!tests/**'
```

This ran ripgrep correctly with full flag support.

## Repro (anonymized)

1. In any repo, run: `rg -li "sometoken" -g '*.py' -g '!tests/**'`
2. Observe failure from `/usr/bin/grep` about an invalid `-g`/`--glob` option.
3. Confirm bypass works: `RG=$(command -v rg); "$RG" -li "sometoken" --glob '*.py'`

---

## Additional occurrences — 2026-06-03 (rtk 0.42.0-algolia.2)

High-volume recurrence across four separate projects on the same day, confirming
the bug is still live. New flag variant observed: `--type py` (ripgrep's file-type
filter), previously not documented.

### New flag affected: `--type py` / `-t py`

```
$ rg -l "PATTERN" --type py
/usr/bin/grep: unrecognized option '--type'
Usage: grep [OPTION]... PATTERNS [FILE]...
Try 'grep --help' for more information.
```

After the `--type` failure, a follow-up attempt with the short form also failed:

```
$ rg -l -t py "PATTERN"
/usr/bin/grep: invalid option -- 't'
Usage: grep [OPTION]... PATTERNS [FILE]...
Try 'grep --help' for more information.
```

This short-flag failure is noteworthy: `-t` is a valid `grep` flag (`--text`, print
binary files as text), so RTK passed it through to `grep` unmodified, where it was
then rejected — the short form is not `--type`-equivalent in GNU grep. Both the long
form `--type py` and the short form `-t py` are broken.

### Workarounds tried

1. `rtk proxy rg -l "PATTERN" -tpy` — rejected by the user (rtk proxy invocation blocked)
2. `/usr/bin/rg -n "PATTERN" --type py` — **worked** (absolute path bypasses hook)
3. `grep -rl -E "PATTERN" dir --include='*.py'` — **worked** (native grep with --include)

### Occurrence count (2026-06-03)

| Project type (anonymized) | `--type` failures | `--glob`/`-g` failures |
|---|---|---|
| Python monorepo A | 10 | 16 |
| Python monorepo B (evals) | 4 | 12 |
| TypeScript SPA | 1 | 4 |
| Python service C (backburner) | 5 | 22 |
| Other | 0 | 2 |

The `--glob`/`-g` failures far outnumber `--type` failures; both are from the same
root cause (rg rewritten to grep, flags not translated).

---

## RESOLVED — 2026-06-03

**Actual root cause** (deeper than "flags not translated"): the hook correctly
rewrote `rg …` → `rtk grep …`, but `rtk grep`'s clap interface declared typed short
flags (`-l` = max-line-length, `-t` = file-type) that **collide with grep/rg's own
`-l`/`-t`**. Idiomatic `rg -l PATTERN …` (flags before the pattern) made clap bind
`-l` to max-len, consume the pattern as its non-numeric value, and **fail to parse**.
On any parse failure `main::run_fallback()` re-executes the raw args as an external
command — so `rtk grep -l foo --type py` ended up running literal `grep … --type py`,
which is exactly where `/usr/bin/grep: unrecognized option '--type'` came from. (The
same path explains the `-v`-leading variant of the EACCES/exit-127 bug: `run_fallback`
tried to exec `-v` as a binary.)

**Fix:** `rtk grep` no longer declares any typed flags. It captures the entire
argument vector verbatim (`trailing_var_arg + allow_hyphen_values`) and forwards it to
ripgrep unchanged, only regrouping/truncating the output. Clap can no longer fail on
grep/rg flag order, so the destructive fallback is never reached. Combined short
bundles like `-li` are recognized as format flags (file-list passthrough); the rg→grep
fallback (only when ripgrep is absent) now strips rg-only flags so grep won't choke.

Verified fixed: `rg -li PAT -g '*.py'`, `rg -l PAT --type py`, `rg -l -t py PAT`,
BRE `foo\|bar`, `--glob`, `-A`/context, `-c` count — all succeed via `rtk grep`.
See `src/cmds/system/grep_cmd.rs` (`locate_pattern`, `grep_safe_args`,
`token_has_format_flag`) and the de-typed `Grep` variant in `src/main.rs`.

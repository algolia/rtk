# RTK rewrites `rg` invocations to `grep`, dropping rg-only flags

> ✅ **RESOLVED in 0.42.0-algolia.3** (verified 2026-06-25 against a fresh `main` build).
> `rg` args are now forwarded to ripgrep verbatim (commit `73d8e14`); `rg -g/--glob/--type-add/-li`
> run as ripgrep, not GNU grep. The reporting session ran the installed `algolia.2` binary — a
> deploy gap, not an open code bug. Re-verified: `rtk rg -li "x" -g '*.py' .` searches correctly.

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

---

## REOPENED — additional case after the fix (2026-06-03 PM, rtk 0.42.0-algolia.2)

The de-typing fix above did not cover **`grep -E`** (GNU extended-regex flag).

**Observed:** `grep -nE '^    (async )?def ' file.py` failed with:

```
rg: error parsing flag -E: grep config error: unknown encoding: ^    (async )?def
```

i.e. `-E` was bound to ripgrep's `-E/--encoding`, and the **regex pattern was
consumed as the encoding value**. So `grep -E` (a no-op-ish "use ERE" request,
which is rg's default dialect anyway) is mis-rewritten and the command dies.

**Expected:** `-E` from a `grep` invocation should be recognized as
extended-regex (drop it for rg, since rg is ERE by default) — never mapped to
`--encoding`. Same root family as the resolved typed-short-flag collision; `-E`
just wasn't in the verified set.

**Workaround:** invoke `rg` directly without `-E` (rg is ERE natively), or
`rtk proxy grep -E …`.

### RESOLVED — 2026-06-04

`-E` belongs to the same hazard class as `-r`: a grep flag whose letter ripgrep
reuses for a **value-taking** flag (`-r`→`--replace`, `-E`→`--encoding`), so
forwarding it makes rg swallow the pattern. The recursive-stripping helper was
generalized (`strip_grep_recursive` → `strip_grep_only_flags`) to drop `-r`/`-R`
**and** `-E` (plus `--extended-regexp`), including from combined bundles
(`-nE`→`-n`, `-rE`→dropped) while preserving value-taking flags and their values.
rg is ERE by default, so dropping `-E` is safe.

Verified: `grep -nE 'def' x.py` returns matches (was "unknown encoding"). Guarded
by `test_strip_grep_only_extended_regex_E` (unit) and
`grep_extended_regex_ere_flag_is_handled` (behavioral, `tests/grep_flag_regression.rs`).

---

## Occurrence 2026-06-08 — reverse direction: `grep --include` rewritten to `rg`

**rtk version:** (current session)
**Component:** command rewrite / hook (input side)

Same root cause, opposite substitution: a GNU `grep` invocation was executed as
`rg`, so the grep-only `--include` glob flag hit ripgrep and was rejected.

### Observed
```
$ grep -rIli "black sesame" --include='*.jsonl' .
rg: unrecognized flag --include

similar flags that are available: --include-zero
```
Notably, an *earlier* `grep -rIli ... --include='*.jsonl'` call in the same
session (wrapped in a `for` loop) ran fine — so the rewrite is inconsistent:
the same `--include` flag survives in one invocation and is fatal in another.
This suggests the rewrite heuristic depends on surrounding command structure
(loop/compound vs. bare invocation), not just the binary name.

### Expected
Either keep `grep` as `grep` (so `--include` is valid), or translate
`--include='*.jsonl'` to the rg equivalent `-g '*.jsonl'` when substituting.

### Workaround
Rewrite by hand to rg-native syntax: `rg -li "PATTERN" -g '*.jsonl' .`

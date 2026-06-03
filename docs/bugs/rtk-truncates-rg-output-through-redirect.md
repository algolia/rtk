# RTK truncates `rg`/`grep` output to ~25 rows even through a file redirect

**Date:** 2026-06-03
**Severity:** High (silent data loss — dropped rows are invisible when output is captured, not viewed)
**Component:** output regrouping/truncation in `rtk grep`
**rtk version:** 0.42.0-algolia.2

## Summary

`rtk grep` (the rewrite target for both `grep` and `rg`) caps match output at
~25 rows and appends a `[+N more]` summary line. This is reasonable for an
interactive TTY (token saving), but the cap is **also applied when stdout is
redirected to a file or piped**, where the consumer expects the *complete*
result. The truncation marker is written into the file, so downstream tooling
silently sees a partial result with no signal that anything was dropped.

## Observed

A source file with 56 matching method definitions:

```
$ rg -n '^    (async )?def \w' case_generator.py --no-heading
56 matches in 1 files:

<...25 rows...>
[+31 more]
```

Redirecting to a file does **not** restore the full output:

```
$ rg -n '...' case_generator.py --no-heading > /tmp/methods.txt
$ wc -l /tmp/methods.txt
28            # header + 25 rows + "[+31 more]" — NOT 56
```

So 31 of 56 matches were dropped from a file the caller intended to be complete.

## Expected

When stdout is **not a TTY** (redirected to a file, piped to another process),
`rtk grep` should emit the **full, unsummarized** output. Truncation +
`[+N more]` is a presentation affordance for interactive reading only; it must
not alter captured data. (`isatty(stdout)` gate, or an explicit
`--no-truncate`/`RTK_NO_TRUNCATE` honored automatically for non-TTY.)

## Impact

This is more dangerous than the cosmetic identifier-mangling bug: an agent or
script that redirects search output to a file and processes it will operate on
a silently-truncated set, with the `[+N more]` line as the only (easily missed)
tell. In a code-navigation context it leads to "I found all N call sites" when
only the first 25 were seen.

## Workaround

`rtk proxy rg …` (bypasses the filter entirely) when complete output is needed,
or pipe through `tail -n +1` is **not** sufficient (truncation already happened
upstream) — must use `rtk proxy`.

## Minimal reproduction

```bash
# any file with >25 matches for the pattern
rtk proxy rg -c PATTERN file        # => real count, e.g. 56
rg PATTERN file > out.txt; wc -l out.txt   # => ~26 (capped + marker)
```

---

## RESOLVED — 2026-06-04 (regular-file gate)

A blanket "full output when non-TTY" gate was rejected: RTK's primary use is an agent
running a command whose stdout is a captured **pipe** (non-TTY) — that is exactly when
truncation must stay on to save tokens. The fix distinguishes a real redirect from a
captured pipe via `fstat`:

`stdout_is_regular_file()` (in `src/cmds/system/grep_cmd.rs`) returns true only when
stdout is a regular file (`S_ISREG`). In that case `rtk grep` emits the complete,
unregrouped rg output (no per-file/global caps, no `[+N more]`) — the same passthrough
used for format flags. TTYs and pipes keep the compact, capped view.

Net behavior:
- `grep PATTERN file > out.txt` → **all** matches, no marker (redirect = authoritative).
- `grep PATTERN file | …` and interactive TTY → compact, capped + `[+N more]` (token save).

Verified live (40-match fixture): redirect → 40 lines / 0 markers; pipe → 25 + `[+15 more]`.
Guarded by `grep_redirect_to_file_keeps_all_matches` in `tests/grep_flag_regression.rs`.

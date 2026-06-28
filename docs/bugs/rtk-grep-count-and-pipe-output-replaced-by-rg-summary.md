# RTK replaces `grep` line output with rg's "N matches in M files" summary, and prefixes `grep -c` counts with the filename

> ⚠️ **PARTLY RESOLVED in 0.42.0-algolia.4** (verified 2026-06-25 against a fresh `main` build).
> 1. **`grep -c FILE` prefixing the count with `file:`** → FIXED. rtk no longer forces `-n`/`-H` in
>    output-format mode (`-c`/`-l`/`-o`/...), so `grep -c file` returns a bare `3` like GNU grep.
> 2. **The misleading "0 matches" near-misread** → was the `algolia.2` deploy gap; on `algolia.3`+ the
>    counts are accurate. Re-verified.
> 3. **`grep PATTERN file | head` showing "N matches in M files:" instead of raw lines** → **BY DESIGN.**
>    That is rtk's core pipe-compaction (its whole reason to exist), and the counts are now correct. If you
>    need byte-faithful grep lines, redirect to a file (`> out.txt`, emitted verbatim) or use `rtk proxy grep`.
> 4. **`rg --files` emitting a bogus "N matches in 0 files" summary** → ✅ **FIXED (unreleased, commit
>    `cba316f`).** `--files` is a path-list format with no `file:line:content` to regroup; it is now
>    recognized as a format flag and passed through unmangled. Scoreboard row `rg --files src` (was
>    CORRUPT, now OK). Compacting the path list further is a possible future enhancement.

**Date:** 2026-06-25
**Severity:** High (silent data loss + near-misread: a real "8 matches" was shown as "0 matches", almost leading to a wrong "feature is broken" conclusion)
**Component:** output filtering / `grep` proxied through `rg` (output side)
**rtk version:** 0.42.0-algolia.2

## Summary

When `grep` is proxied through ripgrep, RTK rewrites the **output** in two ways that
corrupt programmatic log analysis:

1. `grep -c PATTERN FILE` (count mode) — RTK returns the count **prefixed with the
   filename** (`rserve.log:3`) instead of the bare integer (`3`) that `grep -c`
   prints. Worse, consecutive `grep -c` calls in a script got their outputs
   **merged onto one line**, so the labels and counts desynchronised.
2. `grep PATTERN FILE | head -N` (line mode through a pipe) — RTK returned rg's
   **match-count summary** (`1 matches in 0 files:`, `0 matches for 'fonts/'`)
   **in place of the actual matching lines**. The "0 files" is nonsensical for a
   single real file, and the summary count (`1`) even disagreed with the `grep -c`
   count (`2`) for the same pattern in the same file moments earlier.

## Observed (anonymized)

A script tallying access-log hits:

```
printf "style.json: "; grep -c 'style.json' rserve.log
printf "idf.pmtiles range hits: "; grep -c 'idf.pmtiles' rserve.log
printf "font pbf hits: "; grep -c 'fonts/' rserve.log
echo "=== sample lines ==="; grep 'idf.pmtiles' rserve.log | head -4
echo "=== font lines ==="; grep 'fonts/' rserve.log | head -3
```

RTK-filtered output actually returned:

```
style.json: rserve.log:3
idf.pmtiles range hits: rserve.log:2
font pbf hits: sprite hits: rserve.log:4      <- two -c outputs merged on one line
=== sample lines ===
1 matches in 0 files:                          <- actual matched lines REPLACED by summary
[+1 more]
=== font lines ===
0 matches for 'fonts/'                         <- summary, not lines; "0" misleading
```

The danger: I was using the log to decide whether a browser had fetched map tiles.
The mangled output read like "0 / 1 matches" (feature looks broken). Re-running the
identical tally through **`python3`** (which RTK does not filter) revealed the truth:
8 distinct `idf.pmtiles` ranges and 7 `fonts/` hits — the feature worked fine. The
RTK output would have led directly to a wrong "the basemap never loads" conclusion.

## Expected behavior

- `grep -c PATTERN FILE` → bare integer per invocation, one per line, no `FILE:`
  prefix, never merged across invocations.
- `grep PATTERN FILE | head -N` → the actual matching lines (up to N), verbatim —
  never replaced by an rg-style "N matches in M files" summary.

Read-only log/text inspection output must be byte-faithful to what stock `grep`
would print; the rg summary format is not a drop-in substitute for `grep`'s lines.

## Workaround

Analyse logs with a tool RTK does not proxy: `python3 -c "..."` (Counter over
`open(file)`), or `wc -l` on a pre-filtered temp file. Do **not** trust
`grep -c` / `grep | head` counts in a script when correctness matters.

## Reproduction (generic)

```
printf 'a\nb\na\n' > /tmp/t.txt
grep -c 'a' /tmp/t.txt            # stock: prints "2"; via hook: observed "/tmp/t.txt:2"
grep 'a' /tmp/t.txt | head -2     # stock: two lines "a","a"; via hook: rg summary
```

---

## Recurrence 2026-06-25 — `rg --files <dirs>` (file-LIST mode, no pipe) replaced by summary

Same summary-substitution, but on `rg --files` (list every file rg would search) with
**no pipe and no count flag**:

- **Command:** `rg --files app lib components src` (intent: list all source files)
- **Observed:** `55 matches in 0 files:` followed by `[+55 more]` — i.e. the file
  listing was replaced by an rg-style match-count summary. `--files` emits *paths*, not
  *matches*, so "55 matches in 0 files" is doubly nonsensical (there is no pattern and no
  match concept in `--files` mode).
- **Expected:** the newline-separated list of file paths, verbatim.
- **Workaround:** `find <dirs> -type f -name '*.ts' -o -name '*.tsx'` returned the real
  list (~51 files) immediately. `rg --files | sort` may also dodge it, untested.

This widens the trigger beyond `grep -c` / `grep | head`: the summary-replacement also
fires for `rg --files` with no pipe at all, so the heuristic deciding "this is a
count/summary context" is mis-firing on plain path-listing output.

# rtk condenses `head` output, reordering and fragmenting lines

> 🔴 **REOPENED — recurred on 0.42.0-algolia.4** (2026-06-26). The algolia.3 "not reproducible"
> note below is stale; the condenser is back on a different file.
> ✅ ~~**NOT REPRODUCIBLE on 0.42.0-algolia.3**~~ (verified 2026-06-25 against a fresh `main` build).
> `rtk head -8 CHANGELOG.md` returns the first 8 lines verbatim, in order. The original report ran the
> installed `algolia.2` binary; the same fixes that closed the grep/permission cluster appear to cover it.
> Reopen with the exact command + `rtk --version` if it recurs on `algolia.3`+.

## Recurrence — 2026-06-26, rtk 0.42.0-algolia.4

- **Command**: `head -4 CHANGELOG.md` (a different repo's CHANGELOG.md, ~130 lines, markdown).
- **Observed** (via Claude Code hook): line 1 `# Changelog`, line 2 blank, then line 3 was
  `  from indoor vs outdoor temperature and alerts only on a **mismatch** with the` — a fragment from
  deep in the file (a Sprint-3 entry), not line 3 (`## Sprint 4 — ...`), followed by `[128 more lines]`.
- **Expected**: the first 4 lines verbatim, in order.
- **Workaround**: re-read with the Read tool (range read) — returned the true first lines unmangled.
- So the condenser still triggers on a plain `head -N <file>` over a markdown file > a few-hundred lines,
  reordering + substituting deep-file fragments. Same root cause as the original report; not fixed in algolia.4.

- **Date**: 2026-06-11
- **Severity**: medium (silent output corruption — misleads the agent into believing a file is mangled when it is intact)
- **Affected component**: output filtering of `head -N <file>` (and compound `head && rg -c`)
- **rtk version**: (run `rtk --version` to confirm; observed via Claude Code hook proxying)

## Summary

Root cause class: **output-mangling**. When a `head -N <file>` command (chained with `rg -c`) runs through the rtk hook, the returned "output" is not the first N lines of the file. Instead rtk appears to apply a content condenser: it returns mid-file sentence fragments, out of order, followed by a `[2521 more lines]` marker — for a command whose entire purpose is positional (first N lines, verbatim).

## Observed

Command:

```bash
head -8 CHANGELOG.md && rg -c "^## " CHANGELOG.md
```

Returned output (anonymized):

```
# Changelog

All notable changes to the agentic-evals toolkit.

from the one-per-message `feedback` table, because eval + review flows need many judgments
  use hard-exits (code 2) with the migration path: `--app xxx` etc.
  import order (`discovery`), an f-string hoist that keeps the rendered string byte-identical
[2521 more lines]CHANGELOG.md:47
```

Lines 5-7 of the output are fragments from ~hundreds of lines deep in the file, not lines 5-7 of the file. The real lines 5-8 (`Format: [Keep a Changelog]...`, `---`, first `## ` header) were dropped. This led the agent to conclude a just-completed cherry-pick had corrupted the CHANGELOG; a follow-up `sed -n 1,40p` showed the file was intact.

## Expected

`head -N` output is positional and must be passed through verbatim (it is already tiny — 8 lines). Any summarization/condensation of `head`/`tail`/`sed -n` output defeats the command's purpose and produces actively false signal.

## Workaround

Re-read the same range with `sed -n 1,40p <file>` (passed through unmangled in the same session).

## Minimal repro

1. Take any file > ~2000 lines with markdown structure.
2. Through the rtk hook, run: `head -8 <file> && rg -c "^## " <file>`.
3. Compare with raw `head -8 <file>`: rtk's version interleaves deep-file fragments and appends a `[N more lines]` marker.

## Notes

Possibly the same condenser as `rtk-truncates-rg-output-through-redirect.md` / `rtk-mangles-grep-output-identifiers.md`, but the trigger here is a plain `head` on a file (no redirect, no grep), and the failure mode is reordering + fragment substitution, not just truncation.

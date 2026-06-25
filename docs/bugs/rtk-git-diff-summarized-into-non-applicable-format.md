# RTK rewrites `git diff` output into a lossy summary, destroying the unified diff

> ✅ **RESOLVED in 0.42.0-algolia.4** (verified 2026-06-25 against a fresh `main` build).
> `git diff` now passes through verbatim instead of the `--- Changes ---` / `+N -M` summary. Re-verified:
> the emitted text is a valid unified diff and `git apply --check` accepts it. `git show` keeps its own
> commit-display compaction; `--stat`/`--no-compact` are unaffected.

**Date:** 2026-06-25
**Severity:** High (the emitted text is NOT a valid patch -- `git apply` / `patch`
fail on it; the actual added/removed lines are replaced by a `+N -M` count, so
the change content is unrecoverable from the proxied output)
**Component:** output filtering -- `git diff` proxied and summarized
**rtk version:** (run `rtk --version`; observed in current Algolia build, 2026-06-25)

## Summary

`git diff -- <file>` run through the RTK hook does NOT return a unified diff. The
real hunk (with `@@ ... @@`, context lines, and `+`/`-` line content) is replaced
by an RTK summary format: a `--- Changes ---` header, the file name, an abbreviated
single-line preview of the change, and a `+1 -0` line/insertion-deletion count.

The result reads fine for a human glance but is **not a patch**: it cannot be fed
to `git apply`, `patch -p1`, or saved as a `.diff` for later application. Anyone
capturing `git diff > foo.diff` to ship a reproducible patch gets an unusable file.

## Observed

```
$ git diff -- path/to/File.java        # via rtk hook
 .../path/to/File.java       | 1 +
 1 file changed, 1 insertion(+)

--- Changes ---

path/to/File.java
  @@ -303,6 +303,7 @@ public class ... {
  +          <the one added line>

             // trailing context line
             ...
  +1 -0
```

The `--- Changes ---` / `+1 -0` framing is RTK's, not git's. The leading two-space
indent on the hunk body and the collapsed context make it non-parseable as a
unified diff.

## Expected

`git diff` must pass through verbatim (or be a byte-faithful unified diff). It is
already a compact, structured format; summarizing it is lossy and breaks the
primary downstream use (apply the patch elsewhere). At minimum, `git diff` should
be on the passthrough list alongside other format-sensitive git plumbing.

## Workaround

`rtk proxy git diff -- <file>` returns the true unified diff (proxy escape hatch
bypasses output filtering). Used that to regenerate a valid `.diff` after the
hook-mangled one failed to apply.

## Minimal reproduction

```bash
cd "$(mktemp -d)" && git init -q && printf 'a\nb\nc\n' > f.txt && git add f.txt && git commit -qm init
printf 'a\nb\nB2\nc\n' > f.txt
git diff -- f.txt > viahook.diff     # via rtk hook
git apply --check viahook.diff       # EXPECTED: clean; OBSERVED: fails (not a unified diff)
rtk proxy git diff -- f.txt > real.diff
git apply --check real.diff          # clean
```

## Suggested fix

Add `git diff` (and likely `git show`, `git format-patch`) to the output
passthrough allowlist. Diff/patch output is structurally meaningful and
machine-consumed; RTK's summarization is appropriate for verbose human-oriented
output, not for plumbing whose entire value is byte-exact applicability.

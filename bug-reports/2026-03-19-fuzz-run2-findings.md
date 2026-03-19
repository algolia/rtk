# RTK Agentic Fuzzing — Run 2 Results (2026-03-19)

**Method**: Static regression tests + LLM-generated commands (Qwen 3.5), 2 rounds × 10 families × 8 commands.
**Binary**: rtk 0.22.4 (with all Run 1 fixes applied)
**Stats**: 210 tests | 112 pass | 4 warn | 83 fail | 11 skip | **40% failure rate**

---

## Summary by Module

| Module | Tests | Pass | Fail | Warn | Key Issues |
|--------|-------|------|------|------|------------|
| git-log | 25 | 25 | 0 | 0 | **All fixed** |
| git-status | 4 | 4 | 0 | 0 | **All fixed** (porcelain markers now PASS) |
| git-diff | 21 | 19 | 2 | 0 | --name-only/--name-status format slightly altered |
| git-show | 12 | 9 | 3 | 0 | --name-only crashes (exit 128), --raw DATA_LOSS |
| git-branch | 21 | 4 | 17 | 0 | **CRITICAL**: --format, --sort cause massive expansion |
| grep | 25 | 7 | 18 | 0 | File ordering, -h Clap intercept, many format flags |
| ls | 21 | 10 | 11 | 0 | -la strips all metadata (permissions, sizes) |
| find | 15 | 3 | 12 | 0 | RTK find != system find (exit 2 on all) |
| diff | 22 | 3 | 19 | 0 | **CRITICAL**: exit code 1 swallowed, all modes broken |
| wc | 20 | 19 | 1 | 0 | -c byte count altered |
| tree | 20 | 16 | 0 | 4 | Minor line expansion warnings |
| gh-pr | 2 | 2 | 0 | 0 | **All fixed** |
| curl | 1 | 1 | 0 | 0 | OK |
| env | 1 | 1 | 0 | 0 | OK |

---

## NEW BUGS (not in Run 1)

### BUG 8: diff exit code 1 always swallowed (CRITICAL)

**Affected**: All diff commands when files differ (exit code 1)

`diff` returns exit 1 when files differ (not an error). RTK swallows this, returning 0. Also, `diff -q` with arguments RTK doesn't understand exits 2.

**Impact**: 19/22 diff tests fail. Scripts relying on `diff` exit codes break silently.

### BUG 9: find commands all fail with exit 2 (CRITICAL)

**Affected**: All `find` commands

RTK's `find` is a custom glob-based implementation, not a passthrough to system `find`. It doesn't accept standard find flags (`-maxdepth`, `-name`, `-type`, `-printf`). Every command fails with exit 2.

**Impact**: 12/15 find tests fail. RTK find is fundamentally incompatible with system find.

### BUG 10: git branch --format/--sort cause 10-15x line expansion (HIGH)

**Affected**: `git branch --format=...`, `git branch --sort=...`, `git branch -v`

RTK's branch filter outputs verbose info (86-94 lines) for what should be 6 lines. The filter doesn't detect format-changing flags and applies its own expansion.

**Impact**: 17/21 git-branch tests fail.

### BUG 11: ls -la strips all file metadata (HIGH)

**Affected**: `ls -la`, `ls -l`, `ls -lhS`, `ls -la --color=never`

RTK's ls filter strips permissions, ownership, sizes, dates — the exact info users request with `-l`. The filter assumes all ls output should be compacted to filenames-only, but `-l` users explicitly want metadata.

**Impact**: 11/21 ls tests fail. 100% anchor token loss on `-l` variants.

### BUG 12: git show --name-only exits 128 (MEDIUM)

**Affected**: `git show --name-only HEAD`, `git show --name-only -1`

RTK's git show handler doesn't recognize `--name-only` and fails with exit 128. The flag should trigger passthrough like git-log does.

**Impact**: 3/12 git-show tests fail.

### BUG 13: wc -c byte count altered (LOW)

**Affected**: `wc -c` (byte count mode)

RTK's wc filter reformats the byte count output, achieving only 45% similarity.

**Impact**: 1/20 wc tests fail. Minor formatting difference.

---

## Previously Fixed (verified passing)

| Bug | Module | Status |
|-----|--------|--------|
| gh --json passthrough | gh-pr | FIXED |
| grep -c Clap collision | grep | FIXED |
| grep -l/-m Clap collision | grep | FIXED |
| git log --format passthrough | git-log | FIXED |
| git log --stat/--graph/--patch | git-log | FIXED |

---

## Recommended Fix Priority

1. **diff exit code preservation** — exit 1 is not an error, must be preserved
2. **git branch --format passthrough** — same pattern as git-log fix
3. **ls -l metadata preservation** — -l is the most common ls flag
4. **find compatibility** — either passthrough to system find or document limitation
5. **git show --name-only passthrough** — same pattern as git-log fix
6. **grep ordering** — false positive from rg thread parallelism (heuristic issue, not RTK bug)

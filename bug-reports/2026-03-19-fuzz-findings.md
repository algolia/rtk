# RTK Agentic Fuzzing Results — 2026-03-19

**Method**: LLM-generated commands (Qwen 3.5 via Algolia inference proxy) targeting format-changing flags, comparing raw vs RTK output with 6 heuristic checks.

**Stats**: 64 tests | 34 pass | 30 fail | 0 warn | 47% failure rate

---

## BUG 1: grep flags crash with exit code 2 (CRITICAL)

**Affected flags**: `-l`, `--vimgrep`, `-A/-B/-C`, `--json`, `--no-filename`, `--count-matches`

**Symptom**: RTK exits with code 2 (rg error) while raw rg exits 0. These flags are passed through to rg via `extra_args` but conflict with RTK's hardcoded `-n --no-heading` flags, or RTK's Clap parser rejects them before they reach rg.

**Root cause**: RTK's grep command has structured params (`pattern`, `path`, `max_len`, etc.) that consume positional args before the `extra_args` trailing var arg. Flags like `-l` or `--vimgrep` may be consumed by Clap as unknown flags, or conflict with `-n`.

**Impact**: 7/16 grep tests fail with EXIT_CODE_MISMATCH.

**Commands that fail**:
- `rtk grep 'fn ' --count` (despite -c fix — `--count` long form works differently?)
- `rtk grep 'pub ' -l`
- `rtk grep 'fn ' --vimgrep`
- `rtk grep 'pub ' -A 2 -B 2`
- `rtk grep 'use ' --json`
- `rtk grep 'pub ' --no-filename --count-matches`

**Fix priority**: HIGH — these are common rg flags that users expect to work.

---

## BUG 2: grep -c passthrough incomplete (MEDIUM)

**Symptom**: `rtk grep 'fn ' -c` now works (from earlier fix) but output has LINE_EXPANSION (64 → 73 lines) and MARKER_INJECTION (📄, 🔍). Similarity to raw: 4%.

**Root cause**: The `-c` fix detects short flag but the long `--count` form triggers a different code path. Also, when running in the test repo (not /tmp file), the count output includes more files and RTK's formatting adds headers/markers.

**Fix priority**: MEDIUM — revisit count mode passthrough logic.

---

## BUG 3: git log --format is filtered when it shouldn't be (HIGH)

**Affected flags**: `--format='...'`, `--pretty=raw`, `--pretty=format:'...'`

**Symptom**: When user provides custom `--format`, RTK still applies its compact log filter, truncating lines and reformatting output. Similarity drops to 20-45%.

**Commands that fail**:
- `git log --format='%H %s %b' -10` (45% similarity)
- `git log --format='%H %s' -10` (36%)
- `git log --format='%H %ci' --abbrev-commit --decorate=no -10` (20%)
- `git log --format='%H' --name-only -10` (43%)
- `git log --pretty=raw -10` (38%)
- `git log --pretty=raw --numstat -10` (38%)

**Root cause**: `git.rs:run_log()` detects `--format`/`--pretty` to avoid injecting its own `--pretty`, but still applies the compact filter function to the output. Should passthrough when user specifies format.

**Fix priority**: HIGH — custom format strings are scripting staples.

---

## BUG 4: git log --graph output mangled (MEDIUM)

**Command**: `git log --oneline --graph --abbrev-commit -10`

**Symptom**: 33% similarity. RTK's compact log strips the graph characters (|, *, \, /) that make `--graph` useful.

**Fix priority**: MEDIUM — `--graph` is visual; stripping graph chars defeats its purpose.

---

## BUG 5: git log DATA_LOSS on stat/numstat/patch (MEDIUM)

**Affected flags**: `--stat`, `--numstat`, `--shortstat`, `--patch`

**Symptom**: RTK's compact log drops `commit`, `Author:`, `Date:` lines and reorganizes stat info. This is intentional compression for default `git log`, but when combined with `--stat`/`--numstat`/`--patch`, the user wants detailed file-level info.

**Commands that fail**:
- `git log --stat -10`
- `git log --numstat -10`
- `git log --shortstat -10`
- `git log --patch -10`
- `git log --stat --no-merges -10`

**Fix priority**: MEDIUM — RTK's compression is valid for basic `git log` but should detect these flags and adjust.

---

## BUG 6: git status --porcelain --long conflict (LOW)

**Command**: `git status --porcelain --long` (contradictory flags)

**Symptom**: 61% similarity. Git resolves the conflict (--long wins), but RTK applies its status filter which strips lines.

**Fix priority**: LOW — contradictory flags are an edge case, but RTK should still passthrough.

---

## BUG 7: git diff exit code swallowed (LOW)

**Command**: `git diff --porcelain=v2` (invalid flag)

**Symptom**: Raw git exits 129 (error), RTK exits 0. RTK swallows the error exit code.

**Fix priority**: LOW — invalid flag, but exit code preservation matters for scripts.

---

## Summary by Module

| Module | Tests | Pass | Fail | Key Issues |
|--------|-------|------|------|------------|
| git-log | 16 | 3 | 13 | --format passthrough, --graph mangling, stat data loss |
| git-status | 16 | 14 | 2 | --porcelain --long edge case |
| git-diff | 16 | 15 | 1 | Exit code swallowed |
| grep | 16 | 2 | 14 | Most rg flags crash (exit 2), count mode incomplete |

## Recommended Fix Priority

1. **grep flag passthrough** — most flags crash entirely (exit 2)
2. **git log --format passthrough** — custom formats are scripting fundamentals
3. **git log --graph preservation** — visual output destroyed
4. **git log stat flags** — adjust compression when stat flags present
5. **grep count mode** — complete the -c/--count fix

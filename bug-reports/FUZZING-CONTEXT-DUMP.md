# RTK Agentic Fuzzing — Context Dump

Raw notes for future talk/narrative. Unpolished brain dump.

## Timeline

- **2026-03-19**: Built fuzzer v1, ran rounds 1-2, found 5 bugs (FUZZ-001 to 005), fixed all
- **2026-03-20**: Extended fuzzer to v2 (6 new families, stderr heuristic), found 3 more bugs
- **2026-03-20**: Extended fuzzer to v3 (35 families, 139 tests), found 7 new bugs (FUZZ-006 to 012)
- Total: **15 bugs discovered**, 5 fixed, 10 open

## Numbers That Tell the Story

| Round | Tests | Families | Heuristics | Bugs Found | Failure Rate |
|-------|-------|----------|------------|------------|-------------|
| 1 | 64 | 17 | 6 | 5 | 47% → 20% (after fixes) |
| 2 | 85 | 23 | 7 | 3 | 17% → 8% (after fixes) |
| 3 | 139 | 35 | 7 | 7 | 29% (new families expose more) |

The **29% in round 3 is not regression** — we expanded into known-weak areas (docker, pip, npm). Previously-fixed families still pass.

## What the Fuzzer Finds (Bug Taxonomy)

### The Big Three Bug Classes

1. **Clap Schema Too Narrow** (FUZZ-001, 004, 005, 006, 007c, 009)
   - RTK defines specific args for a command
   - Real tool accepts way more flags
   - Clap rejects valid flags with exit code 2
   - Pattern: `EXIT_CODE_MISMATCH + DATA_LOSS`
   - Fix: `trailing_var_arg = true` or explicit flag definitions

2. **Format Override Without Escape Hatch** (FUZZ-002, 003, 007a)
   - RTK injects `--format=json` or `--message-format=json` internally
   - User's format flag is ignored or conflicts
   - Output parsed as JSON when it shouldn't be
   - Pattern: `FORMAT_ALTERED + DATA_LOSS`
   - Fix: detect user's format flag, skip injection, passthrough

3. **Wrong Command Routing** (FUZZ-008)
   - RTK maps user command to wrong underlying command
   - `npm list` → `npm run list` (wrong)
   - Pattern: `EXIT_CODE_MISMATCH + DATA_LOSS`
   - Fix: proper subcommand routing

### Secondary Classes

4. **Stderr Consumed by Filter** (FUZZ-010)
   - cargo clippy/test produce on stderr
   - RTK filter reads stderr, summarizes to stdout
   - Original stderr content lost
   - Pattern: `STDERR_LOSS`

5. **Clap Intercepting Tool Flags** (FUZZ-004)
   - `-h` intercepted as help by Clap
   - `--` separator eaten before reaching underlying tool
   - Pattern: `EXIT_CODE_MISMATCH`

6. **Filter Too Aggressive** (FUZZ-011, 012)
   - Branch filter strips remote refs
   - Diff filter strips too much context
   - Pattern: `DATA_LOSS` (debatable — intentional compression?)

## What Worked (methodology)

### The Fuzzer Architecture
```
GENERATE → EXECUTE (raw + rtk) → COMPARE (7 heuristics) → REPORT
```

This is simple but effective. The key insight: **you don't need to understand what the correct output is, you just need to compare raw vs. RTK and flag differences.**

### Static Tests > LLM-Generated Tests
- Static tests are deterministic, fast, and catch known patterns
- LLM rounds add diversity but most bugs were found by static tests
- Best approach: static tests for known risks, LLM rounds for exploration

### The 7 Heuristics
1. **JSON_MANGLED** — catches format override bugs instantly
2. **EXIT_CODE_MISMATCH** — catches Clap rejection (exit 2) and routing errors (exit 1)
3. **DATA_LOSS** — anchor token sampling catches content disappearing
4. **FORMAT_ALTERED** — high-similarity check catches format overrides
5. **STDERR_LOSS** — catches the cargo clippy class of bugs
6. **LINE_EXPANSION** — RTK should compress, not expand
7. **MARKER_INJECTION** — RTK emoji markers in wrong places

Most valuable: EXIT_CODE_MISMATCH (28 hits), DATA_LOSS (29 hits)
Least useful: LINE_EXPANSION, MARKER_INJECTION (mostly warnings, not failures)

### Source Mining for Test Cases
- Upstream issues (rtk-ai/rtk) → found `--` separator bug class
- `rtk discover` → found which commands are most used
- Module source code audit → found format injection patterns in pip/go/ruff/vitest/pytest
- All three sources yielded unique bugs not found by the other two

### RTK_MAP for Multi-Command Families
The mapping table for commands where raw ≠ rtk (e.g., `cat` → `rtk read`, `rg` → `rtk grep`) was essential for testing cross-command families like `separator` and `empty-output`.

## What Didn't Work / Limitations

### False Positives Required Tuning
- grep results come in random order (threads) → sort before compare
- wc intentionally compresses output → skip format check for wc
- Large diff output legitimately loses data by design → gray area

### LLM Generation Quality
- MiniMax M2.5 sometimes generates destructive commands despite prompt
- Blocklist catches them, but wastes API calls
- Sometimes generates commands with nonexistent flags
- Better prompt engineering could improve yield

### Missing Coverage
These RTK modules have ZERO fuzzer coverage:
- `vitest`, `tsc`, `next`, `lint`, `prettier`, `playwright`, `prisma` (need JS/TS project)
- `pytest` (need Python test suite)
- `kubectl` (cluster timeout)
- `ruff` (need Python project with lint issues)
- `pnpm` (need Node project)
- `go test/build/vet` (need Go project)

Could be addressed with mock fixtures or docker-based test environments.

### The "Intentional Compression" Debate
Some failures are arguably by design:
- `diff` losing line range markers — that's the point of "ultra-condensed diff"
- `git branch -a` filtering remotes — might be intentional noise reduction
- `rg --no-heading` large output losing lines — truncation is a feature

Need to classify each as bug vs. feature before fixing.

## Architecture Insights (for the talk)

### RTK's Fundamental Tension
RTK wants to be a transparent proxy AND an intelligent filter. These goals conflict:
- **Transparent proxy**: pass everything through unchanged
- **Intelligent filter**: modify output to save tokens
- When the filter doesn't understand the output format → mangling

### The "Long Tail of Flags" Problem
Every CLI tool has hundreds of flags. RTK's Clap schema was designed for the common case (5-10 flags per command). The fuzzer systematically explores the long tail:
- `docker ps` has ~15 flags, RTK supports 0
- `pip list` has ~10 flags, RTK supports 2
- `find` has ~50 predicates, RTK supports 0

### The Fix Pattern Is Always the Same
```
detect format-changing flag → bypass filter → passthrough raw output
```

This could be a **global fix**: for ANY command, if unknown flags are present, default to passthrough rather than filtering. Trade token savings for correctness.

## Key Quotes / Insights for Narrative

- "The fuzzer doesn't understand correct output. It just notices when RTK and the real tool disagree."
- "47% failure rate on first run. That's almost half of all commands broken."
- "Every bug follows the same pattern: RTK assumed one output format, the user asked for another."
- "Static tests found more bugs than LLM generation. But LLM found the ones we didn't think to test."
- "The fix is always the same three lines: detect the flag, bypass the filter, passthrough."

## Tools & Infrastructure

- **Fuzzer**: Python, single script (`scripts/fuzz-rtk.py`), ~600 lines
- **LLM**: MiniMax M2.5 via Algolia inference proxy (OIDC vault auth)
- **Test repo**: Shallow clone of rtk itself into /tmp
- **Safety**: Blocklist for destructive commands, 30s timeout, no shell=True
- **Output**: JSON report with full raw/rtk comparison data

## Open Questions

1. Should RTK default to passthrough for unknown flags? (global fix vs. per-command fix)
2. How to handle the "intentional compression" gray area? (configurable strictness?)
3. Is there a way to auto-generate Clap schemas from tool --help output?
4. Should the fuzzer run in CI? (regression detection)
5. How to test modules that need specific project types (JS, Python, Go)?

## LLM Round Results (targeting weak families)

Ran 2 LLM rounds × 8 commands per family on docker-ps, pip, npm, find:
- **94% failure rate** (75/80 executed)
- Confirms docker/pip/npm/find are comprehensively broken, not just edge cases
- LLM-generated tests found additional variants:
  - `pip list --outdated --format=json` → JSON_MANGLED
  - `npm outdated --json` → JSON_MANGLED (npm run outdated doesn't exist)
  - `npm view --json name` → JSON_MANGLED (npm run view doesn't exist)
  - `find . -printf '%h\n'` → EXIT_CODE_MISMATCH (printf not a Clap arg)
  - `pip list --user --format=columns` → EXIT_CODE_MISMATCH + FORMAT_ALTERED
  - `pip list --quiet --name-only` → new flag combo not in static tests
- The LLM is good at generating flag combinations we didn't think of
- But 94% fail rate means these families are so broken that even basic commands fail

## Files

```
scripts/fuzz-rtk.py          — the fuzzer (v3)
bug-reports/FUZZ-001-*.md    — individual bug reports (1-5 fixed, 6-12 open)
bug-reports/FUZZ-ROUND3-SUMMARY.md — round 3 detailed results
/tmp/fuzz-round3.json        — full JSON report with raw/rtk output data
```

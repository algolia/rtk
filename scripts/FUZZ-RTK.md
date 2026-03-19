# RTK Agentic Fuzzer

LLM-powered fuzzing system that discovers bugs in RTK's command output filters by generating diverse command invocations with format-changing flags.

## How It Works

```
[GENERATE]  →  [EXECUTE]  →  [COMPARE]  →  [REPORT]
 Qwen 3.5      raw + rtk     6 heuristics    JSON
```

1. **Generate**: Asks an LLM (Qwen 3.5 via Algolia inference proxy) to generate commands with format-changing flags
2. **Execute**: Runs each command raw AND through RTK
3. **Compare**: 6 heuristic checks (JSON integrity, line expansion, emoji injection, exit codes, data loss, format preservation)
4. **Report**: Structured JSON with PASS/WARN/FAIL verdicts

Also includes **static regression tests** for known edge cases that run without LLM calls.

## Usage

```bash
# Quick: static tests only (no LLM needed)
python scripts/fuzz-rtk.py --rounds 0 --family git-log,grep --use-cwd

# Standard: 3 rounds of LLM-generated tests across all families
python scripts/fuzz-rtk.py --rounds 3 --per-round 10 --use-cwd

# Targeted: specific families
python scripts/fuzz-rtk.py --family git-log,grep,ls --rounds 2

# Save report
python scripts/fuzz-rtk.py --output /tmp/fuzz-report.json

# Dry run: generate commands but don't execute
python scripts/fuzz-rtk.py --dry-run
```

## Requirements

- Python 3.11+
- `requests` (`pip install requests`)
- RTK installed (`rtk --version`)
- Vault access for LLM API token (`vault read --field=token identity/oidc/token/enablers`)

## Command Families

| Family | Tool | Focus |
|--------|------|-------|
| git-log | git log | --format, --pretty, --stat, --graph, --raw |
| git-status | git status | --porcelain, --short |
| git-diff | git diff | --stat, --name-only, --raw |
| git-show | git show | --format, --no-patch |
| git-branch | git branch | --format, -v, --sort |
| grep | rg | -c, -l, --json, --vimgrep, -A/-B/-C |
| gh-pr | gh pr | --json, --template, --jq |
| gh-run | gh run | --json, --log |
| cargo-build | cargo build | --message-format |
| cargo-test | cargo test | --message-format, -q |
| cargo-clippy | cargo clippy | --message-format |
| ls | ls | -l, -1, -a, -R |
| tree | tree | -L, -d, -f |
| find | find | -name, -type, -printf |
| cat | cat/read | -n, -b |
| curl | curl | -s, -v, -I |
| wc | wc | -l, -w, -c |
| env | env | (no flags) |
| diff | diff | -u, -y, -q |

## Results (2026-03-19)

First run: **64 tests, 30 failures (47% failure rate)**

Bugs found and fixed:
- `gh --json` passthrough (JSON output was reformatted into lossy summary)
- `grep -c` passthrough (count format misinterpreted by line parser)
- grep Clap flag collisions (-l, -c, -m collided with rg flags)
- git log --format passthrough (custom format output mangled by compact filter)
- git log --stat/--graph/--patch passthrough (detail flags need raw output)

## 6 Comparison Heuristics

1. **JSON Integrity**: raw is valid JSON -> RTK must also be valid JSON
2. **Line Expansion**: RTK should compress, not expand (flag if RTK > raw + 3 lines)
3. **Emoji/Marker Injection**: RTK markers in output when raw has none
4. **Exit Code Match**: must be identical
5. **Data Loss**: sample anchor tokens from raw, check >=50% appear in RTK
6. **Format Preservation**: when machine-readable flags present, similarity must be >=90%

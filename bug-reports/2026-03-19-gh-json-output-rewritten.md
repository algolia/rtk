# RTK rewrites `gh` JSON output into human-readable format

**Date**: 2026-03-19

**Command**:
```bash
gh run list --workflow="Claude AI PR Review" --limit 5 --json databaseId,conclusion,createdAt,headBranch
```

**Expected output** (raw JSON array):
```json
[{"conclusion":"success","createdAt":"2025-11-25T09:47:59Z","databaseId":19665221905,"headBranch":"feature/pr-review-skill-ci"}, ...]
```

**Actual RTK output**:
```
🏃 Workflow Runs
  ✅ Claude AI PR Review [19665221905]
  ✅ Claude AI PR Review [19466389770]
  ✅ Claude AI PR Review [19463012443]
```

**Impact**: Any downstream JSON parsing (`python3 -c "json.load(sys.stdin)"`, `jq`, etc.) fails with `JSONDecodeError`. The `--json` flag explicitly requests structured output, but RTK intercepts and reformats it into a lossy human-readable summary that drops most fields.

Also affects `gh pr list --json`, `gh api` (paginated), and any `gh` subcommand with `--json`.

**Workaround**: `command gh` bypasses RTK, but this is fragile and easy to forget. Also doesn't work inside `subprocess.run(['gh', ...])` in Python scripts.

**Suggested fix**: RTK should pass through output verbatim when `--json` flag is present in `gh` commands, or when stdout is being piped (not a TTY).

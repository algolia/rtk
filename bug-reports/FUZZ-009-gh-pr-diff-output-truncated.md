# FUZZ-009: gh pr diff — Output Truncated by RTK Renderer

**Severity**: MEDIUM
**Status**: OPEN
**Discovered by**: Live agentic usage (pr-review skill smoke test), 2026-03-20
**Affected modules**: `src/gh_cmd.rs` (or equivalent gh output renderer)

## Summary

`gh pr diff` output is visually truncated when passed through RTK. The raw patch data is silently cut, leaving the consuming agent with an incomplete diff. The agent noted it was "visually truncated by the RTK renderer" and had to fall back to `gh pr diff --jq` / `gh pr view --json` to retrieve sufficient data.

## Reproduction

```bash
# Raw: full unified diff, potentially thousands of lines
gh pr diff 1074 --repo algolia/conversational-ai

# Via RTK: diff truncated — only first N lines rendered, rest silently dropped
rtk gh pr diff 1074 --repo algolia/conversational-ai

# Workaround that worked reliably:
gh pr view 1074 --repo algolia/conversational-ai --json files,additions,deletions,body
```

## Observed behavior

- The agent (running inside a subagent task) called `gh pr diff` and received a truncated response
- No error, no warning — output just ends mid-diff
- Retry with `--jq` field extraction on `gh pr view --json` worked consistently
- `gh pr view` (metadata only) was unaffected

## Root cause hypothesis

RTK likely applies a line-count or byte-count cap when rendering `gh` output, similar to how `head -n` would truncate. For large diffs (PR #1074 was ~400 lines of patch), the renderer hits the cap and stops without signaling truncation to the caller.

Alternatively, RTK may be transforming `gh pr diff` into a `gh pr view --patch` variant that has different output limits.

## Impact

- Agents doing PR review receive incomplete diffs → miss findings in later files
- Silent truncation: no exit code change, no stderr warning — agent has no signal to retry
- Workaround exists (`--json` + field extraction) but requires the agent to know about it

## Heuristics triggered

- DATA_LOSS (partial — tail of diff silently dropped)
- No EXIT_CODE_MISMATCH (exit 0 in both cases)
- No FORMAT_ALTERED (the portion received is correct unified diff format)

## Workaround

Use `gh pr view --json files` for file list, then read individual file diffs via `gh api` or `git diff` against the PR branch. Alternatively `gh pr diff | head -n 9999` is explicitly blocked by RTK's SIGPIPE rules — use `run_in_background` + `Read` on output file instead.

## Context

Found during smoke test of `pr-review` skill migration (algolia/conversational-ai PR #1076). The review agent completed successfully using the `--json` workaround but flagged this as a pipeline friction point worth fixing.

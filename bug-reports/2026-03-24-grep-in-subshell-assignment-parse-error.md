# RTK Bug: `grep` in subshell variable assignment causes parse error

**Date**: 2026-03-24
**Severity**: HIGH — causes silent data loss / command failure
**Category**: Command rewrite destroys valid shell syntax

---

## Symptom

When a bash command contains `grep` inside a subshell variable assignment (i.e. `VAR=$(grep ...)`), RTK rewrites it into an `rtk grep` invocation that breaks the surrounding shell syntax, producing:

```
(eval):N: parse error near `APP_ID=$(grep '
```

Exit code 1, command never runs.

## Reproducer

```bash
APP_ID=$(grep '^SW_APP_ID=' .env | cut -d'=' -f2)
API_KEY=$(grep '^SW_API_KEY=' .env | cut -d'=' -f2)
echo "$APP_ID"
```

RTK rewrites the `grep` calls but leaves the `$(...)` subshell wrapper mangled, resulting in a shell parse error.

## Root Cause (hypothesis)

RTK intercepts `grep` at the token level and rewrites to `rtk grep <pattern> <path>`, but does not account for:
1. The enclosing `VAR=$(...)` subshell context
2. The `| cut` pipe that follows — `rtk grep` output format differs from raw grep output

The rewritten command is likely something like:
```bash
APP_ID=$(rtk grep '^SW_APP_ID=' .env | cut -d'=' -f2)
```
which may still fail if `rtk grep` output format (with line numbers, headings) doesn't match what `cut` expects, OR the rewrite itself is syntactically broken.

## Workaround

Use `Grep` tool (dedicated) for file searches, or isolate the grep into a separate Bash call before the multi-line command block. Or use `python3 -c` to read the file entirely without grep.

## Impact

Any multi-line bash block where grep is used inline for credential/config extraction fails silently. Particularly painful when the error appears at line N > 1, making it hard to spot which line caused the parse failure.

## Expected Behavior

RTK should either:
- Skip rewriting `grep` that appears inside `$(...)` subshell assignments, OR
- Rewrite correctly preserving pipe chains, OR
- Detect the context and pass through as-is

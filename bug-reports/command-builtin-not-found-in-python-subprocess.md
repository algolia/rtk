# `command gh` not available in Python subprocess

## Summary
`command` is a shell builtin, so `subprocess.run(["command", "gh", ...])` fails with `Permission denied` / `FileNotFoundError`. This matters because CLAUDE.md tells us to use `command gh` to bypass RTK interception of `gh --json` output.

## Context
- RTK intercepts `gh` commands run from Claude's Bash tool
- The workaround `command gh` works in Bash but **not** in Python's `subprocess.run()`
- Python scripts that call `gh api` via subprocess don't need `command` because RTK only hooks the Claude Bash tool, not arbitrary child processes
- But the CLAUDE.md instruction "use `command gh` to bypass" creates a trap: you reflexively use it everywhere

## Reproduction
```python
import subprocess
# This fails: [Errno 13] Permission denied: 'command'
subprocess.run(["command", "gh", "api", "/repos/..."], capture_output=True)

# This works fine (RTK doesn't intercept Python subprocess):
subprocess.run(["gh", "api", "/repos/..."], capture_output=True)
```

## Suggested fix
Either:
1. Document that `command gh` is only needed in Claude Bash tool, not Python subprocess
2. Or make RTK not intercept `gh api` calls (only `gh --json` patterns that it compresses)

## Impact
Low — workaround is obvious once you know. But it cost ~2 minutes of confusion per occurrence.

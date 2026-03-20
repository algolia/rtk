# FUZZ-008: npm — Hardcoded to `npm run`

**Severity**: MEDIUM
**Status**: OPEN
**Discovered by**: Agentic fuzzer round 3, 2026-03-20
**Affected modules**: `src/npm_cmd.rs`

## Summary

RTK's npm handler hardcodes all input as `npm run <args>`. Any npm subcommand other than `run` fails because RTK prepends `run` to whatever the user typed.

## Reproduction

```bash
npm list --depth=0              # lists dependencies (exit 0)
rtk npm list --depth=0          # "npm ERR! Missing script: list" (exit 1)

npm list --depth=0 --json       # valid JSON dependency tree
rtk npm list --depth=0 --json   # "npm ERR!" — JSON_MANGLED

npm config list                 # shows npm config (exit 0)
rtk npm config list             # "npm ERR! Missing script: config" (exit 1)
```

## Root cause

`npm_cmd.rs` constructs the command as:
```
npm run <user_args>
```

So `rtk npm list` becomes `npm run list`, which looks for a script named "list" in package.json. Since no such script exists, npm errors.

## Fix pattern

Detect the first argument as a subcommand. Route known subcommands (`list`, `outdated`, `view`, `config`, `install`) appropriately instead of always prepending `run`.

## Impact

3 of 139 fuzzer tests fail. `npm list` and `npm config` are commonly used commands. JSON output from `npm list --json` is consumed by CI pipelines and dependency auditing tools.

## Heuristics triggered

- EXIT_CODE_MISMATCH (raw=0, rtk=1)
- DATA_LOSS (100% — no output)
- JSON_MANGLED (npm list --json: valid JSON in, error text out)
- FORMAT_ALTERED (6% similarity)

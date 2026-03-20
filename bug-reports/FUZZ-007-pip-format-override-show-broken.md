# FUZZ-007: pip — Format override + show/list broken

**Severity**: HIGH
**Status**: OPEN
**Discovered by**: Agentic fuzzer round 3, 2026-03-20
**Affected modules**: `src/pip_cmd.rs`

## Summary

Three distinct issues in pip handling:

1. `pip list` forces `--format=json` internally, rejecting user's `--format=columns|freeze`
2. `pip show <package>` returns exit 1 instead of 0
3. `pip list --not-required` rejected by Clap

## Reproduction

### 7a. Format override
```bash
pip list --format=freeze        # absl-py==2.3.1  (exit 0)
rtk pip list --format=freeze    # "error: unexpected argument" (exit 2)

pip list --format=columns       # Package  Version  (exit 0)
rtk pip list --format=columns   # "error: unexpected argument" (exit 2)
```

### 7b. pip show broken
```bash
pip show requests               # Name: requests\nVersion: 2.32.3  (exit 0)
rtk pip show requests           # (nothing or error)  (exit 1)

pip show pip                    # Name: pip\nVersion: 25.0.1  (exit 0)
rtk pip show pip                # (exit 1)
```

### 7c. --not-required rejected
```bash
pip list --not-required         # lists packages not depended on (exit 0)
rtk pip list --not-required     # "error: unexpected argument" (exit 2)
```

## Root cause

- **7a**: `run_list()` always appends `--format=json` to args. If user also passes `--format=X`, pip receives both and the second one (json) wins — but Clap rejects the user's flag before it reaches pip.
- **7b**: `pip show` is routed through `run_passthrough()` but something in the argument handling loses the package name or routes incorrectly.
- **7c**: `--not-required` not in Clap schema; no trailing_var_arg to accept unknown flags.

## Fix pattern

- Detect user's `--format` flag and skip injecting `--format=json` (passthrough if non-json format)
- Add `trailing_var_arg = true` for list subcommand to accept unknown flags
- Debug `pip show` routing to find where package name is lost

## Impact

7 of 139 fuzzer tests fail. pip is widely used in Python development. Format freeze output is standard for requirements.txt generation — breaking it disrupts Python workflows.

## Heuristics triggered

- EXIT_CODE_MISMATCH (exit 2 for Clap rejection, exit 1 for show)
- DATA_LOSS (100% content loss on rejected commands)
- FORMAT_ALTERED (3% similarity when format overridden)
- STDERR_LOSS (pip list --outdated stderr warnings lost)

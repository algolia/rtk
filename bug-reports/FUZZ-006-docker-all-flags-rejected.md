# FUZZ-006: Docker — All flags rejected by Clap

**Severity**: HIGH
**Status**: OPEN
**Discovered by**: Agentic fuzzer round 3, 2026-03-20
**Affected modules**: `src/container.rs` (DockerCommands::Ps, DockerCommands::Images)

## Summary

RTK's Docker Clap schema defines fixed-shape commands with zero extra arguments. Every flag beyond the bare command (`-a`, `-q`, `--format`, `--no-trunc`, `--filter`, `--digests`) causes Clap rejection with exit code 2.

## Reproduction

```bash
# All of these work raw:
docker ps -a
docker ps -q
docker ps --format '{{.Names}} {{.Status}}'
docker ps --no-trunc
docker ps -a --filter status=running
docker ps --format json
docker images -q
docker images --format '{{.Repository}}:{{.Tag}}'
docker images --no-trunc
docker images --digests
docker images --format json

# All of these fail through RTK:
rtk docker ps -a                # "error: unexpected argument '-a'"  (exit 2)
rtk docker ps --format json     # "error: unexpected argument '--format'" (exit 2)
rtk docker images -q            # "error: unexpected argument '-q'" (exit 2)
```

## Root cause

`DockerCommands::Ps` and `DockerCommands::Images` in `src/container.rs` don't have `#[arg(trailing_var_arg = true, allow_hyphen_values = true)]` or any extra flag definitions. The Clap enum variants are completely fixed-shape.

Only bare `docker ps` and `docker images` work. Any flag triggers Clap rejection.

## Fix pattern

Add `trailing_var_arg = true` + `allow_hyphen_values = true` to the Ps and Images variants, OR detect format-changing flags and passthrough. Same pattern as PR #5 fix for git args.

## Impact

12 of 139 fuzzer tests fail — the single largest failure cluster. Docker users can only use the most basic commands through RTK. All customization is blocked.

## Heuristics triggered

- EXIT_CODE_MISMATCH (raw=0, rtk=2 across all tests)
- DATA_LOSS (100% data loss — no output produced)

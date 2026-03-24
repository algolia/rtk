# Bug: `docker compose -f` flag not forwarded correctly

## Observed behavior
`rtk docker compose -f deploy/docker-compose.yml build` fails with:
```
error: unexpected argument '-f' found

Usage: rtk docker compose [OPTIONS] <COMMAND>
```

## Expected behavior
The `-f` flag should be forwarded to `docker compose` as-is, since it's a valid `docker compose` option (specify compose file path).

## Reproduction
```bash
# This fails through rtk:
docker compose -f deploy/docker-compose.yml build

# This works directly:
/usr/bin/docker compose -f deploy/docker-compose.yml build
```

## Context
Working directory: `/home/pln/Work/Perso/Apps/PerfectWedding`
The compose file is at `deploy/docker-compose.yml` (not in default location).

## Root cause hypothesis
RTK's argument parser for `docker compose` is consuming `-f` as its own option rather than forwarding it to the underlying `docker compose` command. The `-f` flag likely needs to be treated as a passthrough argument.

## Date
2026-03-22

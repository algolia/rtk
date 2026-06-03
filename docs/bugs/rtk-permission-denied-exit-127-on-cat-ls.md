# RTK crashes with `[rtk: Permission denied (os error 13)]` on `cat` / `ls` / `head`

**Date:** 2026-06-03
**Severity:** High (command silently fails; exit 127 returned; caller must fall back to a different tool)
**Component:** RTK hook / internal file-system access
**rtk version:** 0.42.0-algolia.2

## Summary

On certain `cat`, `ls`, and `head` invocations RTK emits:

```
[rtk: Permission denied (os error 13)]
```

and the shell reports **exit 127** (command not found), meaning the underlying
command **never ran**. RTK is hitting an internal `EACCES` (os error 13) —
likely while trying to read or write its own cache/tracking files — and aborting
before launching the child process.

The files being read are world-readable; the issue is not a file-permission
problem on the target files, but an internal RTK operation that fails before
exec.

## Observed

### Trigger 1 — multi-file `cat`

```
$ cat src/store/persistence.ts src/store/resourcesSlice.ts src/engine/tick.ts
Exit code 127
[rtk: Permission denied (os error 13)]
```

Files exist and are `-rw-rw-r--`; the user has read access.

### Trigger 2 — `ls` combined with `head` in a chain

```
$ ls docs/ && echo "---" && head -3 docs/A.md docs/B.md docs/C.md docs/D.md 2>&1
Exit code 127
GDD.md  20.4K
...
---
[rtk: Permission denied (os error 13)]
```

Partial output before the error suggests RTK managed to start processing but
crashed mid-stream.

### Trigger 3 — `cat` piped to `head`

```
$ cat tsconfig.json vite.config.ts 2>&1 | head -80
[rtk: Permission denied (os error 13)]
```

No exit code banner this time; the error appears inline and the pipe produces no
output.

## Frequency

9 confirmed occurrences in one TypeScript SPA project across two separate
sessions. Each occurrence forced the caller to recover via the editor Read tool
(which bypasses the shell hook entirely).

Also observed once in the RTK project itself during internal testing of the
`rtk hook claude` path:

```
$ echo '{"tool_name":"Bash","tool_input":{"command":"git status"}}' \
    | /home/pln/.local/bin/rtk hook claude 2>&1 | head
[rtk] WARNING: untrusted project filters (.rtk/filters.toml)
[rtk] Filters NOT applied. Run `rtk trust` to review and enable.
[rtk: Permission denied (os error 13)]
(exit=0)
```

Here the `exit=0` from the surrounding wrapper masked the error, but the hook
itself failed with the same os error 13.

## Expected behavior

RTK should execute `cat`/`ls`/`head` normally. Internal RTK operations (cache
writes, tracking updates) must not abort the child-process launch; at worst they
should log a warning and continue.

## Workaround

Fall back to the editor **Read** tool for single-file content; it bypasses the
shell hook entirely and is unaffected. For directory listing, no clean workaround
exists via shell — a separate `find` command sometimes works if that path avoids
the trigger.

## Suspected root cause

RTK attempts to write a tracking or cache entry (e.g. in its local `.rtk/`
directory or in a global state file) before or after running the command. When
that write hits a permissions boundary (e.g. `.rtk/` is owned by a different user
or the directory does not exist and cannot be created), RTK propagates the
`EACCES` as a fatal error rather than degrading gracefully. The trigger is
intermittent, which is consistent with a race or a lazy-init path that only runs
when the tracking file is absent.

## Repro (anonymized)

1. In a project directory where the `.rtk/` state directory is missing or
   not writable by the current user:
   ```
   cat file1.ts file2.ts file3.ts
   ```
2. Observe `Exit code 127` + `[rtk: Permission denied (os error 13)]`.
3. Confirm files are readable: `ls -la file1.ts file2.ts` (no permissions issue).
4. Retry after `mkdir -p .rtk && chmod 755 .rtk` or after `rtk trust` — check
   whether the error disappears.

---

## RESOLVED — 2026-06-03

**Actual root cause** (not the `.rtk/` cache-write the report guessed): the
`[rtk: …]` text comes only from `main::run_fallback()`, reached when clap fails to
parse the invocation. The fallback then tries to **spawn `args[0]` as a binary**. When
that name is not a runnable binary on PATH — a shell builtin like `read` (which is
what `cat …` rewrites to via `rtk read` when *its* clap parse fails on a flag-like
arg), a typo, or a non-executable file — the spawn is doomed. On PATH layouts that
contain an unsearchable directory, `execvp`'s scan returns **EACCES (os error 13)**
rather than ENOENT, and rtk surfaced that raw: `[rtk: Permission denied (os error 13)]`,
exit 127 — which reads as a file-permission failure on the user's files. It never was.

Reproduced deterministically (any machine): `rtk zzznosuchcmd foo`,
`rtk read -a.ts` (clap-fails → falls back to the `read` builtin).

**Fix:** `run_fallback` now resolves `args[0]` up front. If it is not a runnable
binary, rtk skips the doomed spawn and reports honestly:
- missing command → `rtk: <name>: command not found`
- existing non-executable file → `rtk: <name>: not executable (permission denied)`

Both still exit 127 (correct for not-found), but the message no longer implicates
file permissions. The `-v`-leading variant noted in
[rtk-rewrites-rg-to-grep-dropping-flags.md](./rtk-rewrites-rg-to-grep-dropping-flags.md)
is also gone — that one was the same clap-fail→fallback chain, now fixed at the grep
layer too. See `unrunnable_command_message()` in `src/main.rs`.

**Residual (follow-up, not a crash):** when `cat …` is rewritten to `rtk read …` and
read's clap parse fails, run_fallback only sees the rewritten name (`read`), so the
message names `read`, not the original `cat`. Honest but slightly indirect. A fuller
fix would make rewrite targets clap-tolerant (as done for grep) or preserve the
original command for the fallback path.

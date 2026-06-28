# rtk find rejects `-exec` / compound predicates instead of passing through

- **Date:** 2026-06-27
- **Severity:** low (graceful refusal with guidance, but blocks a valid command)
- **Affected component:** `rtk find` command rewrite (hook intercepts bare `find`)
- **rtk --version:** (not captured at incident time; reproduce with current build)

## Summary (root cause)

The Claude Code hook rewrites a bare `find` invocation to `rtk find`. When the
command uses `-exec` (or other compound predicates / actions like `-not`), `rtk find`
refuses with:

```
rtk: rtk find does not support compound predicates or actions (e.g. -not, -exec). Use `find` directly.
```

This is a command-rewrite interception: a valid GNU `find` command that would have
run fine is blocked because the rewrite layer can't model `-exec`/compound predicates.
The user must then work around it (here: fell back to the editor's file-read tool).

## Observed

Two separate invocations, both rejected (output to stdout, exit non-zero enough to
abort the intended read):

```
find /path/to/base -name '*.py' -not -path '*__pycache__*' -exec echo "--- {} ---" \; -exec cat {} \;
  -> rtk: rtk find does not support compound predicates or actions (e.g. -not, -exec). Use `find` directly.

find /path/to/app -type f -not -path '*__pycache__*' -exec echo "--- {} ---" \; -exec cat {} \;
  -> rtk: rtk find does not support compound predicates or actions (e.g. -not, -exec). Use `find` directly.
```

## Expected

When `rtk find` encounters predicates/actions it does not optimize (`-exec`, `-not`,
`-o`, etc.), it should **transparently pass the command through to the system `find`**
(proxy mode) rather than refuse. The whole point of the rewrite is to be invisible;
refusing turns a 0-token convenience into a hard stop the caller has to route around.

## Workaround

- Run `find` via a path that the hook does not rewrite, or
- Avoid `-exec`: read files with the editor's dedicated read tool instead of
  `find ... -exec cat`.

## Minimal reproduction

```
mkdir -p /tmp/rtkrepro/sub && printf 'x\n' > /tmp/rtkrepro/a.py && printf 'y\n' > /tmp/rtkrepro/sub/b.py
find /tmp/rtkrepro -name '*.py' -not -path '*sub*' -exec echo "--- {} ---" \; -exec cat {} \;
# observed: rtk refusal. expected: lists a.py with its contents.
```

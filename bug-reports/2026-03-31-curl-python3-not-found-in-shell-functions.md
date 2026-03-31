# RTK Bug: `curl` and `python3` not found inside shell function bodies

**Date**: 2026-03-31
**Severity**: HIGH — completely breaks multi-step API scripts
**Category**: Command rewrite breaks function definitions

---

## Symptom

When a bash command defines a shell function that uses `curl` and pipes to `python3`, both commands fail with `command not found` despite being present at `/usr/bin/curl` and `/usr/bin/python3`.

```
create_link:2: command not found: curl
create_link:11: command not found: python3
```

## Reproduction

```bash
create_link() {
  local url="$1"
  curl -s -X POST "https://api.short.io/links" \
    -H "Content-Type: application/json" \
    -H "Authorization: $API_KEY" \
    -d '{"originalURL": "'$url'"}' | python3 -c "import sys,json; print(json.load(sys.stdin))"
}
create_link "https://example.com"
```

## Expected

`curl` and `python3` execute normally inside the function body.

## Actual

RTK hook rewrites `curl` and/or `python3` inside the function definition, producing invalid command references that fail at invocation time.

## Workaround

Use `/usr/bin/curl` and `/usr/bin/python3` absolute paths, or use a dedicated CLI tool instead of curl.

## Context

Trying to create short.io links via API. Both `which curl` and `which python3` confirm they exist at `/usr/bin/`.

## Resolution (v0.34.2-algolia.1)

**Root cause**: Two issues compounding:
1. `curl` piped to `python3`/`jq` was rewritten to `rtk curl`, which auto-compresses JSON output — breaking downstream pipe consumers that expect raw JSON.
2. Shell function definitions containing rewritable commands could theoretically be corrupted (though current parser already skipped most function forms via the `$((` / compound detection).

**Fix**:
- Added `curl`/`wget` to the pipe-incompatible list in `rewrite_compound()` — they are not rewritten when piped.
- Added explicit shell function definition detection (`() {`, `function `) in `rewrite_command()` — bail early on function bodies.
- 8 new tests covering function definitions, curl pipe skipping, and compound edge cases.

**Status**: FIXED

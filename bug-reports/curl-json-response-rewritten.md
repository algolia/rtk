# Bug: `curl` JSON API responses rewritten into schema-like format

## Observed behavior
`curl -sf http://localhost:8042/api/content/themes` returns what looks like a schema/type description instead of actual JSON data:

```
[{
    description: string,
    emoji: string,
    id: string,
    subthemes:
    [{
        depth: int,
        id: string,
        question_count: int,
        title: string
      }] (5)
    title: string
  }] (3)
```

## Expected behavior
Should return the raw JSON response from the API, which is a valid JSON array with actual values (strings, numbers, etc).

## Reproduction
```bash
# Through rtk (broken - returns schema):
curl -sf http://localhost:8042/api/content/themes

# Direct (works - returns actual JSON):
/usr/bin/curl -sf http://localhost:8042/api/content/themes
```

## Impact
- Piping to `python3 -m json.tool` or `jq` fails with JSON parse errors
- Any downstream tool expecting valid JSON will break
- The actual data values are lost, making the response useless for debugging

## Root cause hypothesis
RTK's curl proxy is summarizing/compacting JSON responses by replacing actual values with their types (e.g., `"Notre mariage"` → `string`). This is too aggressive for API debugging where you need to see actual data.

## Date
2026-03-22

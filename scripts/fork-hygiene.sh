#!/usr/bin/env bash
#
# fork-hygiene.sh — Algolia fork hygiene gate + auto-fixer.
#
# After every upstream catchup (and before every release), upstream identity
# and telemetry references leak back in. This script catches all known leak
# classes and can auto-fix the deterministic ones.
#
# Usage:
#   scripts/fork-hygiene.sh            # CHECK only (exit 1 on any leak) — use as a gate
#   scripts/fork-hygiene.sh --fix      # apply deterministic fixes, then CHECK
#
# What --fix does NOT touch (intentional — manual judgment required):
#   - LICENSE copyright + CONTRIBUTING CLA grantee (legal; fork retains attribution)
#   - code-comment provenance referencing upstream issue numbers (accurate history)
#   - CHANGELOG.md historical telemetry entries (history)
#   - src/filters/*.toml (filter *data* about brew/etc., not install instructions)
#
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"

REPO="algolia/rtk"
SLACK='#proj-internal-skills (Slack)'
CARGO_INSTALL="cargo install --git https://github.com/${REPO}"
FIX=0
[[ "${1:-}" == "--fix" ]] && FIX=1

# Files where the patterns are *documented* (rules) or are legitimate data/history.
# Excluded from the leak scan so the gate isn't self-tripped.
# --hidden so .github/, .rtk/ etc. are scanned (ripgrep skips dotdirs by default);
# !.git keeps the object store out.
GATE_EXCLUDES=(--hidden --glob '!.git' --glob '!CLAUDE.md' --glob '!scripts/fork-hygiene.sh' --glob '!CONTRIBUTING.md' --glob '!LICENSE' --glob '!CHANGELOG.md' --glob '!hooks/pi/README.md')
# Shared excludes for --fix replacements (never rewrite the rules doc, the script, or legal text)
FIX_EXCLUDES=(--hidden --glob '!.git' --glob '!Cargo.lock' --glob '!CLAUDE.md' --glob '!scripts/fork-hygiene.sh' --glob '!CONTRIBUTING.md' --glob '!LICENSE')

if [[ $FIX -eq 1 ]]; then
  echo "==> Applying deterministic fork-hygiene fixes"

  # 1. Repo slug (everywhere except the lockfile).
  #    NOTE: code-comment provenance should reference "upstream #<n>" (no slug),
  #    not "rtk-ai/rtk#<n>" — keep history meaning without re-introducing the slug.
  rg -l "${FIX_EXCLUDES[@]}" 'rtk-ai/rtk' | while read -r f; do
    sed -i 's#rtk-ai/rtk#'"${REPO}"'#g' "$f"
  done

  # 2. Website + star-history + link text
  rg -l "${FIX_EXCLUDES[@]}" 'rtk-ai\.app|rtk-ai%2Frtk' | while read -r f; do
    sed -i \
      -e 's#https://www\.rtk-ai\.app/guide#https://github.com/'"${REPO}"'/tree/main/docs/guide#g' \
      -e 's#https://www\.rtk-ai\.app#https://github.com/'"${REPO}"'#g' \
      -e 's#rtk-ai\.app/guide#'"${REPO}"' docs#g' \
      -e 's#rtk-ai%2Frtk#algolia%2Frtk#g' \
      -e 's#(rtk-ai\.app)#('"${REPO}"')#g' \
      "$f"
  done

  # 3. Emails -> Slack ('|' delimiter: replacement contains '#')
  rg -l "${FIX_EXCLUDES[@]}" 'contact@rtk-ai\.app|security@rtk-ai\.app' | while read -r f; do
    sed -i -e "s|contact@rtk-ai\.app|${SLACK}|g" -e "s|security@rtk-ai\.app|${SLACK}|g" "$f"
  done

  # 4. Homebrew install/uninstall -> cargo (user-facing docs only; not filter data)
  for f in README.md README_es.md README_fr.md README_ja.md README_ko.md README_zh.md \
           openclaw/README.md INSTALL.md docs/guide/getting-started/installation.md Formula/rtk.rb; do
    [[ -f "$f" ]] || continue
    sed -i \
      -e "s|brew tap rtk-ai/tap && brew install rtk|${CARGO_INSTALL}|g" \
      -e "s|brew install rtk-ai/tap/rtk|${CARGO_INSTALL}|g" \
      -e "s|brew install rtk|${CARGO_INSTALL}|g" \
      -e "s|brew uninstall rtk|cargo uninstall rtk|g" \
      "$f"
  done
  echo "    done. Review with: git diff"
fi

echo "==> Running fork-hygiene CHECK"
fail=0

leak() { # <label> <ripgrep-args...>
  local label="$1"; shift
  local hits
  if hits=$(rg -n "$@" "${GATE_EXCLUDES[@]}" 2>/dev/null); then
    echo "::error:: ${label}:"; echo "$hits" | sed 's/^/    /'; fail=1
  fi
}

leak "upstream repo slug (rtk-ai/rtk)"        'rtk-ai/rtk' --glob '!Cargo.lock'
leak "upstream website (rtk-ai.app)"          'rtk-ai\.app'
leak "upstream contact email"                 'contact@rtk-ai\.app|security@rtk-ai\.app'
leak "Homebrew install (no fork tap)"         'brew (install|uninstall|tap) rtk' --glob '!src/filters/**'
leak "hardcoded stale version string"         '"rtk 0\.\d+\.\d+"' --glob '*.md' --glob '*.rb'
leak "telemetry residue in docs"              'telemetry' --glob '*.md' -i
leak "telemetry residue in source"            'telemetry|maybe_ping|ureq' --glob 'src/**/*.rs' -i
leak "dead link to deleted telemetry doc"     'TELEMETRY\.md|resources/telemetry' --glob '*.md'

if [[ $fail -eq 0 ]]; then
  echo "    ✓ clean — no upstream/telemetry leaks"
else
  echo ""
  echo "Hygiene check FAILED. Run 'scripts/fork-hygiene.sh --fix' for the deterministic ones,"
  echo "then handle any telemetry/legal/provenance items by hand (see header)."
  exit 1
fi

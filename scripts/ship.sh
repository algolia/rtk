#!/usr/bin/env bash
#
# ship.sh — Algolia fork release driver (single source of truth).
#
# Encodes the WHOLE fork release so no step is forgotten — most importantly the
# manual `gh workflow run release.yml`, which the CD workflow does NOT do on this
# fork (release-please is disabled) and which v0.42.0-algolia.2 skipped, leaving a
# tag with no published binaries.
#
# This is a fork-owned artifact: on every upstream refork it must be restored
# alongside fork-hygiene.sh (it does not exist on upstream). See CLAUDE.md
# "Upstream Catchup Procedure".
#
# Usage:
#   scripts/ship.sh <X.Y.Z-algolia.N> [--prerelease] [--no-dispatch] [--dry-run]
#
# Example:
#   scripts/ship.sh 0.42.0-algolia.4
#
# Steps: gate (fmt/clippy/test) → fork-hygiene → version bump (Cargo.toml+lock)
#        → guard CHANGELOG entry + manifest pin → commit → tag → push → dispatch
#        the Release workflow for the tag.
set -euo pipefail

die() { echo "✗ $*" >&2; exit 1; }
note() { echo "▶ $*"; }

VERSION="${1:-}"
PRERELEASE="false"
DISPATCH="true"
DRY_RUN="false"
shift || true
for arg in "$@"; do
  case "$arg" in
    --prerelease) PRERELEASE="true" ;;
    --no-dispatch) DISPATCH="false" ;;
    --dry-run) DRY_RUN="true" ;;
    *) die "unknown flag: $arg" ;;
  esac
done

[ -n "$VERSION" ] || die "usage: scripts/ship.sh <X.Y.Z-algolia.N> [--prerelease] [--no-dispatch] [--dry-run]"
# Fork version scheme: X.Y.Z-algolia.N (NOT plain semver).
echo "$VERSION" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+-algolia\.[0-9]+$' \
  || die "version '$VERSION' must match X.Y.Z-algolia.N (fork scheme)"
TAG="v$VERSION"
BASE="${VERSION%-algolia.*}"   # upstream base, e.g. 0.42.0

cd "$(git rev-parse --show-toplevel)"

note "1/8 Quality gate (fmt, clippy, test)"
cargo fmt --all --check
cargo clippy --all-targets
cargo test --all

note "2/8 Fork hygiene"
scripts/fork-hygiene.sh

note "3/8 Guard CHANGELOG + release-please manifest"
grep -q "\[$VERSION\]" CHANGELOG.md \
  || die "CHANGELOG.md has no '[$VERSION]' entry — add it by hand (release-please is OFF on the fork)"
MANIFEST_VER="$(grep -oE '"[0-9]+\.[0-9]+\.[0-9]+"' .release-please-manifest.json | tr -d '"' | head -1)"
[ "$MANIFEST_VER" = "$BASE" ] \
  || die ".release-please-manifest.json is '$MANIFEST_VER', expected upstream base '$BASE' (do NOT bump it)"

note "4/8 Bump Cargo.toml -> $VERSION (GNU sed) + refresh lockfile"
if [ "$DRY_RUN" = "true" ]; then
  echo "  (dry-run) would set version = \"$VERSION\""
else
  sed -i "s/^version = .*/version = \"$VERSION\"/" Cargo.toml
  cargo update -p rtk
fi

note "5/8 Verify built version"
if [ "$DRY_RUN" != "true" ]; then
  cargo build --release
  target/release/rtk --version | grep -q "$VERSION" || die "binary version mismatch"
fi

note "6/8 Commit release (NO AI-fingerprint trailers)"
if [ "$DRY_RUN" = "true" ]; then
  echo "  (dry-run) would commit chore(release): $VERSION"
else
  git add Cargo.toml Cargo.lock CHANGELOG.md
  git commit -m "chore(release): $VERSION"
fi

note "7/8 Tag $TAG + push branch + tag"
BRANCH="$(git branch --show-current)"
if [ "$DRY_RUN" = "true" ]; then
  echo "  (dry-run) would tag $TAG and push $BRANCH + tag"
else
  git tag -a "$TAG" -m "Release $TAG"
  git push origin "$BRANCH"
  git push origin "$TAG"
fi

note "8/8 Dispatch Release workflow (builds 5-platform assets + GH release)"
if [ "$DISPATCH" != "true" ]; then
  echo "  skipped (--no-dispatch). Run manually:"
  echo "    gh workflow run release.yml -f tag=$TAG -f prerelease=$PRERELEASE"
elif [ "$DRY_RUN" = "true" ]; then
  echo "  (dry-run) would: gh workflow run release.yml -f tag=$TAG -f prerelease=$PRERELEASE"
else
  gh workflow run release.yml -f tag="$TAG" -f prerelease="$PRERELEASE"
  echo "  dispatched. Monitor: gh run list --workflow=release.yml --limit 3"
fi

echo "✓ ship complete: $TAG"

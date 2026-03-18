#!/bin/bash
# Auto-format and lint Rust code before commits
# Hook: PreToolUse for git commit (Claude Code)
# Also installed as .git/hooks/pre-commit for native git

set -e

echo "🦀 Running Rust pre-commit checks..."

# Find cargo (support both rustup and system installs)
if [ -f "$HOME/.cargo/env" ]; then
    # shellcheck source=/dev/null
    source "$HOME/.cargo/env"
fi

if ! command -v cargo &>/dev/null; then
    echo "⚠️  cargo not found — skipping pre-commit checks"
    exit 0
fi

# Auto-fix formatting
cargo fmt --all

# Stage any fmt changes so they're included in the commit
git add -u

# Strict clippy — matches CI exactly
if ! cargo clippy --all-targets -- -D warnings 2>&1; then
    echo "❌ Clippy -D warnings failed. Fix errors before committing."
    exit 1
fi

echo "✅ Pre-commit checks passed"

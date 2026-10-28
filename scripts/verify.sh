#!/bin/bash
# TideTerm 三阶段快捷检查（非完整 CI/发布门禁，范围见 docs/wiki/Development.md）
# 用法: bash scripts/verify.sh
set -euo pipefail

PASS=0
FAIL=0

if command -v cargo >/dev/null 2>&1; then
  CARGO_BIN="cargo"
elif [ -n "${USERPROFILE:-}" ] && [ -x "${USERPROFILE}/.cargo/bin/cargo.exe" ]; then
  CARGO_BIN="${USERPROFILE}/.cargo/bin/cargo.exe"
elif [ -x "${HOME}/.cargo/bin/cargo" ]; then
  CARGO_BIN="${HOME}/.cargo/bin/cargo"
elif command -v where.exe >/dev/null 2>&1; then
  CARGO_BIN="$(where.exe cargo | tr -d '\r' | head -n 1)"
  if command -v cygpath >/dev/null 2>&1; then
    CARGO_BIN="$(cygpath -u "$CARGO_BIN")"
  else
    drive="${CARGO_BIN:0:1}"
    rest="${CARGO_BIN:2}"
    CARGO_BIN="/mnt/${drive,,}${rest//\\//}"
  fi
else
  CARGO_BIN="cargo"
fi

case "$(uname -s 2>/dev/null || echo unknown)" in
  MINGW*|MSYS*|CYGWIN*) IS_WINDOWS_BASH=1 ;;
  *) IS_WINDOWS_BASH=0 ;;
esac

case "$CARGO_BIN" in
  *.exe) USE_WINDOWS_RUST_CHECK=1 ;;
  *) USE_WINDOWS_RUST_CHECK=$IS_WINDOWS_BASH ;;
esac

echo "═══════════════════════════════════════════"
echo "  TideTerm 三阶段检查"
echo "═══════════════════════════════════════════"

# ─────────────────────────────────────────────
# Stage 1: 文件大小门禁
# ─────────────────────────────────────────────
echo ""
echo "▶ Stage 1/3: 文件大小门禁 (check-file-size.sh)"
if bash scripts/check-file-size.sh 2>&1; then
  echo "✅ 文件大小门禁通过"
  PASS=$((PASS + 1))
else
  echo "❌ 文件大小门禁失败"
  FAIL=$((FAIL + 1))
fi

# ─────────────────────────────────────────────
# Stage 2: tiderm-core Rust 测试 + Clippy
# ─────────────────────────────────────────────
echo ""
echo "▶ Stage 2/3: tiderm-core Rust 测试 + Clippy"
if (cd crates/tiderm-core && "$CARGO_BIN" test && "$CARGO_BIN" clippy --all-targets -- -D warnings 2>&1); then
  echo "✅ tiderm-core Rust 测试 + Clippy 通过"
  PASS=$((PASS + 1))
else
  echo "❌ tiderm-core Rust 测试或 Clippy 失败"
  FAIL=$((FAIL + 1))
fi

# ─────────────────────────────────────────────
# Stage 3: Qt 壳（native）Rust 测试 + Clippy
# ─────────────────────────────────────────────
echo ""
echo "▶ Stage 3/3: Qt 壳 (native) Rust 测试 + Clippy"
if (cd native && "$CARGO_BIN" test --bin native && "$CARGO_BIN" clippy --all-targets -- -D warnings 2>&1); then
  echo "✅ Qt 壳 Rust 测试 + Clippy 通过"
  PASS=$((PASS + 1))
else
  echo "❌ Qt 壳 Rust 测试或 Clippy 失败"
  FAIL=$((FAIL + 1))
fi

# ─────────────────────────────────────────────
# Summary
# ─────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════"
echo "  三阶段检查结果: ${PASS}/3 通过"
echo "═══════════════════════════════════════════"

if [ "$FAIL" -gt 0 ]; then
  echo "❌ ${FAIL} 项失败 — 请修复后重新验证"
  exit 1
else
  echo "✅ 三阶段检查通过 — 继续核实变更所需的其余检查与人类审查"
  exit 0
fi

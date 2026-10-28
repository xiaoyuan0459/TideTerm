#!/bin/bash
# 文件大小门禁 — 防止代码膨胀
#
# 本脚本定义实际阈值、豁免和冻结清单；规则文档只解释语义。
# 对超标文件进行硬阻断。已标记技术债的文件冻结在当前行数，
# 不得继续增长——只能通过重构减少。
#
# 用法:
#   bash scripts/check-file-size.sh          # 检查所有文件，违规 exit 1
#   bash scripts/check-file-size.sh --ci     # 同上（参数为兼容既有文档保留，脚本本就有违规即 exit 1）
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"

# ═══════════════════════════════════════════════════════════
# 限制定义（唯一信源；变更语义时同步 docs/wiki/Development.md）
# ═══════════════════════════════════════════════════════════

# 硬限制
RUST_SOURCE_LIMIT=500       # crates + native/src + tools
CPP_SOURCE_LIMIT=500        # native/cpp（ADR-012 Phase B：C++ 门禁对齐 Rust）

# 豁免模式（glob 匹配）
EXEMPT_PATTERNS=(
    "native/src/rust/shared/theme_db/*.rs"  # 生成文件: sync_themes.py 输出; rustfmt 展开数据常量后超过硬限
)

# 已标记技术债文件 — 冻结在当前行数
# 格式: "文件相对路径:最大允许行数"
# 更新规则: 重构使行数下降后，手动降低此值
declare -A TECH_DEBT_FREEZE=(
)

# ═══════════════════════════════════════════════════════════
# 辅助函数
# ═══════════════════════════════════════════════════════════

VIOLATIONS=0

# 判断文件是否匹配豁免模式
is_exempt() {
    local file="$1"
    for pattern in "${EXEMPT_PATTERNS[@]}"; do
        # 使用 bash 通配符匹配
        if [[ "$file" == $pattern ]]; then
            return 0
        fi
    done
    return 1
}

# 检查单个文件
check_file() {
    local file="$1"
    local lines="$2"

    # 豁免检查
    if is_exempt "$file"; then
        echo "  ⏭  $file ($lines 行) — 豁免"
        return 0
    fi

    # 技术债冻结检查
    if [[ -n "${TECH_DEBT_FREEZE[$file]:-}" ]]; then
        local max_lines="${TECH_DEBT_FREEZE[$file]}"
        if [ "$lines" -gt "$max_lines" ]; then
            echo "  ❌ $file ($lines 行) — 冻结上限 $max_lines 行，已超 $(($lines - $max_lines)) 行！"
            echo "     此文件已标记为技术债，行数只许减少不许增加。"
            echo "     请将新增逻辑提取到独立文件后重新提交。"
            VIOLATIONS=$((VIOLATIONS + 1))
            return 1
        fi
        echo "  ⚠️  $file ($lines 行) — 技术债冻结中 (上限 $max_lines 行)"
        return 0
    fi

    # Rust 源文件硬限制
    if [[ "$file" == native/src/*.rs ]] || [[ "$file" == crates/*/*.rs ]] || [[ "$file" == tools/*/*.rs ]]; then
        if [ "$lines" -gt "$RUST_SOURCE_LIMIT" ]; then
            echo "  ❌ $file ($lines 行) — 超过 Rust 硬限制 $RUST_SOURCE_LIMIT 行！"
            echo "     请按职责拆分模块，不得自行放宽阈值或增加豁免。"
            VIOLATIONS=$((VIOLATIONS + 1))
            return 1
        fi
        return 0
    fi

    # C++ 源文件硬限制（native/cpp，ADR-012 Phase B）
    if [[ "$file" == native/cpp/*.cpp ]] || [[ "$file" == native/cpp/*.h ]] || [[ "$file" == native/cpp/*/*.cpp ]] || [[ "$file" == native/cpp/*/*.h ]]; then
        if [ "$lines" -gt "$CPP_SOURCE_LIMIT" ]; then
            echo "  ❌ $file ($lines 行) — 超过 C++ 硬限制 $CPP_SOURCE_LIMIT 行！"
            echo "     请按职责拆分模块，不得自行放宽阈值或增加豁免。"
            VIOLATIONS=$((VIOLATIONS + 1))
            return 1
        fi
        return 0
    fi

    # 其他文件不检查
    echo "  ·  $file ($lines 行)"
}

# ═══════════════════════════════════════════════════════════
# 主流程
# ═══════════════════════════════════════════════════════════

echo ""
echo "═══════════════════════════════════════════"
echo "  文件大小门禁 (File Size Gate)"
echo "═══════════════════════════════════════════"
echo ""

# 收集所有源文件
echo "▶ 扫描源文件..."

# Rust 文件
while IFS= read -r -d '' file; do
    rel="${file#$ROOT_DIR/}"
    case "$rel" in
        target/*|vendor/*) continue ;;
    esac
    lines=$(wc -l < "$file")
    check_file "$rel" "$lines" || true
done < <(find "$ROOT_DIR/crates" "$ROOT_DIR/native/src" "$ROOT_DIR/tools" -type d -name target -prune -o -type f -name '*.rs' -print0 2>/dev/null)

# C++ 文件（native，ADR-012 Phase B）
while IFS= read -r -d '' file; do
    rel="${file#$ROOT_DIR/}"
    case "$rel" in
        target/*|vendor/*) continue ;;
    esac
    lines=$(wc -l < "$file")
    check_file "$rel" "$lines" || true
done < <(find "$ROOT_DIR/native/cpp" \( -name '*.cpp' -o -name '*.h' \) -print0 2>/dev/null)

echo ""
echo "═══════════════════════════════════════════"

if [ "$VIOLATIONS" -gt 0 ]; then
    echo "  ❌ ${VIOLATIONS} 个文件超过大小限制"
    echo ""
    echo "  修复方法:"
    echo "  1. 将新增逻辑提取到独立文件/模块"
    echo "  2. 存量冻结只减不增，不以新增冻结/豁免掩盖违规"
    echo "     规则或生成数据例外的调整需说明依据并经审查"
    echo "═══════════════════════════════════════════"
    exit 1
else
    echo "  ✅ 所有文件在限制范围内"
    echo "═══════════════════════════════════════════"
    exit 0
fi

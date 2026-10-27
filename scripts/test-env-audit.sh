#!/bin/bash
# TideTerm 测试环境审计脚本
# 三步任务：
#   1) 从远程仓库拉取最新代码（安全 pull）
#   2) 检测当前机器是否已具备测试环境所需的依赖与工具，列出缺失项
#   3) 基于检测结果，输出一份有序的「测试环境安装计划」（含安装方式、顺序、验证步骤）
#
# 用法（在仓库根目录执行）：
#   bash scripts/test-env-audit.sh
#
# 本脚本只做「检测 + 输出计划」，不会自动安装任何东西（避免耗时/误改系统）。
# 真正的安装请按第三部分给出的命令手动执行。

# ─────────────────────────────────────────────
# 0. 通用辅助
# ─────────────────────────────────────────────
IS_WINDOWS=0
case "$(uname -s 2>/dev/null)" in
  MINGW*|MSYS*|CYGWIN*) IS_WINDOWS=1 ;;
  *) IS_WINDOWS=0 ;;
esac

# 纯文本标记（便于重定向到日志文件，不依赖终端颜色/转义）
ok()   { printf "  [OK]      %s\n" "$1"; }
bad()  { printf "  [MISSING] %s\n" "$1"; }
warn() { printf "  [WARN]    %s\n" "$1"; }
info() { printf "  [INFO]    %s\n" "$1"; }

# 版本比较：ver_ge A B  -> 当 A >= B 时返回 0
ver_ge() {
  [ "$1" = "$2" ] && return 0
  local smaller
  smaller=$(printf '%s\n%s\n' "$1" "$2" | sort -V | head -n1)
  [ "$smaller" = "$2" ]
}

# 在 Windows 上用 PowerShell 读取注册表/版本；非 Windows 或缺少 pwsh 时静默返回空
ps_read() {
  if [ "$IS_WINDOWS" -eq 1 ]; then
    if command -v pwsh.exe >/dev/null 2>&1; then
      pwsh.exe -NoProfile -NonInteractive -Command "$1" 2>/dev/null
    fi
  fi
}

PASS=0; FAIL=0; MISSING_LIST=()
note_missing() { MISSING_LIST+=("$1"); FAIL=$((FAIL+1)); bad "$2"; }
note_ok()      { PASS=$((PASS+1)); ok "$1"; }

# 期望的版本基线（来自 rust-toolchain.toml / docs/wiki/Development.md）
RUST_WANT="1.99.0"

echo "══════════════════════════════════════════════════════════════"
echo "  TideTerm 测试环境审计"
echo "  系统: $(uname -s 2>/dev/null) | Windows=$IS_WINDOWS"
echo "══════════════════════════════════════════════════════════════"


# ─────────────────────────────────────────────
# 任务 1/3：拉取远程最新代码
# ─────────────────────────────────────────────
echo ""
echo "▶ 任务 1/3：拉取远程最新代码"
if ! git rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  warn "当前目录不是 git 仓库，跳过拉取。"
elif [ -z "$(git remote 2>/dev/null)" ]; then
  warn "未配置任何 git remote，跳过拉取。"
else
  BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null)
  info "当前分支：${BRANCH:-<未知>}"
  if [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    warn "工作区存在未提交改动，为安全起见跳过 git pull（避免覆盖本地修改）。"
    warn "请先 git stash / git commit 后再手动执行：git pull --ff-only"
  else
    info "工作区干净，执行 git pull --ff-only ..."
    if git pull --ff-only 2>&1 | tail -n 15; then
      info "已更新到最新（或已是最新）。"
    else
      warn "git pull 失败，请检查网络与远程配置；后续检测仍会继续。"
    fi
  fi
fi


# ─────────────────────────────────────────────
# 任务 2/3：依赖与工具检测
# ─────────────────────────────────────────────
echo ""
echo "▶ 任务 2/3：检测测试环境依赖"

# ---- 2.1 Rust 工具链 ----
echo ""
echo "  [Rust 工具链]"
HAVE_RUST=0; HAVE_RUST_OK=0; HAVE_RUSTFMT=0; HAVE_CLIPPY=0; HAVE_MSVC=0; HAVE_QT=0
if command -v cargo >/dev/null 2>&1; then
  HAVE_RUST=1
  RUSTC_VER=$(rustc --version | cut -d' ' -f2)
  info "rustc: ${RUSTC_VER} | cargo: $(cargo --version | cut -d' ' -f2)"
  if [ "$RUSTC_VER" = "$RUST_WANT" ]; then HAVE_RUST_OK=1; note_ok "Rust 版本与 rust-toolchain.toml 一致 (${RUST_WANT})"; else note_missing "rust" "Rust 为 ${RUSTC_VER}，项目要求 ${RUST_WANT}（运行 rustup toolchain install ${RUST_WANT}）"; fi
  if command -v rustfmt >/dev/null 2>&1 || cargo fmt --version >/dev/null 2>&1; then HAVE_RUSTFMT=1; note_ok "rustfmt 已安装"; else note_missing "rustfmt" "缺少 rustfmt（cargo fmt 门禁需要）"; fi
  if cargo clippy --version >/dev/null 2>&1; then HAVE_CLIPPY=1; note_ok "clippy 已安装"; else note_missing "clippy" "缺少 clippy（cargo clippy -D warnings 门禁需要）"; fi
  if [ "$IS_WINDOWS" -eq 1 ]; then
    if rustup target list --installed 2>/dev/null | grep -q 'x86_64-pc-windows-msvc'; then HAVE_MSVC=1; note_ok "MSVC 目标 x86_64-pc-windows-msvc 已安装"; else note_missing "msvc" "缺少 Windows MSVC 编译目标（rustup target add x86_64-pc-windows-msvc）"; fi
  fi
else
  note_missing "rust" "未检测到 Rust/cargo（需 ${RUST_WANT}）"
fi

# ---- 2.3 Windows 系统级依赖（Qt 壳构建需要的 MSVC + Qt） ----
if [ "$IS_WINDOWS" -eq 1 ]; then
  echo ""
  echo "  [Windows 系统依赖 - MSVC 构建工具]"
  # Visual Studio Build Tools 2022 + MSVC
  VS=$(ps_read '$vs="${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe";if(Test-Path $vs){&$vs -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationVersion 2>$null}else{"MISSING"}' | tr -d '\r')
  if [ "$VS" != "MISSING" ] && [ -n "$VS" ]; then note_ok "Visual Studio C++ 构建工具已安装 ($VS)"; else note_missing "vsbuild" "缺少 VS2022 生成工具 + MSVC（Qt 原生构建需要）"; fi
  QT_CANDIDATE="${QT_DIR:-}"
  if [ -n "$QT_CANDIDATE" ] && [ -d "$QT_CANDIDATE/include/QtWidgets" ]; then HAVE_QT=1; note_ok "Qt Widgets SDK 已安装 ($QT_CANDIDATE)"; else note_missing "qtsdk" "缺少 Qt Widgets SDK，或 QT_DIR 未指向 Qt 6 MSVC 套件"; fi
fi

# ---- 2.4 项目级依赖 ----
echo ""
echo "  [项目级依赖]"
if [ -f native/Cargo.toml ] && [ -f crates/tiderm-terminal/Cargo.toml ] && [ -f crates/tiderm-textdoc/Cargo.toml ] && [ -f crates/tiderm-core/Cargo.toml ] && [ -f vendor/libssh2-sys/Cargo.toml ]; then
  note_ok "native / crates / vendor / tiderm-core 均存在"
else
  note_missing "layout" "Qt 客户端或共享 crates/vendor 缺失，仓库结构不完整"
fi

# ---- 2.5 可选环境变量（真实 SSH 链路测试） ----
echo ""
echo "  [可选环境变量]"
if [ -n "${TEST_SSH_HOST:-}" ] && [ -n "${TEST_SSH_USER:-}" ]; then
  note_ok "已设置 TEST_SSH_HOST / TEST_SSH_USER（真实 SSH/SFTP `#[ignore]` 测试可用）"
else
  info "未设置 TEST_SSH_*（可选）：仅真实 SSH/SFTP 链路测试需要，缺失时相关测试自动跳过"
fi


# ─────────────────────────────────────────────
# 任务 3/3：基于检测结果输出安装计划
# ─────────────────────────────────────────────
echo ""
echo "▶ 任务 3/3：测试环境安装计划"
if [ "${#MISSING_LIST[@]}" -eq 0 ]; then
  echo ""
  ok "检测通过，当前机器已具备完整测试环境，无需额外安装。"
else
  echo ""
  echo "  共发现 ${#MISSING_LIST[@]} 项缺失。请按以下顺序安装（序号即推荐执行顺序）："
fi

# 有序的安装步骤：仅当该项缺失时才打印
STEP=0
plan() {
  # $1=key(需在 MISSING_LIST) $2=标题 $3=用途 $4=方式 $5=安装命令 $6=验证命令
  local key="$1"
  local found=0
  for m in "${MISSING_LIST[@]}"; do [ "$m" = "$key" ] && found=1 && break; done
  [ "$found" -eq 0 ] && return
  STEP=$((STEP+1))
  printf '\n  [%d] %s\n' "$STEP" "$2"
  printf '      用途 : %s\n' "$3"
  printf '      方式 : %s\n' "$4"
  printf '      安装 : %s\n' "$5"
  printf '      验证 : %s\n' "$6"
}

plan "rust" "Rust 工具链 ${RUST_WANT}" "后端编译 / cargo test / clippy" "rustup (windows-gnu 或 msvc)" \
  "rustup toolchain install ${RUST_WANT} --profile minimal && rustup default ${RUST_WANT} && rustup component add rustfmt clippy" \
  "rustc --version  (应为 ${RUST_WANT}) ; cargo clippy --version"

plan "msvc" "Windows MSVC 编译目标" "Qt 壳在 Windows 上原生构建" "rustup target" \
  "rustup target add x86_64-pc-windows-msvc" \
  "rustup target list --installed | grep x86_64-pc-windows-msvc"

plan "vsbuild" "Visual Studio 2022 生成工具 + MSVC" "Qt 原生依赖（C++ 工具集/Windows SDK）" "winget 或官方安装器" \
  "winget install Microsoft.VisualStudio.2022.BuildTools --override \"--add Microsoft.VisualStudio.Workload.VCTools --includeRecommended\"" \
  "powershell: vswhere -latest -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64"

plan "qtsdk" "Qt 6 Widgets SDK" "native 的 C++ 视图层" "Qt Online Installer" \
  "安装 Qt 6 MSVC 套件，并设置 QT_DIR 指向其根目录" \
  "test -d \"$QT_DIR/include/QtWidgets\""

plan "layout" "仓库结构完整性" "native / crates / vendor / tiderm-core" "git" \
  "重新 clone 或修复缺失目录（保持 Qt 壳、共享库与 vendored libssh2 齐备）" \
  "test -f native/Cargo.toml && test -f crates/tiderm-terminal/Cargo.toml"

# ─────────────────────────────────────────────
# 汇总
# ─────────────────────────────────────────────
echo ""
echo "══════════════════════════════════════════════════════════════"
echo "  检测汇总: ${PASS} 项满足 / ${FAIL} 项缺失"
echo "══════════════════════════════════════════════════════════════"

# 通用验证步骤（无论是否缺失都给出，便于一次性自检）
echo ""
echo "  完成上述安装后，用以下命令做端到端验证："
echo "    • Rust 检查 : cd crates/tiderm-core && cargo check --tests && cargo clippy --all-targets -- -D warnings"
echo "    • 门禁总检  : bash scripts/verify.sh"
echo "    • Qt 壳     : cd native && cargo test --bin native && cargo clippy --all-targets -- -D warnings"
echo "    • 客户端    : cd native && cargo run --release -- --pty"
echo ""
if [ "$FAIL" -gt 0 ]; then
  echo "  结论：存在缺失项，请先按上面顺序补齐再跑测试。"
else
  echo "  结论：环境就绪，可直接运行测试。"
fi
echo ""

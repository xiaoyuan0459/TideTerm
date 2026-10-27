// SessionVault —— 主密码凭据库（vault 域桥）的 C++ 交互流：设置/解锁对话框、
// 保存会话后的凭据同步、打开会话前的解锁重试。无 moc：QInputDialog 静态方法。
//
// 安全约定（对齐 rust/vault.rs）：vault.json 只存 AES-256-GCM 密文；主密码
// 副本只在 Rust 侧内存（Zeroizing），C++ 拿到后即交桥、不留存。
#pragma once

#include "cpp/bridge_shim.h" // SessCfg（session 域桥 POD）+ vault 域桥函数

#include <cstdint>

class QWidget;

namespace chrome {

/// `sess_open` 的「凭据库未解锁」哨兵。与 Rust 侧 `VAULT_LOCKED`
/// （u32::MAX - 1）一致——cxx 不导出 Rust 常量，两处需同步。
constexpr uint32_t kVaultLocked = 0xFFFFFFFEu;

/// 打开持久会话：sess_open + 凭据库锁定时的解锁重试（一次）。
/// 返回标签 id；kVaultLocked = 用户取消解锁（调用方提示后放弃打开）。
uint32_t sess_open_with_vault(QWidget *parent, const rust::String &sess_id);

/// 保存会话后按 `ssh_remember` 同步凭据库：
/// - 未勾选 → 删除该会话的全部凭据（无需解锁）；
/// - 勾选且表单有密码/口令 → 加密存入（调用前需 ensure_vault_ready）；
/// - 勾选但字段为空 → 保留既有条目（表单永不回填密码，视为「未改动」）。
void sync_saved_credentials(const SessCfg &cfg, const rust::String &id);

/// 确保凭据库可用：未设置 → 设置向导；已设置 → 解锁。false = 用户取消。
bool ensure_vault_ready(QWidget *parent);
} // namespace chrome

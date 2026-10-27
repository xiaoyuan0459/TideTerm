//! vault 域桥（ADR-014 §Decision 3「按域拆桥」）：主密码凭据库的 C++ 操作面。
//!
//! `vault_` 前缀跨桥全局唯一；错误以空串 = 成功约定回传（对齐 session 域）。
//! 桥面只暴露**会话语义**操作（按会话 id 存/取密码与口令）；键格式是
//! [`crate::rust::vault`] 的内部约定，绝不外泄——否则存取与注入会错位。

#[cxx::bridge]
pub mod vault_ffi {
    /// [`fn vault_load_password`] 结果（cxx 桥不便传 Result<(String, String)>）。
    pub struct VaultLoad {
        /// true = secret 有效；false 时 error 说明原因（未解锁 / 无条目 / 解密失败）。
        pub ok: bool,
        pub error: String,
        /// 凭据明文（仅本进程内存；ok = false 时为空）。
        pub secret: String,
    }

    extern "Rust" {
        /// vault.json 是否已设置（用户设过主密码）。
        fn vault_is_set() -> bool;
        /// 首次设置主密码（至少 8 位）并解锁；返回错误文本，空 = 成功。
        fn vault_setup(master_password: String) -> String;
        /// 验证主密码并缓存解锁态（连续失败触发 core 节流锁定）；空 = 成功。
        fn vault_unlock(master_password: String) -> String;
        /// 清除本进程解锁态（主密码副本清零）。
        fn vault_lock();
        fn vault_is_unlocked() -> bool;
        /// 加密存「会话密码」（需已解锁；覆盖旧值）；空 = 成功。
        fn vault_store_password(session_id: String, secret: String) -> String;
        /// 加密存「会话私钥口令」（需已解锁；覆盖旧值）；空 = 成功。
        fn vault_store_passphrase(session_id: String, secret: String) -> String;
        /// 读「会话密码」（需已解锁）。
        fn vault_load_password(session_id: String) -> VaultLoad;
        /// 会话是否存有任何凭据（密码或口令；无需解锁）。
        fn vault_has_credentials(session_id: String) -> bool;
        /// 删除会话全部凭据（密码 + 口令；无需解锁）；true = 删除了条目。
        fn vault_forget_session(session_id: String) -> bool;
        /// 自检钩子：vault.json 重定向到指定目录（hermetic，防污染用户数据）。
        fn vault_test_dir(path: String);
        /// 自检钩子：恢复默认 store 路径并清除解锁态。
        fn vault_test_reset();
    }
}

fn vault_is_set() -> bool {
    crate::rust::vault::is_set()
}

fn vault_setup(master_password: String) -> String {
    crate::rust::vault::setup(&master_password)
        .err()
        .unwrap_or_default()
}

fn vault_unlock(master_password: String) -> String {
    crate::rust::vault::unlock(&master_password)
        .err()
        .unwrap_or_default()
}

fn vault_lock() {
    crate::rust::vault::lock()
}

fn vault_is_unlocked() -> bool {
    crate::rust::vault::is_unlocked()
}

fn vault_store_password(session_id: String, secret: String) -> String {
    crate::rust::vault::store_session_password(&session_id, &secret)
        .err()
        .unwrap_or_default()
}

fn vault_store_passphrase(session_id: String, secret: String) -> String {
    crate::rust::vault::store_session_passphrase(&session_id, &secret)
        .err()
        .unwrap_or_default()
}

fn vault_load_password(session_id: String) -> vault_ffi::VaultLoad {
    match crate::rust::vault::load_session_password(&session_id) {
        Ok(secret) => vault_ffi::VaultLoad {
            ok: true,
            error: String::new(),
            secret,
        },
        Err(error) => vault_ffi::VaultLoad {
            ok: false,
            error,
            secret: String::new(),
        },
    }
}

fn vault_has_credentials(session_id: String) -> bool {
    crate::rust::vault::session_has_credentials(&session_id)
}

fn vault_forget_session(session_id: String) -> bool {
    let had = crate::rust::vault::session_has_credentials(&session_id);
    crate::rust::vault::remove_session_credentials(&session_id);
    had
}

fn vault_test_dir(path: String) {
    tiderm_core::master_password::set_vault_dir(Some(std::path::PathBuf::from(path.to_string())));
}

fn vault_test_reset() {
    crate::rust::vault::lock();
    tiderm_core::master_password::set_vault_dir(None);
}

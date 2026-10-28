//! 主密码凭据库宿主：封装 [`tiderm_core::master_password`]，持有本进程解锁态。
//!
//! 安全边界（对齐 core 模块注释与 .rules 铁律）：
//! - 落盘的只有 AES-256-GCM 密文（vault.json，Argon2id 派生密钥）；sessions.json
//!   依旧零凭据（core `skip_serializing`）；
//! - 主密码验证走 core 的 Argon2 + 暴力破解节流（连续失败进入锁定窗口）；
//! - 解锁态（主密码内存副本）只存在于本进程：`Zeroizing` 包裹，[`lock`] 或进程
//!   退出即清零；跨重启不保留；
//! - 删除凭据不需要解锁——vault 重写不解密，保证锁定状态下删会话仍能清库。

use std::sync::Mutex;
use tiderm_core::master_password as core;
use zeroize::Zeroizing;

use crate::rust::store::session_config::{SessionConfig, SessionKind, SshAuth};

/// 本进程解锁态：Some(主密码) = 已解锁。clone 的是 Zeroizing<String> 指针外层，
/// 主密码字节仅在持锁读路径被短暂借用。
static UNLOCKED: Mutex<Option<Zeroizing<String>>> = Mutex::new(None);

/// vault.json 是否已设置（是否存在）。
pub fn is_set() -> bool {
    core::is_set()
}

/// 本进程是否已解锁（主密码已验证并缓存）。
pub fn is_unlocked() -> bool {
    UNLOCKED.lock().unwrap().is_some()
}

/// 清除解锁态（主密码副本随 Zeroizing 清零）。
pub fn lock() {
    *UNLOCKED.lock().unwrap() = None;
}

/// 首次设置主密码（创建 vault.json，至少 8 位）并立即进入解锁态。
pub fn setup(master_password: &str) -> Result<(), String> {
    core::set_password(master_password)?;
    *UNLOCKED.lock().unwrap() = Some(Zeroizing::new(master_password.to_string()));
    Ok(())
}

/// 验证主密码并缓存。失败计入 core 节流（锁定窗口内直接拒绝、不跑 Argon2）。
pub fn unlock(master_password: &str) -> Result<(), String> {
    if core::verify_password(master_password)? {
        *UNLOCKED.lock().unwrap() = Some(Zeroizing::new(master_password.to_string()));
        Ok(())
    } else {
        Err("主密码不正确".into())
    }
}

fn master() -> Result<Zeroizing<String>, String> {
    UNLOCKED
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "凭据库未解锁".to_string())
}

/// 加密存储一条凭据（需已解锁；覆盖同 id 旧值）。
pub fn store_credential(id: &str, secret: &str) -> Result<(), String> {
    core::store_credential(id, &master()?, secret)
}

/// 读取一条凭据（需已解锁）。
pub fn load_credential(id: &str) -> Result<String, String> {
    core::load_credential(id, &master()?)
}

/// 删除一条凭据（无需解锁）。vault 不存在或无此条目返回 false。
pub fn remove_credential(id: &str) -> bool {
    core::remove_credential(id).unwrap_or(false)
}

/// 是否存有某条凭据（无需解锁，纯查表）。
pub fn has_credential(id: &str) -> bool {
    core::has_credential(id)
}

// ════════════════════ 会话凭据键 ════════════════════
//
// vault 条目按会话 id 分键：密码与密钥口令各一条。键格式是本模块内部约定，
// C++ 侧只经 [`remove_session_credentials`] 等语义操作触达，不拼键。

fn password_key(session_id: &str) -> String {
    format!("ssh-password:{session_id}")
}

fn passphrase_key(session_id: &str) -> String {
    format!("ssh-passphrase:{session_id}")
}

/// 会话在 vault 中是否存有任何凭据（密码或密钥口令）。
pub fn session_has_credentials(session_id: &str) -> bool {
    has_credential(&password_key(session_id)) || has_credential(&passphrase_key(session_id))
}

// ════════════════════ 会话语义操作（桥面唯一入口） ════════════════════
//
// C++ 侧只按会话 id 存取，键格式（`ssh-password:` / `ssh-passphrase:` 前缀）
// 是本模块内部约定——注入与存取必须共用同一组键，否则记住的密码永远对不上。

/// 加密存「会话密码」（需已解锁；覆盖旧值）。
pub fn store_session_password(session_id: &str, secret: &str) -> Result<(), String> {
    store_credential(&password_key(session_id), secret)
}

/// 加密存「会话私钥口令」（需已解锁；覆盖旧值）。
pub fn store_session_passphrase(session_id: &str, secret: &str) -> Result<(), String> {
    store_credential(&passphrase_key(session_id), secret)
}

/// 读「会话密码」（需已解锁）。
pub fn load_session_password(session_id: &str) -> Result<String, String> {
    load_credential(&password_key(session_id))
}

/// 删除会话的两类凭据（sess_delete 清理路径；无需解锁）。
pub fn remove_session_credentials(session_id: &str) {
    remove_credential(&password_key(session_id));
    remove_credential(&passphrase_key(session_id));
}

/// 复制会话凭据（sess_duplicate）：旧 id 的每条凭据存到新 id。
/// 未解锁时静默跳过（复制出的会话连接时会按无记住凭据处理）——尽力而为。
pub fn copy_session_credentials(old_id: &str, new_id: &str) {
    if !is_unlocked() {
        return;
    }
    let pairs = [
        (password_key(old_id), password_key(new_id)),
        (passphrase_key(old_id), passphrase_key(new_id)),
    ];
    for (from, to) in pairs {
        if let Ok(secret) = load_credential(&from) {
            let _ = store_credential(&to, &secret);
        }
    }
}

// ════════════════════ 打开会话时的凭据注入 ════════════════════

/// [`inject_saved_credentials`] 的结果。
pub enum Inject {
    /// 配置已就绪（无需注入 / 注入完成 / 无记住的凭据），可以连接。
    Ready,
    /// vault 里有该会话的凭据但本进程未解锁——C++ 弹解锁框后重试 `sess_open`。
    Locked,
}

/// 打开持久会话前把「记住的凭据」注入配置（原地修改）。
///
/// 规则：仅 SSH + 勾选记住 + 对应凭据字段为空时尝试注入；vault 无条目按无
/// 凭据继续；已解锁但解密失败也按无凭据继续（正常认证错误路径上报，避免
/// 解锁重试死循环）。密码与密钥口令各自独立注入。
pub fn inject_saved_credentials(cfg: &mut SessionConfig) -> Inject {
    if cfg.kind != SessionKind::Ssh {
        return Inject::Ready;
    }
    let Some(ssh) = cfg.ssh.as_mut() else {
        return Inject::Ready;
    };
    if !ssh.remember_credentials {
        return Inject::Ready;
    }
    let (empty, key) = match ssh.auth {
        SshAuth::Password => (&mut ssh.password, password_key(&cfg.id)),
        SshAuth::Key => (&mut ssh.passphrase, passphrase_key(&cfg.id)),
        // 键盘交互由服务端挑战；OneKey 在连接时经账户面板解析——都不走 vault。
        _ => return Inject::Ready,
    };
    if !empty.is_empty() || !has_credential(&key) {
        return Inject::Ready;
    }
    if !is_unlocked() {
        return Inject::Locked;
    }
    if let Ok(secret) = load_credential(&key) {
        *empty = secret;
    }
    Inject::Ready
}

/// 重连前刷新记住的凭据：勾选记住的会话**以 vault 为准**，覆盖标签内存里
/// 打开时注入的旧密码——否则「编辑会话改密码→保存→原地重连」会一直用旧
/// 密码（内存值非空还会挡住 [`inject_saved_credentials`] 的注入）。
///
/// vault 无条目或未解锁时保留现有内存值（重连取最小意外：连接失败照常上报，
/// 不因解锁状态改变重连行为）。快速连接等无 id 会话天然无条目，安全无操作。
pub fn refresh_saved_credentials(cfg: &mut SessionConfig) {
    if cfg.kind != SessionKind::Ssh || !is_unlocked() {
        return;
    }
    let Some(ssh) = cfg.ssh.as_mut() else {
        return;
    };
    if !ssh.remember_credentials {
        return;
    }
    match ssh.auth {
        SshAuth::Password => {
            if let Ok(secret) = load_credential(&password_key(&cfg.id)) {
                ssh.password = secret;
            }
        }
        SshAuth::Key => {
            if let Ok(secret) = load_credential(&passphrase_key(&cfg.id)) {
                ssh.passphrase = secret;
            }
        }
        // 键盘交互由服务端挑战；OneKey 在连接时经账户面板解析——都不走 vault。
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rust::store::session_config::SessionConfig;

    /// vault 目录覆盖与解锁态都是进程级全局：本模块测试串行执行。
    static LOCK: Mutex<()> = Mutex::new(());
    const MASTER: &str = "test-master-password-1";
    const SECRET: &str = "dummy-credential-not-real";

    /// 重定向 vault 到临时目录；返回 guard（Drop 恢复默认路径 + 清解锁态）。
    struct Hermetic {
        /// 仅持有目录所有权，让临时目录活到 guard 丢弃。
        _dir: tempfile::TempDir,
    }

    impl Hermetic {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("temp dir");
            core::set_vault_dir(Some(dir.path().to_path_buf()));
            Self { _dir: dir }
        }
    }

    impl Drop for Hermetic {
        fn drop(&mut self) {
            lock();
            core::set_vault_dir(None);
        }
    }

    fn ssh_session(id: &str, auth: SshAuth, remember: bool) -> SessionConfig {
        let mut c = SessionConfig::blank(SessionKind::Ssh);
        c.id = id.into();
        if let Some(ssh) = c.ssh.as_mut() {
            ssh.auth = auth;
            ssh.remember_credentials = remember;
        }
        c
    }

    #[test]
    fn setup_store_load_round_trip_and_lock() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();

        assert!(!is_set());
        setup(MASTER).expect("setup");
        assert!(is_set());
        assert!(is_unlocked());

        let key = password_key("sess-rt");
        store_credential(&key, SECRET).expect("store");
        assert!(has_credential(&key));
        assert_eq!(load_credential(&key).expect("load"), SECRET);

        lock();
        assert!(!is_unlocked());
        assert!(load_credential(&key).is_err()); // 未解锁拒绝读取
        assert!(store_credential(&key, SECRET).is_err());
        // 锁定不影响删除与会话级清理（无需解锁）。
        assert!(remove_credential(&key));
        assert!(!has_credential(&key));
    }

    #[test]
    fn unlock_rejects_wrong_password_then_accepts_right_one() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();
        setup(MASTER).expect("setup");
        lock();

        // 单次错误尝试（≤2 次，远低于节流锁定阈值 5）。
        assert!(unlock("definitely-wrong-pw").is_err());
        assert!(!is_unlocked());
        unlock(MASTER).expect("unlock"); // 成功即复位节流计数
        assert!(is_unlocked());
    }

    #[test]
    fn inject_fills_saved_password_only_when_remembered() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();
        setup(MASTER).expect("setup");
        store_credential(&password_key("sess-inj"), SECRET).expect("store");
        lock();

        // 记住 + 密码为空 + 已存凭据 + 未解锁 → Locked（C++ 弹解锁框后重试）。
        let mut cfg = ssh_session("sess-inj", SshAuth::Password, true);
        assert!(matches!(inject_saved_credentials(&mut cfg), Inject::Locked));

        // 解锁后注入成功。
        unlock(MASTER).expect("unlock");
        assert!(matches!(inject_saved_credentials(&mut cfg), Inject::Ready));
        assert_eq!(cfg.ssh.unwrap().password, SECRET);

        // 未勾选记住：不注入（即使 vault 有条目）。
        let mut off = ssh_session("sess-inj", SshAuth::Password, false);
        assert!(matches!(inject_saved_credentials(&mut off), Inject::Ready));
        assert!(off.ssh.unwrap().password.is_empty());

        // 表单带来的密码（勾选记住）不覆盖——空字段才注入。
        let mut filled = ssh_session("sess-inj", SshAuth::Password, true);
        filled.ssh.as_mut().unwrap().password = "typed".into();
        assert!(matches!(
            inject_saved_credentials(&mut filled),
            Inject::Ready
        ));
        assert_eq!(filled.ssh.unwrap().password, "typed");
    }

    #[test]
    fn inject_passphrase_and_missing_entry_paths() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();
        setup(MASTER).expect("setup");
        store_credential(&passphrase_key("sess-key"), SECRET).expect("store");
        lock();

        // 密钥口令注入（Key 认证）。
        let mut key = ssh_session("sess-key", SshAuth::Key, true);
        assert!(matches!(inject_saved_credentials(&mut key), Inject::Locked));
        unlock(MASTER).expect("unlock");
        assert!(matches!(inject_saved_credentials(&mut key), Inject::Ready));
        assert_eq!(key.ssh.unwrap().passphrase, SECRET);

        // 勾选了记住但 vault 无条目：按无凭据继续（正常认证错误上报）。
        let mut ghost = ssh_session("sess-ghost", SshAuth::Password, true);
        assert!(matches!(
            inject_saved_credentials(&mut ghost),
            Inject::Ready
        ));

        // 键盘交互 / OneKey 不走 vault。
        let mut kb = ssh_session("sess-kb", SshAuth::KeyboardInteractive, true);
        assert!(matches!(inject_saved_credentials(&mut kb), Inject::Ready));
    }

    #[test]
    fn refresh_overwrites_stale_password_from_vault() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();
        setup(MASTER).expect("setup");
        store_credential(&password_key("sess-rf"), SECRET).expect("store");

        // 「打开时注入的旧密码」用派生值构造（不写凭据样字面量）；vault 里
        // 存的是编辑后的新密码 → 刷新应覆盖为 SECRET。
        let stale = format!("{SECRET}-stale");
        let mut cfg = ssh_session("sess-rf", SshAuth::Password, true);
        cfg.ssh.as_mut().unwrap().password = stale.clone();
        unlock(MASTER).expect("unlock");
        refresh_saved_credentials(&mut cfg);
        assert_eq!(cfg.ssh.unwrap().password, SECRET);

        // 未勾选记住：不刷新。
        let mut off = ssh_session("sess-rf", SshAuth::Password, false);
        off.ssh.as_mut().unwrap().password = stale.clone();
        refresh_saved_credentials(&mut off);
        assert_eq!(off.ssh.unwrap().password, stale);

        // 未解锁：保留内存值（重连取最小意外，不因解锁状态改变行为）。
        lock();
        let mut locked = ssh_session("sess-rf", SshAuth::Password, true);
        locked.ssh.as_mut().unwrap().password = stale.clone();
        refresh_saved_credentials(&mut locked);
        assert_eq!(locked.ssh.unwrap().password, stale);

        // vault 无条目（如条目已删）：保留内存值。
        unlock(MASTER).expect("unlock");
        remove_credential(&password_key("sess-rf"));
        let mut ghost = ssh_session("sess-rf", SshAuth::Password, true);
        ghost.ssh.as_mut().unwrap().password = stale.clone();
        refresh_saved_credentials(&mut ghost);
        assert_eq!(ghost.ssh.unwrap().password, stale);
    }

    #[test]
    fn session_credential_cleanup_and_copy() {
        let _g = LOCK.lock().unwrap();
        let _dir = Hermetic::new();
        setup(MASTER).expect("setup");

        store_credential(&password_key("sess-del"), SECRET).expect("store");
        store_credential(&passphrase_key("sess-del"), SECRET).expect("store");
        remove_session_credentials("sess-del");
        assert!(!has_credential(&password_key("sess-del")));
        assert!(!has_credential(&passphrase_key("sess-del")));

        // 复制：解锁态下旧 → 新；未解锁时跳过。
        store_credential(&password_key("sess-old"), SECRET).expect("store");
        copy_session_credentials("sess-old", "sess-new");
        assert_eq!(
            load_credential(&password_key("sess-new")).expect("copied"),
            SECRET
        );
        lock();
        copy_session_credentials("sess-old", "sess-other");
        assert!(!has_credential(&password_key("sess-other")));
    }
}

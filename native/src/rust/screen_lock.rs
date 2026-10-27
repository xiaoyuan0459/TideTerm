//! Persistent lock settings and inactivity policy. Secrets are stored only as salted verifiers.
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Algorithm, Argon2, Params, Version,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

#[derive(Serialize, Deserialize)]
struct PasswordFile {
    version: u8,
    #[serde(default)]
    password_hash: Option<String>,
    #[serde(default = "default_idle_minutes")]
    idle_minutes: u32,
}

const DEFAULT_IDLE_MINUTES: u32 = 0;
fn default_idle_minutes() -> u32 {
    DEFAULT_IDLE_MINUTES
}

struct IdleTimer {
    minutes: u32,
    last_activity: Instant,
}

impl IdleTimer {
    fn due(&self, now: Instant) -> bool {
        self.minutes != 0
            && now.duration_since(self.last_activity)
                >= Duration::from_secs(u64::from(self.minutes) * 60)
    }
    fn activity(&mut self, now: Instant) {
        self.last_activity = now;
    }
}

#[derive(Default)]
struct Attempts {
    failures: u32,
    escalations: u32,
    until: Option<Instant>,
}

struct PasswordStore {
    path: PathBuf,
    attempts: Mutex<Attempts>,
    idle: Mutex<IdleTimer>,
}

impl PasswordStore {
    fn new(path: PathBuf) -> Self {
        let minutes = Self::read(&path)
            .map(|file| file.idle_minutes)
            .unwrap_or(DEFAULT_IDLE_MINUTES);
        Self {
            path,
            attempts: Mutex::new(Attempts::default()),
            idle: Mutex::new(IdleTimer {
                minutes,
                last_activity: Instant::now(),
            }),
        }
    }

    fn read(path: &std::path::Path) -> Result<PasswordFile, String> {
        let size = match fs::metadata(path) {
            Ok(metadata) => metadata.len(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(PasswordFile {
                    version: 2,
                    password_hash: None,
                    idle_minutes: DEFAULT_IDLE_MINUTES,
                })
            }
            Err(_) => return Err("无法读取锁屏配置".into()),
        };
        if size > 4096 {
            return Err("锁屏配置损坏".into());
        }
        let data = fs::read_to_string(path).map_err(|_| "无法读取锁屏配置")?;
        let file: PasswordFile = serde_json::from_str(&data).map_err(|_| "锁屏配置损坏")?;
        if !matches!(file.version, 1 | 2)
            || file.idle_minutes > 1440
            || (file.version == 1 && file.password_hash.is_none())
        {
            return Err("锁屏配置损坏".into());
        }
        Ok(file)
    }

    fn is_set(&self) -> bool {
        // A read/access error must not silently turn a protected lock into Enter-to-unlock.
        Self::read(&self.path)
            .map(|file| file.password_hash.is_some())
            .unwrap_or(true)
    }

    fn verify_locked(&self, password: &str, attempts: &mut Attempts) -> Result<(), String> {
        let now = Instant::now();
        if let Some(until) = attempts.until.filter(|until| *until > now) {
            return Err(format!(
                "尝试过多，请在 {} 秒后重试",
                until.duration_since(now).as_secs() + 1
            ));
        }
        let file = Self::read(&self.path)?;
        let hash = file.password_hash.as_deref().ok_or("尚未设置锁屏密码")?;
        let parsed = PasswordHash::new(hash).map_err(|_| "锁屏配置损坏")?;
        let params = Params::try_from(&parsed).map_err(|_| "锁屏密码配置损坏")?;
        if parsed.algorithm.as_str() != "argon2id"
            || parsed.version != Some(19)
            || params.m_cost() != 65536
            || params.t_cost() != 3
            || params.p_cost() != 4
        {
            return Err("锁屏配置损坏".into());
        }
        match Argon2::default().verify_password(password.as_bytes(), &parsed) {
            Ok(()) => {
                *attempts = Attempts::default();
                Ok(())
            }
            Err(argon2::password_hash::Error::Password) => {
                attempts.failures += 1;
                if attempts.failures >= 5 {
                    let seconds = 30u64
                        .saturating_mul(2u64.saturating_pow(attempts.escalations))
                        .min(300);
                    attempts.until = Some(Instant::now() + Duration::from_secs(seconds));
                    attempts.escalations = attempts.escalations.saturating_add(1);
                    attempts.failures = 0;
                }
                Err("锁屏密码不正确".into())
            }
            Err(_) => Err("锁屏配置损坏".into()),
        }
    }

    fn verify(&self, password: &str) -> Result<(), String> {
        self.verify_locked(password, &mut self.attempts.lock().unwrap())
    }

    fn save(&self, current: &str, next: &str, removing: bool, minutes: u32) -> Result<(), String> {
        if minutes > 1440 {
            return Err("自动锁屏时间应为 0 至 1440 分钟".into());
        }
        let mut attempts = self.attempts.lock().unwrap();
        let mut file = Self::read(&self.path)?;
        if file.password_hash.is_some() {
            self.verify_locked(current, &mut attempts)?;
        }
        if removing {
            file.password_hash = None;
        } else if !next.is_empty() {
            if next.chars().count() < 8 || next.len() > 1024 {
                return Err("锁屏密码至少 8 个字符，最多 1024 字节".into());
            }
            let salt = SaltString::generate(&mut OsRng);
            let params = Params::new(65536, 3, 4, None).map_err(|_| "无法设置锁屏密码")?;
            file.password_hash = Some(
                Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
                    .hash_password(next.as_bytes(), &salt)
                    .map_err(|_| "无法设置锁屏密码")?
                    .to_string(),
            );
        }
        file.version = 2;
        file.idle_minutes = minutes;
        let parent = self.path.parent().ok_or("无法保存锁屏设置")?;
        fs::create_dir_all(parent).map_err(|_| "无法保存锁屏设置")?;
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|_| "无法保存锁屏设置")?;
        serde_json::to_writer(&mut temp, &file).map_err(|_| "无法保存锁屏设置")?;
        temp.flush().map_err(|_| "无法保存锁屏设置")?;
        temp.as_file().sync_all().map_err(|_| "无法保存锁屏设置")?;
        // Same-directory atomic replacement; a failed save leaves the previous verifier intact.
        temp.persist(&self.path).map_err(|_| "无法保存锁屏设置")?;
        *self.idle.lock().unwrap() = IdleTimer {
            minutes,
            last_activity: Instant::now(),
        };
        Ok(())
    }
}

fn store() -> Result<&'static PasswordStore, String> {
    static STORE: OnceLock<Result<PasswordStore, String>> = OnceLock::new();
    STORE
        .get_or_init(|| {
            tiderm_core::config_dir().map(|dir| PasswordStore::new(dir.join("screen_lock.json")))
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub fn is_set() -> bool {
    store().map(PasswordStore::is_set).unwrap_or(true)
}

pub fn verify(password: String) -> Result<(), String> {
    let password = Zeroizing::new(password);
    store()?.verify(&password)
}

pub fn save(current: String, next: String, removing: bool, minutes: u32) -> Result<(), String> {
    let current = Zeroizing::new(current);
    let next = Zeroizing::new(next);
    store()?.save(&current, &next, removing, minutes)
}

pub fn idle_minutes() -> u32 {
    store()
        .map(|store| store.idle.lock().unwrap().minutes)
        .unwrap_or(DEFAULT_IDLE_MINUTES)
}

pub fn activity() {
    if let Ok(store) = store() {
        store.idle.lock().unwrap().activity(Instant::now());
    }
}

pub fn idle_due() -> bool {
    store()
        .map(|store| store.idle.lock().unwrap().due(Instant::now()))
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "screen_lock_tests.rs"]
mod tests;

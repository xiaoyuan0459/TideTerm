//! Persistent defaults for new recordings; active writers keep their own settings.
use crate::bridge::log::log_ffi::LogSettings;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Settings {
    directory: String,
    prefix: String,
    format: u8,
    rotate_mib: u32,
    keep: u32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            directory: String::new(),
            prefix: "session".into(),
            format: 0,
            rotate_mib: 8,
            keep: 5,
        }
    }
}
impl Settings {
    fn validate(&self) -> Result<(), String> {
        if !self.directory.is_empty()
            && (!Path::new(&self.directory).is_absolute() || self.directory.len() > 4096)
        {
            return Err("日志目录必须是绝对路径".into());
        }
        if self.prefix.is_empty()
            || self.prefix.len() > 48
            || !self
                .prefix
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err("日志名前缀限 1–48 个英文字母、数字、连字符或下划线".into());
        }
        if self.format > 1
            || !(1..=256).contains(&self.rotate_mib)
            || !(2..=20).contains(&self.keep)
        {
            return Err("无效的日志格式或轮换设置".into());
        }
        Ok(())
    }
    fn view(self, root: &Path) -> LogSettings {
        LogSettings {
            directory: if self.directory.is_empty() {
                root.join("logs").display().to_string()
            } else {
                self.directory
            },
            prefix: self.prefix,
            format: self.format,
            rotate_mib: self.rotate_mib,
            keep: self.keep,
        }
    }
}
fn read(root: &Path) -> Result<LogSettings, String> {
    let path = root.join("log-settings.json");
    let settings: Settings = match std::fs::File::open(&path) {
        Ok(file) => {
            use std::io::Read;
            let mut data = Vec::new();
            file.take(8193)
                .read_to_end(&mut data)
                .map_err(|e| e.to_string())?;
            if data.len() > 8192 {
                return Err("日志设置文件过大".into());
            }
            serde_json::from_slice(&data).map_err(|e| format!("日志设置读取失败：{e}"))?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Settings::default(),
        Err(e) => return Err(format!("日志设置读取失败：{e}")),
    };
    settings.validate()?;
    Ok(settings.view(root))
}
fn write(root: &Path, options: LogSettings) -> Result<(), String> {
    use std::io::Write;
    let settings = Settings {
        directory: options.directory,
        prefix: options.prefix,
        format: options.format,
        rotate_mib: options.rotate_mib,
        keep: options.keep,
    };
    settings.validate()?;
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let mut temp = tempfile::NamedTempFile::new_in(root).map_err(|e| e.to_string())?;
    temp.write_all(&serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    temp.persist(root.join("log-settings.json"))
        .map_err(|e| e.error.to_string())?;
    Ok(())
}
pub fn slog_settings() -> Result<LogSettings, String> {
    read(&super::store::state_directory()?)
}
pub fn slog_default_settings() -> Result<LogSettings, String> {
    Ok(Settings::default().view(&super::store::state_directory()?))
}
pub fn slog_save_settings(options: LogSettings) -> Result<(), String> {
    write(&super::store::state_directory()?, options)
}
pub(super) fn recording() -> Result<super::continuous_log::Recording, String> {
    let options = slog_settings()?;
    super::continuous_log::Recording::configured(
        &PathBuf::from(&options.directory),
        &options.prefix,
        options.format,
        u64::from(options.rotate_mib) * 1024 * 1024,
        options.keep as usize,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_defaults_survive_reload_and_invalid_changes_preserve_previous_file() {
        let root = tempfile::tempdir().unwrap();
        let mut options = read(root.path()).unwrap();
        options.directory = root.path().join("captured").display().to_string();
        options.prefix = "work".into();
        options.format = 1;
        options.rotate_mib = 2;
        options.keep = 3;
        write(root.path(), options).unwrap();
        let before = std::fs::read(root.path().join("log-settings.json")).unwrap();
        let restored = read(root.path()).unwrap();
        assert_eq!(restored.prefix, "work");
        assert_eq!(
            (restored.format, restored.rotate_mib, restored.keep),
            (1, 2, 3)
        );
        let mut invalid = restored;
        invalid.prefix = "../escape".into();
        assert!(write(root.path(), invalid).is_err());
        assert_eq!(
            std::fs::read(root.path().join("log-settings.json")).unwrap(),
            before
        );
        std::fs::write(root.path().join("log-settings.json"), b"{invalid").unwrap();
        assert!(read(root.path()).is_err());
    }
}

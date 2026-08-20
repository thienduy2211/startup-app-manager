//! Vi tri file tren dia. Tat ca duoi `%APPDATA%\StartupAppManager`.

use std::io;
use std::path::PathBuf;

const APP_DIR_NAME: &str = "StartupAppManager";

/// Thu muc goc chua config va log.
///
/// Fallback ve thu muc hien tai khi `%APPDATA%` khong ton tai (vi du chay trong
/// moi truong service khong co user profile) de app van hoat dong thay vi panic.
pub fn config_dir() -> PathBuf {
    match std::env::var_os("APPDATA") {
        Some(appdata) => PathBuf::from(appdata).join(APP_DIR_NAME),
        None => PathBuf::from(".").join(APP_DIR_NAME),
    }
}

pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn logs_dir() -> PathBuf {
    config_dir().join("logs")
}

/// Log cua ban than manager.
pub fn manager_log_file() -> PathBuf {
    logs_dir().join("manager.log")
}

/// Log rieng cho stdout/stderr cua tung app duoc quan ly.
pub fn app_log_file(app_id: u64) -> PathBuf {
    logs_dir().join(format!("app-{app_id}.log"))
}

/// Tao san cac thu muc can thiet. Goi mot lan luc khoi dong.
pub fn ensure_dirs() -> io::Result<()> {
    std::fs::create_dir_all(logs_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_file_nam_trong_config_dir() {
        assert_eq!(config_file().parent().unwrap(), config_dir());
        assert_eq!(logs_dir().parent().unwrap(), config_dir());
    }

    #[test]
    fn app_log_file_tach_biet_theo_id() {
        assert_ne!(app_log_file(1), app_log_file(2));
        assert!(app_log_file(7).ends_with("app-7.log"));
    }
}

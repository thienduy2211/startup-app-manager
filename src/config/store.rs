//! Doc/ghi config duoi dang TOML.
//!
//! Ghi theo kieu atomic: viet ra file tam roi doi ten de len file that. Mat
//! dien giua chung chi lam mat file tam, config cu van nguyen ven.

use std::io;
use std::path::{Path, PathBuf};

use super::model::AppConfig;
use crate::paths;

/// Doc config mac dinh. Khong bao gio that bai: file thieu hoac hong deu tra
/// ve config rong kem canh bao, vi mot config hong khong duoc phep chan
/// manager khoi dong va bo mac cac service dang chay.
pub fn load() -> AppConfig {
    let path = paths::config_file();
    match load_from(&path) {
        Ok(cfg) => cfg,
        Err(LoadError::NotFound) => AppConfig::default(),
        Err(LoadError::Invalid(msg)) => {
            crate::logging::warn(&format!(
                "config {} unreadable ({msg}); using an empty config",
                path.display()
            ));
            AppConfig::default()
        }
    }
}

#[derive(Debug)]
pub enum LoadError {
    NotFound,
    Invalid(String),
}

pub fn load_from(path: &Path) -> Result<AppConfig, LoadError> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(LoadError::NotFound),
        Err(e) => return Err(LoadError::Invalid(e.to_string())),
    };
    // File rong parse ra config rong ma khong bao loi; coi la hong de manager
    // canh bao thay vi lang le bo mac cac service dang chay.
    if text.trim().is_empty() {
        return Err(LoadError::Invalid("empty file".into()));
    }
    let mut config: AppConfig =
        toml::from_str(&text).map_err(|e| LoadError::Invalid(e.to_string()))?;
    for (name, old, fresh) in config.dedupe_ids() {
        crate::logging::warn(&format!(
            "config: app \"{name}\" had duplicate id {old}, assigned new id {fresh}"
        ));
    }
    Ok(config)
}

pub fn save(config: &AppConfig) -> io::Result<()> {
    paths::ensure_dirs()?;
    save_to(config, &paths::config_file())
}

pub fn save_to(config: &AppConfig, path: &Path) -> io::Result<()> {
    let text = toml::to_string_pretty(config)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let tmp = temp_path(path);
    // `rename` chi atomic ve metadata. Khong `sync_all` thi mot lan mat dien co
    // the de lai `config.toml` dai 0 byte -- ma file rong la TOML **hop le**,
    // nen manager khoi dong lai voi 0 app va khong canh bao gi.
    {
        use std::io::Write;
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }

    // Tren Windows `rename` ghi de file dich san co (MOVEFILE_REPLACE_EXISTING).
    match std::fs::rename(&tmp, path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

fn temp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{HealthCheck, ManagedApp};

    #[test]
    fn file_rong_bi_coi_la_hong_chu_khong_phai_config_rong() {
        // Mot lan mat dien giua luc ghi co the de lai file 0 byte, ma file rong
        // lai la TOML hop le. Neu nhan im thi manager khoi dong voi 0 app va
        // user khong he biet danh sach app cua minh vua bien mat.
        let dir = temp_dir("rong");
        let path = dir.join("config.toml");
        std::fs::write(&path, "").unwrap();

        assert!(matches!(load_from(&path), Err(LoadError::Invalid(_))));
        std::fs::remove_dir_all(&dir).ok();
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sam-store-test-{}-{}-{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample() -> AppConfig {
        let mut cfg = AppConfig::default();
        let id = cfg.allocate_id();
        cfg.settings.default_check_interval_secs = 600;
        cfg.apps.push(ManagedApp {
            id,
            name: "Hermes WebUI".into(),
            exe: PathBuf::from(r"C:\venv\Scripts\python.exe"),
            args: r#""C:\Tools\hermes-webui\server.py""#.into(),
            working_dir: Some(PathBuf::from(r"C:\Tools\hermes-webui")),
            check_interval_secs: 600,
            env: [("PYTHONUNBUFFERED".to_string(), "1".to_string())].into(),
            env_from_files: [("TOKEN".to_string(), PathBuf::from(r"C:\x\token"))].into(),
            health: Some(HealthCheck {
                url: "http://127.0.0.1:8787/health".into(),
                ..Default::default()
            }),
            ..Default::default()
        });
        cfg
    }

    #[test]
    fn round_trip_giu_nguyen_du_lieu() {
        let dir = temp_dir("roundtrip");
        let path = dir.join("config.toml");
        let cfg = sample();

        save_to(&cfg, &path).unwrap();
        assert_eq!(load_from(&path).unwrap(), cfg);

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn file_khong_ton_tai_tra_ve_not_found() {
        let dir = temp_dir("missing");
        let err = load_from(&dir.join("khong-co.toml")).unwrap_err();
        assert!(matches!(err, LoadError::NotFound));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn toml_hong_bao_invalid_khong_panic() {
        let dir = temp_dir("corrupt");
        let path = dir.join("config.toml");
        std::fs::write(&path, "day khong phai toml [[[").unwrap();

        assert!(matches!(load_from(&path).unwrap_err(), LoadError::Invalid(_)));
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn config_thieu_field_van_load_duoc() {
        // Mo phong config ghi boi phien ban cu: thieu env, health, restart,
        // next_app_id. Tat ca phai roi ve default thay vi loi.
        let dir = temp_dir("partial");
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "[settings]\ndefault_check_interval_secs = 600\n\n[[apps]]\nid = 1\nname = \"cu\"\nexe = \"a.exe\"\n",
        )
        .unwrap();

        let cfg = load_from(&path).unwrap();
        assert_eq!(cfg.settings.default_check_interval_secs, 600);
        assert_eq!(cfg.apps.len(), 1);
        let app = &cfg.apps[0];
        assert_eq!(app.name, "cu");
        assert!(app.enabled, "field thieu phai ve default la bat");
        assert_eq!(app.restart.max_retries, 5);
        assert!(app.health.is_none());
        assert!(app.env.is_empty());

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn ghi_de_config_cu_khong_de_lai_file_tam() {
        let dir = temp_dir("overwrite");
        let path = dir.join("config.toml");

        save_to(&AppConfig::default(), &path).unwrap();
        let cfg = sample();
        save_to(&cfg, &path).unwrap();

        assert_eq!(load_from(&path).unwrap(), cfg);
        assert!(!temp_path(&path).exists(), "file tam phai duoc doi ten di");

        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn id_da_cap_van_giu_sau_khi_load_lai() {
        let dir = temp_dir("idpersist");
        let path = dir.join("config.toml");

        let mut cfg = AppConfig::default();
        cfg.allocate_id();
        cfg.allocate_id();
        save_to(&cfg, &path).unwrap();

        // Bo dem phai song sot qua vong doc/ghi, neu khong id se bi cap lai
        // sau moi lan khoi dong lai manager.
        let mut reloaded = load_from(&path).unwrap();
        assert_eq!(reloaded.allocate_id(), 3);

        std::fs::remove_dir_all(dir).ok();
    }
}

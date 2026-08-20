//! Gom env vars cho mot app tu ba nguon, theo thu tu uu tien tang dan:
//! `env_file` -> `env_from_files` -> `env` inline.
//!
//! File nguon chi duoc doc, khong bao gio bi ghi de. Viec doc lai o moi lan
//! spawn khien token xoay vong tu co hieu luc ma khong phai sua config.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use super::model::ManagedApp;

#[derive(Debug)]
pub enum EnvError {
    /// File duoc khai bao trong config nhung khong doc duoc.
    Unreadable {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for EnvError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EnvError::Unreadable { path, source } => {
                write!(f, "cannot read env file {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for EnvError {}

/// Gia tri o nguon sau ghi de nguon truoc.
pub fn resolve(app: &ManagedApp) -> Result<BTreeMap<String, String>, EnvError> {
    let mut out = BTreeMap::new();

    if let Some(path) = &app.env_file {
        out.extend(parse_kv_lines(&read(path)?));
    }

    for (var, path) in &app.env_from_files {
        // Trim ca newline cuoi file lan khoang trang thua: file token thuong
        // duoc ghi kem newline, ma newline trong header HTTP se lam hong request.
        out.insert(var.clone(), read(path)?.trim().to_string());
    }

    out.extend(app.env.iter().map(|(k, v)| (k.clone(), v.clone())));
    Ok(out)
}

fn read(path: &Path) -> Result<String, EnvError> {
    std::fs::read_to_string(path).map_err(|source| EnvError::Unreadable {
        path: path.to_path_buf(),
        source,
    })
}

/// Parse dang `KEY=VALUE` moi dong. Bo qua dong trong va dong comment.
/// Dong khong co dau `=` bi bo qua thay vi lam hong ca file.
///
/// Cong khai de UI dung lai khi doc o nhap env nhieu dong, thay vi viet lai
/// mot bo parse thu hai co the lech hanh vi.
pub fn parse_kv_lines(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            if key.is_empty() {
                return None;
            }
            Some((key.to_string(), unquote(value.trim()).to_string()))
        })
        .collect()
}

/// Bo cap nhay bao quanh gia tri neu co. Chi bo khi ca hai dau deu la nhay
/// cung loai, de gia tri chua nhay le van giu nguyen.
fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' || first == b'\'') && first == last {
            return &value[1..value.len() - 1];
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, content: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sam-env-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    #[test]
    fn bo_qua_dong_trong_va_comment() {
        let parsed = parse_kv_lines("# comment\n\nA=1\n  \n# B=2\nC=3\n");
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed["A"], "1");
        assert_eq!(parsed["C"], "3");
        assert!(!parsed.contains_key("B"));
    }

    #[test]
    fn giu_nguyen_dau_bang_trong_gia_tri() {
        let parsed = parse_kv_lines("URL=http://x/?a=1&b=2\n");
        assert_eq!(parsed["URL"], "http://x/?a=1&b=2");
    }

    #[test]
    fn bo_cap_nhay_bao_quanh_nhung_giu_nhay_le() {
        assert_eq!(parse_kv_lines("A=\"x y\"\n")["A"], "x y");
        assert_eq!(parse_kv_lines("A='x y'\n")["A"], "x y");
        assert_eq!(parse_kv_lines("A=\"x\n")["A"], "\"x");
    }

    #[test]
    fn dong_khong_co_dau_bang_bi_bo_qua() {
        let parsed = parse_kv_lines("rac\nA=1\n");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed["A"], "1");
    }

    #[test]
    fn env_from_files_lay_toan_bo_noi_dung_da_trim() {
        // File token that thuong co newline cuoi; newline lot vao header HTTP
        // se lam hong request nen bat buoc phai trim.
        let token = temp_file("token", "  secret-abc123\r\n");
        let app = ManagedApp {
            env_from_files: [("TOKEN".to_string(), token.clone())].into(),
            ..Default::default()
        };

        let env = resolve(&app).unwrap();
        assert_eq!(env["TOKEN"], "secret-abc123");
        // File nguon khong bi sua.
        assert_eq!(std::fs::read_to_string(&token).unwrap(), "  secret-abc123\r\n");
        std::fs::remove_file(token).ok();
    }

    #[test]
    fn thu_tu_uu_tien_env_inline_thang() {
        let file = temp_file("prio", "V=tu-env-file\nONLY_FILE=x\n");
        let from_file = temp_file("prio-raw", "tu-env-from-files");

        let app = ManagedApp {
            env_file: Some(file.clone()),
            env_from_files: [("V".to_string(), from_file.clone())].into(),
            env: [("V".to_string(), "tu-env-inline".to_string())].into(),
            ..Default::default()
        };

        let env = resolve(&app).unwrap();
        assert_eq!(env["V"], "tu-env-inline");
        assert_eq!(env["ONLY_FILE"], "x");

        // Bo env inline: env_from_files phai thang env_file.
        let app = ManagedApp {
            env_file: Some(file.clone()),
            env_from_files: [("V".to_string(), from_file.clone())].into(),
            ..Default::default()
        };
        assert_eq!(resolve(&app).unwrap()["V"], "tu-env-from-files");

        std::fs::remove_file(file).ok();
        std::fs::remove_file(from_file).ok();
    }

    #[test]
    fn file_thieu_bao_loi_ro_rang_khong_panic() {
        let app = ManagedApp {
            env_from_files: [("T".to_string(), PathBuf::from("Z:/khong/ton/tai"))].into(),
            ..Default::default()
        };
        let err = resolve(&app).unwrap_err();
        assert!(err.to_string().contains("khong/ton/tai"), "{err}");
    }

    #[test]
    fn khong_khai_bao_gi_thi_env_rong() {
        assert!(resolve(&ManagedApp::default()).unwrap().is_empty());
    }
}

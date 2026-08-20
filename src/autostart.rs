//! Tu khoi dong cung Windows qua `HKCU\...\Run`.
//!
//! Dung Run key thay vi Task Scheduler vi no khong doi quyen admin. Doi lai
//! app chi chay sau khi user dang nhap, dieu nay chap nhan duoc voi cac app
//! chay o phien lam viec cua user.

use std::io;
use std::path::{Path, PathBuf};

use winreg::enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE};
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "StartupAppManager";

/// Co `--tray` de lan khoi dong tu dong chi hien bieu tuong khay, khong bung
/// cua so quan ly vao mat user moi lan dang nhap.
const TRAY_FLAG: &str = "--tray";

pub fn is_enabled() -> bool {
    is_enabled_for(VALUE_NAME)
}

pub fn enable() -> io::Result<()> {
    enable_for(VALUE_NAME)
}

pub fn disable() -> io::Result<()> {
    disable_for(VALUE_NAME)
}

/// Dong lenh se duoc ghi vao registry.
///
/// Duong dan luon duoc boc nhay kep vi no thuong chua dau cach
/// (`C:\Program Files\...`); thieu nhay thi Windows se chay nham file.
pub fn command_line() -> io::Result<String> {
    Ok(format_command(&std::env::current_exe()?))
}

fn format_command(exe: &Path) -> String {
    format!("\"{}\" {TRAY_FLAG}", exe.display())
}

fn run_key(write: bool) -> io::Result<RegKey> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let access = if write { KEY_READ | KEY_WRITE } else { KEY_READ };
    // Run key luon ton tai tren Windows, nhung dung create de an toan neu
    // profile user bi cat xen.
    hkcu.create_subkey_with_flags(RUN_KEY_PATH, access)
        .map(|(key, _)| key)
}

/// Chi bao `true` khi muc ghi trong registry tro dung file thuc thi hien tai.
///
/// Neu app da bi di chuyen, muc cu tro vao duong dan khong con ton tai va se
/// khong bao gio chay; bao `false` de user bat lai cho dung duong dan moi.
fn is_enabled_for(value_name: &str) -> bool {
    let Ok(key) = run_key(false) else {
        return false;
    };
    let Ok(stored) = key.get_value::<String, _>(value_name) else {
        return false;
    };
    let Ok(current) = std::env::current_exe() else {
        return false;
    };
    match exe_from_command(&stored) {
        Some(path) => paths_equal(&path, &current),
        None => false,
    }
}

fn enable_for(value_name: &str) -> io::Result<()> {
    let key = run_key(true)?;
    key.set_value(value_name, &command_line()?)
}

fn disable_for(value_name: &str) -> io::Result<()> {
    let key = run_key(true)?;
    match key.delete_value(value_name) {
        Ok(()) => Ok(()),
        // Xoa mot muc von khong ton tai la ket qua mong muon, khong phai loi.
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Tach duong dan file thuc thi ra khoi dong lenh da luu.
fn exe_from_command(command: &str) -> Option<PathBuf> {
    let command = command.trim();
    if let Some(rest) = command.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some(PathBuf::from(&rest[..end]));
    }
    // Dong lenh khong boc nhay: lay den dau cach dau tien. Khong khoi phuc
    // duoc duong dan co dau cach, nhung do la muc do chinh xac toi da co the.
    let first = command.split_whitespace().next()?;
    Some(PathBuf::from(first))
}

/// So sanh duong dan khong phan biet hoa thuong, dung quy uoc cua Windows.
fn paths_equal(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        // `canonicalize` that bai neu file khong con ton tai; khi do so sanh
        // tren chuoi van cho ket qua dung trong da so truong hop.
        std::fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .to_lowercase()
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test ghi vao registry that nen dung ten muc rieng de khong dung cham
    /// cau hinh that cua user. Moi test lai co ten rieng: chung chay song song
    /// nen dung chung mot muc thi buoc don dep cua test nay se xoa mat gia tri
    /// ma test kia dang kiem tra.
    fn test_value(tag: &str) -> String {
        format!("StartupAppManager_Test_{tag}")
    }

    /// Don dep ke ca khi test that bai giua chung.
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            disable_for(&self.0).ok();
        }
    }

    #[test]
    fn format_command_boc_nhay_duong_dan_co_dau_cach() {
        let cmd = format_command(Path::new(r"C:\Program Files\a b\sam.exe"));
        assert_eq!(cmd, r#""C:\Program Files\a b\sam.exe" --tray"#);
    }

    #[test]
    fn exe_from_command_doc_duoc_duong_dan_co_nhay() {
        let p = exe_from_command(r#""C:\Program Files\a b\sam.exe" --tray"#).unwrap();
        assert_eq!(p, PathBuf::from(r"C:\Program Files\a b\sam.exe"));
    }

    #[test]
    fn exe_from_command_doc_duoc_duong_dan_khong_nhay() {
        let p = exe_from_command(r"C:\tools\sam.exe --tray").unwrap();
        assert_eq!(p, PathBuf::from(r"C:\tools\sam.exe"));
    }

    #[test]
    fn exe_from_command_voi_chuoi_rac() {
        assert_eq!(exe_from_command(""), None);
        assert_eq!(exe_from_command("\"chua dong nhay"), None);
    }

    #[test]
    fn paths_equal_khong_phan_biet_hoa_thuong() {
        assert!(paths_equal(
            Path::new(r"C:\Tools\SAM.exe"),
            Path::new(r"c:\tools\sam.exe")
        ));
        assert!(!paths_equal(
            Path::new(r"C:\Tools\sam.exe"),
            Path::new(r"C:\Other\sam.exe")
        ));
    }

    #[test]
    fn bat_roi_tat_lam_thay_doi_trang_thai_that() {
        let value = test_value("toggle");
        let _cleanup = Cleanup(value.clone());

        disable_for(&value).unwrap();
        assert!(!is_enabled_for(&value));

        enable_for(&value).unwrap();
        assert!(is_enabled_for(&value), "sau khi bat phai doc lai duoc");

        // Gia tri ghi ra phai chay dung file thuc thi hien tai kem co --tray.
        let key = run_key(false).unwrap();
        let stored: String = key.get_value(&value).unwrap();
        assert!(stored.ends_with(TRAY_FLAG), "{stored}");
        assert!(stored.starts_with('"'), "duong dan phai duoc boc nhay: {stored}");

        disable_for(&value).unwrap();
        assert!(!is_enabled_for(&value));
    }

    #[test]
    fn tat_hai_lan_khong_bao_loi() {
        let value = test_value("tat-hai-lan");
        let _cleanup = Cleanup(value.clone());
        disable_for(&value).unwrap();
        disable_for(&value).expect("xoa muc khong ton tai phai la khong loi");
    }

    #[test]
    fn muc_tro_sang_exe_khac_bi_coi_la_tat() {
        let value = test_value("da-di-chuyen");
        let _cleanup = Cleanup(value.clone());

        // Mo phong truong hop app da bi di chuyen di noi khac.
        let key = run_key(true).unwrap();
        key.set_value(&value, &r#""C:\khong\ton\tai\sam.exe" --tray"#.to_string())
            .unwrap();

        assert!(
            !is_enabled_for(&value),
            "muc tro sai duong dan se khong bao gio chay, phai bao la tat"
        );
    }
}

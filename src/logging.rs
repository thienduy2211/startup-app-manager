//! Log cua ban than manager: ghi noi tiep, tu cat khi qua lon.
//!
//! Khong dung crate log de giu binary nho. Ghi that bai thi im lang bo qua:
//! khong ghi duoc log khong phai ly do de manager ngung giam sat service.

#[cfg(not(test))]
use std::io::Write;
use std::path::Path;
#[cfg(not(test))]
use std::sync::Mutex;

/// Vuot nguong nay thi file hien tai duoc doi thanh `.1` va bat dau file moi.
#[cfg(not(test))]
const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[cfg(not(test))]
static LOG_LOCK: Mutex<()> = Mutex::new(());

pub fn info(message: &str) {
    write_line("INFO", message);
}

pub fn warn(message: &str) {
    write_line("WARN", message);
}

pub fn error(message: &str) {
    write_line("ERROR", message);
}

/// Unit test cua lib khong duoc cham vao log that cua user.
///
/// Chung khong doi `%APPDATA%` duoc -- doi bien moi truong giua cac test chay
/// song song la dua voi nhau -- nen khong chan o day thi moi lan `cargo test`
/// lai ghi them vao dung `manager.log` tren may user, va con co the lam
/// `rotate_if_needed` doi log that su cua ho thanh `.1`. Test tich hop chay o
/// binary rieng nen khong dinh `cfg(test)` cua lib; chung tu tro `%APPDATA%`
/// sang thu muc tam.
#[cfg(test)]
fn write_line(_level: &str, _message: &str) {}

#[cfg(not(test))]
fn write_line(level: &str, message: &str) {
    let _guard = match LOG_LOCK.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    let path = crate::paths::manager_log_file();
    if let Some(parent) = path.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }

    rotate_if_needed(&path, MAX_LOG_BYTES);

    let line = format!("[{}] {level} {message}\n", timestamp());
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Doi file qua lon thanh `.1` va bat dau file moi.
///
/// Dung chung voi log rieng cua tung app: chung con on hon log cua manager vi
/// chua toan bo stdout/stderr cua tien trinh con, va khong co gi cat bot thi
/// mot service noi nhieu se lam day o dia cua user.
pub fn rotate_if_needed(path: &Path, max_bytes: u64) {
    let too_big = std::fs::metadata(path).map(|m| m.len() >= max_bytes);
    if !matches!(too_big, Ok(true)) {
        return;
    }
    // Chi giu mot ban luu: du de xem lai lich su gan nhat ma khong phinh dia.
    let backup = path.with_extension("log.1");
    let _ = std::fs::remove_file(&backup);
    let _ = std::fs::rename(path, &backup);
}

/// Dau thoi gian dang `YYYY-MM-DD HH:MM:SS` theo gio UTC.
///
/// Tu tinh tu Unix epoch de khong phai keo them crate thoi gian. Dung UTC vi
/// doc mui gio local can goi Win32, khong dang cho mot dong log.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let (year, month, day) = civil_from_days((secs / 86_400) as i64);
    let tod = secs % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Thuat toan civil_from_days cua Howard Hinnant: doi so ngay tinh tu
/// 1970-01-01 thanh (nam, thang, ngay).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_dung_o_cac_moc_da_biet() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        // 2024 la nam nhuan: ngay 29/02 phai ton tai.
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
        assert_eq!(civil_from_days(20_634), (2026, 6, 30));
    }

    #[test]
    fn timestamp_dung_dinh_dang_co_dinh() {
        let ts = timestamp();
        assert_eq!(ts.len(), 20, "{ts}");  // 2026-08-19 03:11:15Z
        assert!(ts.ends_with('Z'), "{ts}");
        assert_eq!(ts.as_bytes()[4], b'-');
        assert_eq!(ts.as_bytes()[10], b' ');
        assert_eq!(ts.as_bytes()[13], b':');
    }
}

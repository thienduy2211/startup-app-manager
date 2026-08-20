//! Chan chay hai ban manager cung luc.
//!
//! Hai ban chay song song se cung giam sat mot app: ca hai deu thay app chet,
//! ca hai deu sinh lai, va so tien trinh nhan doi. Named mutex la cach re
//! nhat de OS bao cho ban thu hai biet no den sau.

use std::io;

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, HANDLE};
use windows_sys::Win32::System::Threading::CreateMutexW;

/// Dung namespace `Local\` chu khong phai `Global\`: gioi han trong phien lam
/// viec hien tai, nho vay nhieu user tren cung mot may van chay duoc ban rieng.
const MUTEX_NAME: &str = r"Local\StartupAppManager.SingleInstance";

/// Giu cho den khi tien trinh ket thuc. Tha ra la nhuong cho ban khac.
pub struct InstanceGuard {
    handle: HANDLE,
}

// HANDLE la con tro tho nen khong tu dong Send; mutex handle khong gan voi
// thread nao ca.
unsafe impl Send for InstanceGuard {}
unsafe impl Sync for InstanceGuard {}

/// Vi sao khong lay duoc quyen chay. Hai ly do nay dan den hai thong bao khac
/// nhau: mot cai la binh thuong, cai kia la loi he thong can bao ro.
#[derive(Debug)]
pub enum AcquireError {
    /// Da co mot ban dang chay.
    AlreadyRunning,
    /// Khong tao noi mutex. Bao loi thay vi im lang coi nhu "da chay roi".
    Failed(io::Error),
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcquireError::AlreadyRunning => write!(f, "another instance is already running"),
            AcquireError::Failed(e) => write!(f, "cannot create mutex: {e}"),
        }
    }
}

pub fn acquire() -> Result<InstanceGuard, AcquireError> {
    acquire_named(MUTEX_NAME)
}

fn acquire_named(name: &str) -> Result<InstanceGuard, AcquireError> {
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();

    // SAFETY: `wide` la chuoi UTF-16 ket thuc bang null, con song trong suot
    // loi goi. Truyen null cho security attributes la hop le.
    let handle = unsafe { CreateMutexW(std::ptr::null(), 1, wide.as_ptr()) };

    // Handle van duoc tra ve ngay ca khi mutex da ton tai, nen phai hoi
    // GetLastError truoc khi ket luan.
    let last = io::Error::last_os_error();

    if handle.is_null() {
        return Err(AcquireError::Failed(last));
    }
    if last.raw_os_error() == Some(ERROR_ALREADY_EXISTS as i32) {
        // SAFETY: handle vua tao va chua duoc dong lan nao.
        unsafe { CloseHandle(handle) };
        return Err(AcquireError::AlreadyRunning);
    }
    Ok(InstanceGuard { handle })
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // SAFETY: handle chi duoc dong dung mot lan, tai day.
        unsafe { CloseHandle(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_name(tag: &str) -> String {
        format!(
            r"Local\StartupAppManagerTest.{}.{}.{tag}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        )
    }

    #[test]
    fn ban_dau_tien_lay_duoc_ban_thu_hai_thi_khong() {
        let name = unique_name("doi");
        let first = acquire_named(&name).expect("ban dau tien phai lay duoc");
        assert!(
            matches!(acquire_named(&name), Err(AcquireError::AlreadyRunning)),
            "ban thu hai phai bi tu choi, neu khong ca hai se cung sinh lai app"
        );
        drop(first);
    }

    #[test]
    fn tha_ra_roi_thi_ban_sau_lay_duoc() {
        let name = unique_name("nhuong");
        let first = acquire_named(&name).unwrap();
        drop(first);

        let second = acquire_named(&name);
        assert!(second.is_ok(), "sau khi ban truoc thoat, ban sau phai vao duoc");
    }

    #[test]
    fn ten_khac_nhau_khong_dung_cham() {
        let a = acquire_named(&unique_name("a")).unwrap();
        let b = acquire_named(&unique_name("b"));
        assert!(b.is_ok(), "hai ten khac nhau la hai mutex doc lap");
        drop(a);
    }
}

//! Bat thong bao Explorer khoi dong lai.
//!
//! Windows khong tu ve lai bieu tuong khay: sau khi Explorer song lai, moi app
//! phai tu them bieu tuong cua minh. nwg 1.0.13 khong lam viec do, va voi mot
//! app chi co duy nhat loi vao o khay thi mat bieu tuong la mat luon duong
//! vao -- ban thu hai bi khoa boi mutex don the va chi ke lai "xem bieu tuong
//! o khay", con user chi con cach giet manager tu Task Manager, ma
//! `KILL_ON_JOB_CLOSE` keo theo moi service dang duoc giam sat.

use windows_sys::Win32::UI::WindowsAndMessaging::RegisterWindowMessageW;

/// Id cua message `TaskbarCreated`, thu Explorer phat lai cho moi cua so
/// top-level moi lan taskbar duoc dung lai.
///
/// Tra `0` khi dang ky that bai. Nguoi goi phai coi `0` la "khong co message
/// nao khop": `WM_NULL` mang dung so do va den lien tuc.
pub fn taskbar_created_message() -> u32 {
    let name: Vec<u16> = "TaskbarCreated\0".encode_utf16().collect();
    // SAFETY: `name` la chuoi UTF-16 ket thuc bang NUL va con song het loi
    // goi. Ham chi doc chuoi roi tra ve mot id, khong giu lai con tro nao.
    unsafe { RegisterWindowMessageW(name.as_ptr()) }
}

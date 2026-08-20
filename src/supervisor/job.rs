//! Boc Win32 Job Object trong API an toan.
//!
//! Job Object phuc vu hai viec, va la ly do ca hai deu dung duoc voi target
//! kieu `.cmd` bung tien trinh con roi tu thoat:
//!
//! 1. **Do song chet**: dem so tien trinh con song trong job, thay vi theo doi
//!    tien trinh truc tiep. Mot `.cmd` chay `start /b node ...` se thoat ngay
//!    trong khi node van chay; theo doi tien trinh truc tiep se bao "da chet"
//!    va lam supervisor sinh ra ban sao khong gioi han.
//! 2. **Don rac**: co `KILL_ON_JOB_CLOSE` khien OS giet ca cay tien trinh khi
//!    handle dong, ke ca luc manager bi force-kill.
//!
//! Day la file duy nhat trong du an dung `unsafe`.

use std::io;
use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
};
use windows_sys::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// Snapshot thread hong tam thoi la chuyen binh thuong tren may dang ban.
const SNAPSHOT_RETRIES: u32 = 5;
const SNAPSHOT_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(20);

pub struct Job {
    handle: HANDLE,
}

// HANDLE la con tro tho nen khong tu dong Send. Handle cua job khong gan voi
// thread nao: moi thread deu goi duoc cac Win32 API tren no. Supervisor chay o
// thread rieng nen can chuyen quyen so huu qua.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    /// Tao job moi da bat `KILL_ON_JOB_CLOSE`.
    pub fn new() -> io::Result<Self> {
        // SAFETY: truyen null cho ca hai tham so tuy chon (security attributes
        // va ten job) la hop le; job khong ten nen khong dung do voi tien trinh khac.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }

        let job = Job { handle };

        // SAFETY: `info` duoc zero-init dung kieu ma Win32 mong doi, va kich
        // thuoc truyen vao khop voi kieu do.
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = unsafe {
            SetInformationJobObject(
                job.handle,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// Dua tien trinh con vao job. Moi tien trinh chau sinh ra sau do tu dong
    /// thua ke job, nen ca cay deu duoc theo doi.
    pub fn assign(&self, child: &Child) -> io::Result<()> {
        // SAFETY: handle lay tu `Child` con song trong suot loi goi nay.
        let ok = unsafe { AssignProcessToJobObject(self.handle, child.as_raw_handle() as HANDLE) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Danh thuc tien trinh duoc tao voi `CREATE_SUSPENDED`.
    ///
    /// Phai co buoc nay vi `assign` chi an toan khi tien trinh chua chay dong
    /// lenh nao: mot `.cmd` duoc tha ra truoc khi gan job co the kip bung tien
    /// trinh chau nam **ngoai** job. Khi do cmd.exe thoat, so dem ve 0,
    /// supervisor ket luan "da chet" va sinh them mot ban nua -- dung kieu nhan
    /// ban ma Job Object sinh ra de chan.
    ///
    /// `std::process::Child` khong lo handle cua thread chinh ra ngoai nen phai
    /// tim lai qua snapshot thread cua he thong.
    pub fn resume(child: &Child) -> io::Result<()> {
        let pid = child.id();
        let mut last = None;

        // `CreateToolhelp32Snapshot` va `Thread32First` hong tam thoi bang
        // ERROR_BAD_LENGTH khi he thong dang ban tao/huy thread; cach xu ly
        // duoc khuyen nghi la thu lai. Khong thu lai thi mot lan xui se dua
        // mot service hoan toan khoe manh vao backoff, va du vai lan la
        // CrashLooping -- phai bam tay moi song lai.
        for attempt in 0..SNAPSHOT_RETRIES {
            match Self::resume_once(pid) {
                Ok(()) => return Ok(()),
                Err(e) => last = Some(e),
            }
            if attempt + 1 < SNAPSHOT_RETRIES {
                std::thread::sleep(SNAPSHOT_RETRY_DELAY);
            }
        }
        Err(last.unwrap_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "cannot resume the process")
        }))
    }

    fn resume_once(pid: u32) -> io::Result<()> {
        // SAFETY: snapshot toan he thong, khong nhan con tro nao.
        let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
        if snapshot == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        let snapshot = OwnedHandle(snapshot);

        // SAFETY: `entry` zero-init dung kieu; Win32 doi hoi `dwSize` duoc dat
        // truoc khi goi, va con tro tro toi bien con song trong suot vong lap.
        let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
        entry.dwSize = std::mem::size_of::<THREADENTRY32>() as u32;

        let mut found = unsafe { Thread32First(snapshot.0, &mut entry) };
        if found == 0 {
            return Err(io::Error::last_os_error());
        }

        let mut resumed = 0u32;
        while found != 0 {
            if entry.th32OwnerProcessID == pid {
                // SAFETY: chi xin quyen suspend/resume tren mot thread id lay
                // tu chinh snapshot nay.
                let thread = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) };
                if !thread.is_null() {
                    // `ResumeThread` tra u32::MAX khi loi.
                    if unsafe { ResumeThread(thread) } != u32::MAX {
                        resumed += 1;
                    }
                    unsafe { CloseHandle(thread) };
                }
            }
            found = unsafe { Thread32Next(snapshot.0, &mut entry) };
        }

        if resumed == 0 {
            // Tien trinh dang treo vinh vien; ben goi phai giet no di.
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "no thread found to resume",
            ));
        }
        Ok(())
    }

    /// So tien trinh dang song trong job. `0` nghia la app da chet han.
    pub fn active_processes(&self) -> io::Result<u32> {
        // SAFETY: `acct` zero-init dung kieu, kich thuoc khop, con tro hop le.
        let mut acct: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle,
                JobObjectBasicAccountingInformation,
                std::ptr::addr_of_mut!(acct).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(acct.ActiveProcesses)
    }

    /// Giet toan bo cay tien trinh trong job.
    pub fn terminate(&self) -> io::Result<()> {
        // SAFETY: handle hop le trong suot doi song cua `self`.
        let ok = unsafe { TerminateJobObject(self.handle, 1) };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        // Dong handle kich hoat KILL_ON_JOB_CLOSE: app con chet theo manager.
        // SAFETY: handle chi duoc dong dung mot lan, tai day.
        unsafe { CloseHandle(self.handle) };
    }
}

/// Dong handle khi ra khoi pham vi, ke ca tren duong loi.
struct OwnedHandle(HANDLE);

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // SAFETY: handle chi duoc dong dung mot lan, tai day.
        unsafe { CloseHandle(self.0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::Duration;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    /// Tien trinh song vai giay de do dac.
    fn spawn_sleeper(secs: u32) -> Child {
        Command::new("cmd")
            .args(["/c", &format!("ping -n {secs} 127.0.0.1")])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn duoc cmd")
    }

    #[test]
    fn job_rong_khong_co_tien_trinh_nao() {
        let job = Job::new().unwrap();
        assert_eq!(job.active_processes().unwrap(), 0);
    }

    #[test]
    fn dem_duoc_tien_trinh_sau_khi_assign() {
        let job = Job::new().unwrap();
        let mut child = spawn_sleeper(10);
        job.assign(&child).unwrap();

        assert!(job.active_processes().unwrap() >= 1);

        job.terminate().unwrap();
        let _ = child.wait();
        assert_eq!(job.active_processes().unwrap(), 0, "terminate phai don sach");
    }

    #[test]
    fn tien_trinh_thoat_lam_active_ve_khong() {
        let job = Job::new().unwrap();
        let mut child = Command::new("cmd")
            .args(["/c", "exit 0"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        job.assign(&child).unwrap();
        let _ = child.wait();

        // Cho OS cap nhat so lieu job.
        let mut active = job.active_processes().unwrap();
        for _ in 0..50 {
            if active == 0 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
            active = job.active_processes().unwrap();
        }
        assert_eq!(active, 0);
    }

    #[test]
    fn tien_trinh_chau_thua_ke_job() {
        // cmd.exe sinh ra mot ping con: ca hai deu phai nam trong job.
        let job = Job::new().unwrap();
        let mut child = spawn_sleeper(10);
        job.assign(&child).unwrap();

        let mut peak = 0;
        for _ in 0..50 {
            peak = peak.max(job.active_processes().unwrap());
            if peak >= 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(peak >= 2, "tien trinh chau phai thua ke job, peak={peak}");

        job.terminate().unwrap();
        let _ = child.wait();
    }
}

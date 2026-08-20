//! A9 - bang chung cho quyet dinh kien truc quan trong nhat cua du an.
//!
//! Mot script `.cmd` co the bung tien trinh con roi tu thoat ngay. Khi do:
//!
//! - theo doi tien trinh truc tiep (`Child::try_wait`) bao "da chet"
//! - dem tien trinh trong Job Object bao "con song" -- va day moi la su that
//!
//! Neu supervisor tin vao `try_wait`, no se sinh lai app o moi chu ky trong
//! khi ban cu van chay, tao ra ban sao khong gioi han. Test nay chot lai hanh
//! vi do de khong ai vo tinh doi nguoc lai sau nay.

use std::io::Write;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use startup_app_manager::supervisor::job::Job;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CREATE_SUSPENDED: u32 = 0x0000_0004;

struct Fixture {
    dir: PathBuf,
}
impl Fixture {
    /// Tao thu muc tam chua cac script `.cmd` dung cho test.
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "sam-detach-{}-{}-{tag}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
                + std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Fixture { dir }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn write_cmd(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        // File .cmd can xuong dong kieu CRLF de cmd.exe doc dung.
        for line in body.lines() {
            write!(f, "{line}\r\n").unwrap();
        }
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// Tao tien trinh dung thu tu ma production dung: treo -> gan job -> tha ra.
///
/// Tha truoc roi moi gan thi script co the kip bung tien trinh chau nam ngoai
/// job, va test se do trong khi loi do van con.
fn spawn_hidden(script: &PathBuf, job: &Job) -> std::process::Child {
    let child = Command::new(script)
        .creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn duoc script .cmd");
    job.assign(&child).expect("gan duoc vao job");
    Job::resume(&child).expect("danh thuc duoc tien trinh");
    child
}

fn line_count(path: &PathBuf) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// Cho toi khi dieu kien dung, hoac het thoi gian.
fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    cond()
}

#[test]
fn cmd_kieu_detach_van_duoc_tinh_la_dang_song() {
    let fx = Fixture::new("detach");
    // `start /b` bung ping ra chay rieng roi cmd.exe thoat ngay lap tuc.
    let script = fx.write_cmd(
        "detach.cmd",
        "@echo off\r\nstart \"\" /b ping -n 60 127.0.0.1 >nul",
    );

    let job = Job::new().unwrap();
    let mut child = spawn_hidden(&script, &job);

    // cmd.exe thoat rat nhanh vi no khong cho ping.
    let cmd_exited = wait_until(Duration::from_secs(10), || {
        matches!(child.try_wait(), Ok(Some(_)))
    });
    assert!(cmd_exited, "cmd.exe kieu detach phai thoat ngay");

    // Day la trong tam: tien trinh truc tiep da chet, nhung app van dang chay.
    let active = job.active_processes().unwrap();
    assert!(
        active >= 1,
        "job phai con tien trinh chau dang song, active={active}"
    );

    job.terminate().unwrap();
    assert!(
        wait_until(Duration::from_secs(10), || job
            .active_processes()
            .unwrap()
            == 0),
        "terminate phai don sach ca cay tien trinh"
    );
}

#[test]
fn cmd_kieu_wait_cung_duoc_tinh_la_dang_song() {
    let fx = Fixture::new("wait");
    // Kieu doi lap: cmd.exe cho tien trinh con, ca hai cung nam trong job.
    let script = fx.write_cmd("wait.cmd", "@echo off\r\nping -n 60 127.0.0.1 >nul");

    let job = Job::new().unwrap();
    let mut child = spawn_hidden(&script, &job);

    assert!(
        wait_until(Duration::from_secs(10), || job
            .active_processes()
            .unwrap()
            >= 2),
        "ca cmd.exe lan ping deu phai nam trong job"
    );
    assert!(matches!(child.try_wait(), Ok(None)), "cmd.exe phai con cho");

    job.terminate().unwrap();
    let _ = child.wait();
}

#[test]
fn app_that_su_chet_thi_job_ve_khong() {
    // Doi chung: dam bao phep do khong phai lue nao cung bao "con song".
    let fx = Fixture::new("exit");
    let script = fx.write_cmd("die.cmd", "@echo off\r\nexit 1");

    let job = Job::new().unwrap();
    let mut child = spawn_hidden(&script, &job);
    let _ = child.wait();

    assert!(
        wait_until(Duration::from_secs(10), || job
            .active_processes()
            .unwrap()
            == 0),
        "app thoat han thi job phai rong"
    );
}

#[test]
fn dong_job_giet_ca_cay_tien_trinh() {
    // A8 - manager thoat hoac bi force-kill thi khong de lai tien trinh mo coi.
    let fx = Fixture::new("killonclose");
    let beat = fx.path("beat.txt");
    // Tien trinh chau ghi nhip lien tuc: con ghi la con song. Kiem tra mot job
    // rong xem co bao nhieu tien trinh thi khong chung minh duoc gi -- job rong
    // luon tra 0, ke ca khi co KILL_ON_JOB_CLOSE bi go bo.
    let script = fx.write_cmd(
        "detach.cmd",
        &format!(
            "@echo off\r\nstart \"\" /b cmd /c \"for /l %%i in (1,1,600) do (echo beat>>\"{}\" & timeout /t 1 /nobreak >nul)\"",
            beat.display()
        ),
    );

    let mut child;
    {
        let job = Job::new().unwrap();
        child = spawn_hidden(&script, &job);
        assert!(
            wait_until(Duration::from_secs(10), || line_count(&beat) >= 2),
            "tien trinh chau phai ghi duoc nhip truoc khi dong job"
        );
        // Job bi drop o cuoi block -> CloseHandle -> KILL_ON_JOB_CLOSE.
    }
    let _ = child.wait();

    // Cho cay tien trinh chet han, roi chot moc va xac nhan khong ai ghi them.
    std::thread::sleep(Duration::from_secs(2));
    let after_close = line_count(&beat);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        line_count(&beat),
        after_close,
        "van con tien trinh chau ghi nhip sau khi dong job: con mo coi"
    );
}

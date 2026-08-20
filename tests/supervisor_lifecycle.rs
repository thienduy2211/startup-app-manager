//! Nghiem thu vong doi cua supervisor tren tien trinh that (A3, A5, A6, A8,
//! A11, A12, A13).
//!
//! Cac test nay chay `Supervisor` day du voi tien trinh Windows that chu khong
//! gia lap, vi phan de sai nhat cua app nam dung o cho code Rust gap he dieu
//! hanh. Doi lai chung phai cho theo dong ho that: chu ky kiem tra bi chan duoi
//! o `MIN_CHECK_INTERVAL_SECS` (10 giay) nen moi vong quan sat ton chung do.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, Once};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use startup_app_manager::config::model::MIN_CHECK_INTERVAL_SECS;
use startup_app_manager::config::{AppConfig, HealthCheck, ManagedApp, RestartPolicy};
use startup_app_manager::paths;
use startup_app_manager::supervisor::{AppStatus, Command, SharedStatus, StatusKind, Supervisor};

/// Mot chu ky quan sat cua supervisor, cong bien de tranh test bap benh.
const CYCLE: Duration = Duration::from_secs(11);

/// Doi config va log cua test sang thu muc tam.
///
/// `paths` doc `%APPDATA%` moi lan goi, nen doi bien nay la du de test khong
/// ghi de len du lieu that cua user dang chay app.
fn isolate_appdata() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let root = std::env::temp_dir().join(format!("sam-lifecycle-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_var("APPDATA", &root);
        paths::ensure_dirs().unwrap();
    });
}

struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        isolate_appdata();
        let dir = std::env::temp_dir().join(format!(
            "sam-life-{}-{tag}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Fixture { dir }
    }

    /// File `.cmd` phai xuong dong kieu CRLF de `cmd.exe` doc dung.
    fn write_cmd(&self, name: &str, body: &str) -> PathBuf {
        let path = self.dir.join(name);
        let mut file = std::fs::File::create(&path).unwrap();
        for line in body.lines() {
            write!(file, "{line}\r\n").unwrap();
        }
        path
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// Script chay mai, moi vong ghi them mot dong vao file nhip.
    ///
    /// Dung de phan biet "con song" voi "da bi giet" ma khong can biet PID.
    fn heartbeat_script(&self, name: &str, beat: &Path) -> PathBuf {
        self.write_cmd(
            name,
            &format!(
                ":loop\r\n\
                 echo tick >> \"{}\"\r\n\
                 ping -n 2 127.0.0.1 > nul\r\n\
                 goto loop",
                beat.display()
            ),
        )
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

struct Harness {
    tx: Sender<Command>,
    status: SharedStatus,
    worker: Option<JoinHandle<()>>,
}

impl Harness {
    fn start(apps: Vec<ManagedApp>) -> Self {
        isolate_appdata();
        let mut config = AppConfig::default();
        config.apps = apps;
        let status: SharedStatus = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel();
        let supervisor = Supervisor::new(config, Arc::clone(&status), Arc::new(|| {}));
        let worker = std::thread::spawn(move || supervisor.run(rx));
        Harness {
            tx,
            status,
            worker: Some(worker),
        }
    }

    fn snapshot(&self, id: u64) -> Option<AppStatus> {
        let guard = self.status.lock().unwrap();
        guard.iter().find(|s| s.id == id).cloned()
    }

    /// Cho den khi trang thai thoa dieu kien, hoac that bai kem trang thai
    /// cuoi cung de thong bao loi noi duoc ly do.
    fn wait_for(
        &self,
        id: u64,
        what: &str,
        timeout: Duration,
        pred: impl Fn(&AppStatus) -> bool,
    ) -> AppStatus {
        let deadline = Instant::now() + timeout;
        let mut last = None;
        while Instant::now() < deadline {
            if let Some(status) = self.snapshot(id) {
                if pred(&status) {
                    return status;
                }
                last = Some(status);
            }
            std::thread::sleep(Duration::from_millis(200));
        }
        panic!("qua {timeout:?} van chua {what}; trang thai cuoi: {last:?}");
    }

    fn shutdown(mut self) {
        self.stop_worker();
    }

    fn stop_worker(&mut self) {
        let _ = self.tx.send(Command::Shutdown);
        if let Some(worker) = self.worker.take() {
            let result = worker.join();
            // Panic o day khi mot assert da panic truoc do se thanh double
            // panic va lam abort ca tien trinh test, xoa mat thong bao loi
            // goc -- dung luc can no nhat.
            if result.is_err() && !std::thread::panicking() {
                panic!("supervisor panic");
            }
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Test that bai giua chung van phai don sach tien trinh con, neu khong
        // cac test sau se chay chung voi rac cua test truoc.
        self.stop_worker();
    }
}

fn app(id: u64, name: &str, exe: PathBuf) -> ManagedApp {
    ManagedApp {
        id,
        name: name.to_string(),
        exe,
        // Chu ky ngan nhat ma supervisor chap nhan; mac dinh 5 phut se khien
        // test cho vo ich.
        check_interval_secs: MIN_CHECK_INTERVAL_SECS,
        restart: RestartPolicy {
            // Backoff that (5 giay tro len) se keo dai test ma khong chung
            // minh them dieu gi; quy luat tang dan da co test rieng.
            max_retries: 0,
            backoff_base_secs: 1,
            backoff_max_secs: 1,
        },
        ..Default::default()
    }
}

fn line_count(path: &Path) -> usize {
    std::fs::read_to_string(path)
        .map(|s| s.lines().count())
        .unwrap_or(0)
}

/// A3 - app bi tam dung thi khong duoc dong den, ke ca khi no khong chay.
#[test]
fn app_tam_dung_khong_bao_gio_duoc_sinh_ra() {
    let fixture = Fixture::new("paused");
    let beat = fixture.path("beat.txt");
    let script = fixture.heartbeat_script("paused.cmd", &beat);

    let mut managed = app(301, "paused", script);
    managed.enabled = false;
    let harness = Harness::start(vec![managed]);

    let status = harness.wait_for(301, "bao la dang tam dung", CYCLE, |s| {
        s.kind == StatusKind::Paused
    });
    assert_eq!(status.active_procs, Some(0));

    // Qua han kiem tra dau tien ma van khong co gi duoc sinh ra.
    std::thread::sleep(CYCLE);
    let status = harness.snapshot(301).unwrap();
    assert_eq!(status.kind, StatusKind::Paused, "{status:?}");
    assert_eq!(status.launch_count, 0, "app tam dung ma van bi khoi dong");
    assert_eq!(line_count(&beat), 0, "app tam dung ma van chay");

    harness.shutdown();
}

/// A5 - app khong con thi tu duoc mo lai o chu ky ke tiep.
#[test]
fn app_chet_duoc_sinh_lai_o_chu_ky_ke_tiep() {
    let fixture = Fixture::new("respawn");
    // Script thoat ngay: den han kiem tra la job rong, dung canh "service
    // khong con" ma app nay sinh ra de xu ly.
    let script = fixture.write_cmd("exit-now.cmd", "@echo off\r\nexit /b 0");

    let harness = Harness::start(vec![app(501, "respawn", script)]);

    harness.wait_for(501, "duoc khoi dong lan dau", CYCLE, |s| {
        s.launch_count >= 1
    });
    // Chu ky kiem tra (10s) + backoff (1s) + bien.
    let status = harness.wait_for(501, "duoc sinh lai sau khi chet", CYCLE * 2, |s| {
        s.launch_count >= 2
    });
    assert_ne!(status.kind, StatusKind::CrashLooping);

    harness.shutdown();
}

/// A6 - hong lien tuc thi dung thu lai thay vi quay vong vo tan.
#[test]
fn hong_qua_so_lan_cho_phep_thi_ngung_thu_lai() {
    let fixture = Fixture::new("crashloop");
    let script = fixture.write_cmd("fail.cmd", "@echo off\r\nexit /b 1");

    let mut managed = app(601, "crashloop", script);
    // attempt > max_retries moi bo cuoc, nen 1 nghia la thu lai dung mot lan.
    managed.restart.max_retries = 1;
    let harness = Harness::start(vec![managed]);

    let status = harness.wait_for(601, "bo cuoc", CYCLE * 3, |s| {
        s.kind == StatusKind::CrashLooping
    });
    let attempts = status.launch_count;

    // Da bo cuoc thi khong duoc am tham thu tiep.
    std::thread::sleep(CYCLE);
    let after = harness.snapshot(601).unwrap();
    assert_eq!(after.kind, StatusKind::CrashLooping);
    assert_eq!(
        after.launch_count, attempts,
        "da bao bo cuoc ma van con khoi dong lai"
    );

    harness.shutdown();
}

/// A8 - thoat manager thi khong de lai tien trinh con nao.
#[test]
fn thoat_thi_giet_sach_cay_tien_trinh_con() {
    let fixture = Fixture::new("shutdown");
    let beat = fixture.path("beat.txt");
    let script = fixture.heartbeat_script("alive.cmd", &beat);

    let harness = Harness::start(vec![app(801, "alive", script)]);
    harness.wait_for(801, "chay len", CYCLE, |s| s.active_procs.is_some_and(|n| n >= 1));
    assert!(line_count(&beat) > 0, "script chua kip chay");

    harness.shutdown();

    // Sau khi manager thoat, nhip phai dung han. Doi du lau de mot vong ping
    // con sot lai cung da ket thuc.
    std::thread::sleep(Duration::from_secs(3));
    let after_exit = line_count(&beat);
    std::thread::sleep(Duration::from_secs(3));
    assert_eq!(
        line_count(&beat),
        after_exit,
        "van con tien trinh con chay sau khi manager thoat"
    );
}

/// A11 - stdout va stderr cua app con duoc giu lai de con truy nguyen nguyen nhan.
#[test]
fn stdout_va_stderr_cua_app_duoc_ghi_ra_log_rieng() {
    let fixture = Fixture::new("log");
    let script = fixture.write_cmd(
        "noisy.cmd",
        "@echo off\r\necho ra-stdout\r\necho ra-stderr 1>&2\r\nexit /b 3",
    );

    let log = paths::app_log_file(1101);
    std::fs::remove_file(&log).ok();

    let harness = Harness::start(vec![app(1101, "noisy", script)]);
    harness.wait_for(1101, "chay lan dau", CYCLE, |s| s.launch_count >= 1);
    std::thread::sleep(Duration::from_secs(2));

    let content = std::fs::read_to_string(&log).expect("phai co file log rieng cho app");
    assert!(content.contains("ra-stdout"), "{content}");
    assert!(content.contains("ra-stderr"), "{content}");

    harness.shutdown();
}

/// A12 - tien trinh con song nhung khong con phuc vu duoc thi van phai restart.
#[test]
fn health_check_hong_thi_restart_du_tien_trinh_van_song() {
    let fixture = Fixture::new("health");
    let beat = fixture.path("beat.txt");
    let script = fixture.heartbeat_script("serving.cmd", &beat);

    // Server gia luon tra 503: dung canh app treo ma tien trinh chua chet.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_signal = Arc::clone(&stop);
    // Dem so lan server that su tra loi: khong co no thi test nay van xanh ke
    // ca khi khong co server nao lang nghe, tuc la no khong chung minh duoc
    // "tien trinh con song nhung khong phuc vu duoc" nhu ten no noi.
    let served = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let hits = Arc::clone(&served);
    let server = std::thread::spawn(move || {
        for stream in listener.incoming() {
            if stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            let Ok(mut stream) = stream else { break };
            // Phai doc request truoc: Windows dong socket con du lieu chua doc
            // bang RST, va client co the mat luon phan hoi 503 -- khi do probe
            // bao Unreachable va test van xanh du server khong he tra 503.
            let mut req = [0u8; 1024];
            let _ = std::io::Read::read(&mut stream, &mut req);
            let _ = stream.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\n\r\n");
            hits.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    });

    let mut managed = app(1201, "unhealthy", script);
    managed.health = Some(HealthCheck {
        url: format!("http://127.0.0.1:{port}/health"),
        failures_before_restart: 1,
        ..Default::default()
    });
    let harness = Harness::start(vec![managed]);

    harness.wait_for(1201, "chay len", CYCLE, |s| s.launch_count >= 1);
    let status = harness.wait_for(1201, "bi restart vi health hong", CYCLE * 2, |s| {
        s.launch_count >= 2
    });
    // Tien trinh chua bao gio chet: bang chung la nhip van duoc ghi lien tuc.
    assert!(line_count(&beat) > 0);
    assert_ne!(status.kind, StatusKind::CrashLooping);
    assert!(
        served.load(std::sync::atomic::Ordering::Relaxed) >= 1,
        "server 503 chua he duoc goi: restart co the do mot ly do khac"
    );

    harness.shutdown();
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    // Danh thuc `accept` dang cho de thread server thoat duoc.
    let _ = std::net::TcpStream::connect(("127.0.0.1", port));
    server.join().ok();
}

/// A13 - env duoc bom day du, ke ca bien lay tu file rieng.
#[test]
fn env_duoc_bom_vao_tien_trinh_con() {
    let fixture = Fixture::new("env");
    let script = fixture.write_cmd("dump.cmd", "@echo off\r\nset\r\nexit /b 0");

    // File token thuong co newline cuoi; gia tri bom vao phai da duoc trim.
    let token_file = fixture.path("token.txt");
    std::fs::write(&token_file, "secret-abc123\r\n").unwrap();

    let env_file = fixture.path("app.env");
    std::fs::write(&env_file, "SAM_TU_FILE=gia-tri-file\n").unwrap();

    let log = paths::app_log_file(1301);
    std::fs::remove_file(&log).ok();

    let mut managed = app(1301, "env", script);
    managed.env = [("SAM_INLINE".to_string(), "xin-chao".to_string())].into();
    managed.env_file = Some(env_file);
    managed.env_from_files = [("SAM_TOKEN".to_string(), token_file)].into();

    let harness = Harness::start(vec![managed]);
    harness.wait_for(1301, "chay lan dau", CYCLE, |s| s.launch_count >= 1);
    std::thread::sleep(Duration::from_secs(2));

    let dump = std::fs::read_to_string(&log).expect("phai co log");
    assert!(dump.contains("SAM_INLINE=xin-chao"), "{dump}");
    assert!(dump.contains("SAM_TU_FILE=gia-tri-file"), "{dump}");
    assert!(
        dump.contains("SAM_TOKEN=secret-abc123"),
        "gia tri lay tu file phai duoc trim: {dump}"
    );
    // Bien cua he thong van con: app con can PATH de chay duoc.
    assert!(dump.contains("PATH="), "{dump}");

    harness.shutdown();
}

/// A10 - goi goi ma nguon Node bang ca hai kieu: tro thang vao file `.js`
/// (launch tu them `node`) va tro vao `node.exe` voi script trong tham so.
///
/// Bo qua khi may khong co Node: test nay xac minh su tich hop chu khong phai
/// logic cua supervisor, va logic do da co test rieng.
#[test]
fn chay_duoc_goi_node_bang_ca_hai_kieu_khai_bao() {
    let Some(node) = which("node.exe") else {
        eprintln!("bo qua: may nay khong co node");
        return;
    };

    let fixture = Fixture::new("node");
    let script = fixture.path("hello.js");
    std::fs::write(&script, "console.log('chao-tu-node');\n").unwrap();

    for (id, exe, args) in [
        (1001, script.clone(), String::new()),
        (1002, node.clone(), format!("\"{}\"", script.display())),
    ] {
        let log = paths::app_log_file(id);
        std::fs::remove_file(&log).ok();

        let mut managed = app(id, "node", exe);
        managed.args = args;
        let harness = Harness::start(vec![managed]);
        harness.wait_for(id, "chay lan dau", CYCLE, |s| s.launch_count >= 1);
        std::thread::sleep(Duration::from_secs(2));

        let out = std::fs::read_to_string(&log).unwrap_or_default();
        assert!(out.contains("chao-tu-node"), "app {id} khong chay duoc: {out}");
        harness.shutdown();
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// "Khoi dong" khong duoc dong den app dang chay, con "Khoi dong lai" thi phai
/// sinh ban moi.
///
/// Hai nut nay tung cung goi mot ham; khi do bam "Khoi dong" nham se giet mot
/// service dang phuc vu.
#[test]
fn start_khong_dung_toi_app_dang_chay_nhung_restart_thi_co() {
    let fixture = Fixture::new("start-vs-restart");
    let beat = fixture.path("beat.txt");
    let script = fixture.heartbeat_script("running.cmd", &beat);

    let harness = Harness::start(vec![app(1401, "start-restart", script)]);
    let before = harness.wait_for(1401, "chay len", CYCLE, |s| s.active_procs.is_some_and(|n| n >= 1));

    harness.tx.send(Command::StartNow(1401)).unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let after_start = harness.snapshot(1401).unwrap();
    assert_eq!(
        after_start.launch_count, before.launch_count,
        "app dang chay ma bam Khoi dong van bi sinh lai"
    );

    harness.tx.send(Command::RestartNow(1401)).unwrap();
    let after_restart = harness.wait_for(1401, "duoc sinh lai theo yeu cau", CYCLE, |s| {
        s.launch_count > before.launch_count
    });
    assert!(after_restart.active_procs.is_some_and(|n| n >= 1));

    harness.shutdown();
}

/// Nut "Dung" phai giu app o trang thai dung, chu khong chi giet no mot nhip.
///
/// Vong lap chay moi giay con chu ky quan sat la 10 giay, nen mot lan sinh lai
/// nham xuat hien gan nhu tuc thi va rat de bi nham la "app tu chet roi song
/// lai". Test doi qua mot chu ky day du de bat duoc ca hai kieu sinh lai.
#[test]
fn dung_theo_yeu_cau_thi_khong_bi_tu_sinh_lai() {
    let fixture = Fixture::new("stop-now");
    let beat = fixture.path("beat.txt");
    let script = fixture.heartbeat_script("stopped.cmd", &beat);

    let harness = Harness::start(vec![app(1501, "stop-now", script)]);
    let before = harness.wait_for(1501, "chay len", CYCLE, |s| s.active_procs.is_some_and(|n| n >= 1));

    harness.tx.send(Command::StopNow(1501)).unwrap();
    let stopped = harness.wait_for(1501, "dung han", CYCLE, |s| s.active_procs == Some(0));
    assert_eq!(stopped.kind, StatusKind::Stopped);

    std::thread::sleep(CYCLE);
    let after = harness.snapshot(1501).unwrap();
    assert_eq!(
        after.active_procs,
        Some(0),
        "app da bam Dung nhung supervisor lai sinh lai"
    );
    assert_eq!(after.kind, StatusKind::Stopped);
    assert_eq!(
        after.launch_count, before.launch_count,
        "so lan khoi dong tang len nghia la da co mot lan sinh lai len lut"
    );

    harness.tx.send(Command::StartNow(1501)).unwrap();
    harness.wait_for(1501, "chay lai theo yeu cau", CYCLE, |s| s.active_procs.is_some_and(|n| n >= 1));

    harness.shutdown();
}

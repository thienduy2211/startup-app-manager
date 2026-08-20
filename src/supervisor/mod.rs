//! Vong giam sat: giu cho cac app duoc khai bao luon song.
//!
//! Quyet dinh chuyen trang thai nam trong ham thuan `decide`, tach khoi phan
//! thuc thi co side effect. Nho vay logic rui ro nhat (vong sinh lai vo han)
//! kiem chung duoc bang test tat dinh thay vi phai cho doi thuc te.

pub mod backoff;
pub mod health;
pub mod job;
pub mod launch;

use std::collections::HashMap;
use std::process::Child;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{AppConfig, ManagedApp};
use crate::logging;
use job::Job;

/// Nhip cua vong lap. Ngan hon chu ky kiem tra nhieu lan de lenh tu UI duoc
/// phan hoi nhanh, trong khi viec kiem tra that su van theo chu ky rieng.
pub const TICK: Duration = Duration::from_secs(1);

/// Lenh gui tu UI sang supervisor.
#[derive(Debug)]
pub enum Command {
    /// Config vua doi; ap dung lai toan bo.
    Reload(Box<AppConfig>),
    StartNow(u64),
    StopNow(u64),
    RestartNow(u64),
    Shutdown,
}

/// Ket qua quan sat mot app tai thoi diem den han kiem tra.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    /// Con tien trinh song trong job va health check (neu co) dat.
    Alive,
    /// Con song nhung health check vua hong, chua du so lan de ket luan.
    ///
    /// Phai tach khoi `Alive`: `Alive` xoa lich su that bai, nen neu mot lan
    /// hong duoi nguong cung xoa thi bo dem khong bao gio len toi `max_retries`
    /// va app hong kinh nien se duoc sinh lai vo han.
    Degraded,
    /// Khong quan sat duoc lan nay (truy van Win32 hong). Khong phai dau hieu
    /// song, cung khong phai dau hieu chet -- khong duoc ket luan gi.
    Unknown,
    /// Job khong con tien trinh nao.
    Dead,
    /// Tien trinh con song nhung health check that bai du so lan nguong.
    Unhealthy,
}

/// Trang thai noi bo cua mot app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppState {
    /// Can khoi chay o nhip ke tiep: moi nap config, hoac vua doi tham so
    /// khoi chay nen ban dang chay khong con dung nua.
    Pending,
    /// Dung theo y user. Vong lap khong duoc tu y sinh lai; neu gop chung voi
    /// `Pending` thi nut "Stop" se bi nhip ke tiep xoa ngay.
    Stopped,
    Running { next_check_at: Instant, attempt: u32 },
    /// Tam dung theo `enabled = false`. `was_running` nho lai app truoc do co
    /// dang duoc mong doi la song hay khong, de "Resume all" go tam dung
    /// chu khong khoi dong ho nhung app dang nam yen theo y user.
    Paused { was_running: bool },
    Backoff { retry_at: Instant, attempt: u32 },
    /// Da thu du so lan cho phep ma van hong; ngung thu.
    CrashLooping { attempts: u32 },
}

/// Viec can lam sau khi `decide` chay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nothing,
    /// Giet cay tien trinh cu (neu co) roi khoi dong lai.
    Spawn,
    /// Giet cay tien trinh, khong khoi dong lai.
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub action: Action,
    pub next: AppState,
}

/// Quyet dinh buoc ke tiep cho mot app. Ham thuan: khong cham dia, khong
/// cham tien trinh, khong doc dong ho he thong.
///
/// `observe` chi duoc goi khi that su den han kiem tra, de khong ton mot request
/// HTTP moi nhip. (`publish` van hoi Job Object moi nhip de cap nhat so tien
/// trinh cho UI, nen phan Win32 khong tiet kiem duoc o day.)
pub fn decide(
    state: &AppState,
    app: &ManagedApp,
    now: Instant,
    observe: impl FnOnce() -> Observation,
) -> Transition {
    // App bi tam dung thi khong bao gio duoc sinh lai, du no da chet.
    if !app.enabled {
        return match state {
            AppState::Paused { was_running } => Transition {
                action: Action::Nothing,
                next: AppState::Paused { was_running: *was_running },
            },
            _ => Transition {
                action: Action::Stop,
                next: AppState::Paused { was_running: expected_alive(state) },
            },
        };
    }

    match state {
        // Vua duoc bat lai sau khi tam dung.
        AppState::Paused { was_running: true } => Transition {
            action: Action::Spawn,
            next: AppState::Running {
                next_check_at: now + check_interval(app),
                attempt: 0,
            },
        },

        // Truoc khi tam dung app da nam yen theo y user -- bam nut Dung, dat
        // `launch_on_start = false`, hoac da bo cuoc. "Resume all" la lenh
        // go tam dung, khong phai lenh khoi dong: sinh o day thi mot vong
        // tam dung / tiep tuc lang le bat day nhung app user co y de yen.
        AppState::Paused { was_running: false } => Transition {
            action: Action::Nothing,
            next: AppState::Stopped,
        },

        AppState::Pending => Transition {
            action: Action::Spawn,
            next: AppState::Running {
                next_check_at: now + check_interval(app),
                attempt: 0,
            },
        },

        // Da dung chu dong: chi user moi duoc danh thuc lai.
        AppState::Stopped => Transition { action: Action::Nothing, next: AppState::Stopped },

        AppState::Running { next_check_at, attempt } => {
            if now < *next_check_at {
                return Transition {
                    action: Action::Nothing,
                    next: state.clone(),
                };
            }
            match observe() {
                Observation::Alive => Transition {
                    action: Action::Nothing,
                    // Song tron mot chu ky thi xoa lich su that bai, de mot su
                    // co cu khong lam app bi bo cuoc som o lan hong sau nay.
                    next: AppState::Running {
                        next_check_at: now + check_interval(app),
                        attempt: 0,
                    },
                },
                // Chua ket luan duoc: hen lich kiem tra moi nhung giu nguyen
                // so lan da that bai.
                Observation::Degraded | Observation::Unknown => Transition {
                    action: Action::Nothing,
                    next: AppState::Running {
                        next_check_at: now + check_interval(app),
                        attempt: *attempt,
                    },
                },
                Observation::Dead | Observation::Unhealthy => Transition {
                    action: Action::Stop,
                    next: after_failure(app, now, attempt + 1),
                },
            }
        }

        AppState::Backoff { retry_at, attempt } => {
            if now < *retry_at {
                Transition { action: Action::Nothing, next: state.clone() }
            } else {
                Transition {
                    action: Action::Spawn,
                    next: AppState::Running {
                        next_check_at: now + check_interval(app),
                        attempt: *attempt,
                    },
                }
            }
        }

        // Da bo cuoc: chi khoi dong lai khi user yeu cau ro rang.
        AppState::CrashLooping { attempts } => Transition {
            action: Action::Nothing,
            next: AppState::CrashLooping { attempts: *attempts },
        },
    }
}

/// App co dang duoc mong doi la song khong, tinh theo trang thai hien tai.
///
/// `Stopped` va `CrashLooping` deu nam yen cho lenh tay, nen khong tinh.
fn expected_alive(state: &AppState) -> bool {
    matches!(
        state,
        AppState::Pending | AppState::Running { .. } | AppState::Backoff { .. }
    )
}

/// Trang thai ke tiep sau mot lan hong. Dung chung cho ca quan sat that bai lan
/// spawn that bai, de hai duong that bai cung tuan theo mot chinh sach backoff.
fn after_failure(app: &ManagedApp, now: Instant, attempt: u32) -> AppState {
    if backoff::is_crash_looping(attempt, &app.restart) {
        AppState::CrashLooping { attempts: attempt }
    } else {
        AppState::Backoff {
            retry_at: now + backoff::delay_for(attempt, &app.restart).min(MAX_INTERVAL),
            attempt,
        }
    }
}

fn check_interval(app: &ManagedApp) -> Duration {
    Duration::from_secs(app.effective_check_interval_secs())
}

/// Chan tren cho moi khoang thoi gian doc tu config.
///
/// `config.toml` duoc thiet ke de sua tay, va `Instant + Duration` **panic**
/// khi tran. Release dat `panic = "abort"`, con job co `KILL_ON_JOB_CLOSE`:
/// mot con so go nham se giet luon moi service dang duoc giam sat. Mot ngay la
/// du dai cho bat ky chu ky kiem tra hop ly nao.
const MAX_INTERVAL_SECS: u64 = crate::config::model::MAX_CHECK_INTERVAL_SECS;
const MAX_INTERVAL: Duration = Duration::from_secs(MAX_INTERVAL_SECS);

/// Trang thai rut gon cho UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Stopped,
    Running,
    Paused,
    Backoff,
    CrashLooping,
}

impl StatusKind {
    pub fn label(self) -> &'static str {
        match self {
            StatusKind::Stopped => "stopped",
            StatusKind::Running => "running",
            StatusKind::Paused => "paused",
            StatusKind::Backoff => "backoff",
            StatusKind::CrashLooping => "crash-looping",
        }
    }
}

impl From<&AppState> for StatusKind {
    fn from(s: &AppState) -> Self {
        match s {
            // `Pending` chi ton tai trong mot nhip; voi user no van la "chua chay".
            AppState::Pending | AppState::Stopped => StatusKind::Stopped,
            AppState::Running { .. } => StatusKind::Running,
            AppState::Paused { .. } => StatusKind::Paused,
            AppState::Backoff { .. } => StatusKind::Backoff,
            AppState::CrashLooping { .. } => StatusKind::CrashLooping,
        }
    }
}

/// Anh chup trang thai cho UI doc.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppStatus {
    pub id: u64,
    pub name: String,
    pub kind: StatusKind,
    /// So tien trinh song trong job. Huu ich hon PID vi target kieu `.cmd`
    /// bung tien trinh con co the da thoat trong khi app van chay.
    /// `None` la khong hoi duoc lan nay, khac han `Some(0)` la da chet han.
    /// Hien thanh `0` thi user doc mot service dang khoe la da chet va bam
    /// "Restart" -- ban do nguoi cua dung cai bug ma cach dem nay chan.
    pub active_procs: Option<u32>,
    /// So lan da khoi chay, ke ca lan dau. Doc qua `restarts()` khi can con
    /// so danh cho user.
    pub launch_count: u32,
    /// So lan da thu ma that bai, lay tu may trang thai. Khac `launch_count`:
    /// mot app co `exe` tro vao file khong ton tai hong ngay o `spawn` nen
    /// khong lan nao duoc dem la da khoi chay, va day la ca hong pho bien nhat.
    pub attempts: u32,
    pub last_error: Option<String>,
}

impl AppStatus {
    /// So lan da phai khoi dong lai -- tuc la khong tinh lan chay dau tien.
    ///
    /// Hien thang `launch_count` cho user thi mot app khoe manh chua he chet
    /// van bao "1" duoi cot Restart.
    pub fn restarts(&self) -> u32 {
        self.launch_count.saturating_sub(1)
    }
}

/// Trang thai chay cua mot app, gom ca tai nguyen he dieu hanh.
struct Runtime {
    config: ManagedApp,
    state: AppState,
    job: Option<Job>,
    child: Option<Child>,
    launch_count: u32,
    health_failures: u32,
    /// URL hong thi chi bao mot lan, thay vi moi chu ky mot dong log.
    bad_url_logged: bool,
    /// Nhu tren, cho loi truy van job.
    query_failed_logged: bool,
    last_error: Option<String>,
}

impl Runtime {
    fn new(config: ManagedApp) -> Self {
        // `launch_on_start` chi noi ve luc manager khoi dong. Sau do quyen
        // quyet dinh thuoc ve trang thai, nen `decide` khong doc lai truong nay.
        let state = match (config.enabled, config.launch_on_start) {
            (false, launch_on_start) => AppState::Paused { was_running: launch_on_start },
            (true, true) => AppState::Pending,
            (true, false) => AppState::Stopped,
        };
        Runtime {
            config,
            state,
            job: None,
            child: None,
            launch_count: 0,
            health_failures: 0,
            bad_url_logged: false,
            query_failed_logged: false,
            last_error: None,
        }
    }

    /// `None` nghia la khong hoi duoc, khac han voi `Some(0)` la da chet han.
    ///
    /// Khong ghi log o day: `status()` goi ham nay moi giay cho moi app de ve
    /// bang, nen mot loi keo dai se do day `manager.log` va cuon mat dung phan
    /// lich su can doc nhat. `observe` ghi mot lan, giong `bad_url_logged`.
    fn active_procs(&self) -> Option<u32> {
        match self.job.as_ref() {
            // Chua co job thi chac chan khong co tien trinh nao. Tra `None` o
            // day se lam moi app chua chay hien "?" va khong bao gio bi coi la
            // da chet, tuc la `Pending` khong bao gio duoc sinh len.
            None => Some(0),
            Some(job) => job.active_processes().ok(),
        }
    }

    /// Giet cay tien trinh va tra tai nguyen ve he dieu hanh.
    ///
    /// Giet that bai thi **giu lai** job. Bo handle di la mat luon so dem, va
    /// `active_procs` quay ve `Some(0)`: dung kieu noi doi "da chet han" ma
    /// quy uoc `Unknown` sinh ra de tranh. Cay tien trinh con song se thanh
    /// vo chu, con lan spawn ke tiep dung them mot ban thu hai ben canh no.
    fn stop(&mut self) {
        let cleared = match self.job.as_ref() {
            None => true,
            Some(job) => match job.terminate() {
                Ok(()) => true,
                Err(e) => {
                    // Vi du tien trinh chay o quyen cao hon manager.
                    logging::error(&format!(
                        "{}: cannot kill the process tree: {e}",
                        self.config.name
                    ));
                    false
                }
            },
        };
        // Thu hoi handle de khong ro ri doi tuong tien trinh.
        if let Some(child) = &mut self.child {
            reap(child, REAP_BUDGET);
        }
        self.child = None;
        if cleared {
            self.job = None;
        }
        self.health_failures = 0;
    }

    /// Giet ban cu roi khoi dong ban moi trong mot job moi.
    ///
    /// Luon tao job moi thay vi dung lai: job cu co the con sot tien trinh, va
    /// dung lai se lam so dem song chet lan giua hai the he.
    fn spawn(&mut self) -> Result<(), String> {
        self.stop();
        // `stop` giu job lai khi khong giet duoc. Sinh ban moi ben canh cay
        // tien trinh cu chinh la kieu nhan doi ma so dem job sinh ra de chan,
        // nen bao loi con hon. Hoi khong duoc cung coi la con song.
        if self.active_procs() != Some(0) {
            return Err("old process tree is not fully dead, not spawning a new one".to_string());
        }

        let mut cmd = launch::build_command(&self.config).map_err(|e| e.to_string())?;
        let job = Job::new().map_err(|e| format!("cannot create job: {e}"))?;
        let mut child = cmd.spawn().map_err(|e| format!("cannot spawn: {e}"))?;

        if let Err(e) = job.assign(&child) {
            // Khong gan duoc job thi khong theo doi duoc; giet ngay con hon
            // de lai mot tien trinh mo coi khong ai quan ly.
            //
            // Phai tu giet: tien trinh chua vao job nen `TerminateJobObject`
            // khong cham toi no, va `Child::drop` cua Rust khong giet gi ca.
            //
            // Thu hoi co han: tien trinh sinh ra o trang thai treo nen khong bao
            // gio tu thoat, va neu `kill` that bai thi `wait()` khong han se treo
            // vinh vien ca vong giam sat -- moi app khac mat quan sat, lenh "Exit"
            // khong duoc doc, va manager khong dong duoc nua.
            let _ = child.kill();
            reap(&mut child, REAP_BUDGET);
            return Err(format!("cannot assign to job: {e}"));
        }

        // `build_command` tao tien trinh o trang thai treo de no khong kip lam
        // gi truoc khi vao job; gio moi tha ra.
        if let Err(e) = Job::resume(&child) {
            // Da vao job roi nen `terminate` don duoc; khong don thi con lai
            // mot tien trinh treo vinh vien khong ai danh thuc.
            let _ = job.terminate();
            reap(&mut child, REAP_BUDGET);
            return Err(format!("cannot resume the process: {e}"));
        }

        self.job = Some(job);
        self.child = Some(child);
        Ok(())
    }

    /// Quan sat suc khoe: truoc het xem con tien trinh khong, sau do moi
    /// kiem tra HTTP neu app co khai bao.
    fn observe(&mut self) -> Observation {
        match self.active_procs() {
            Some(0) => {
                self.query_failed_logged = false;
                return Observation::Dead;
            }
            // Khong hoi duoc so tien trinh thi khong co bang chung nao ca. Doc
            // no thanh "da chet" nghia la mot loi Win32 thoang qua giet ca cay
            // tien trinh dang phuc vu -- dung kieu sinh lai oan ma toan bo
            // thiet ke job accounting sinh ra de chan. Cung cach xu ly nhu
            // nhanh `BadUrl` ben duoi: khong biet thi khong ket luan.
            None => {
                if !self.query_failed_logged {
                    self.query_failed_logged = true;
                    logging::warn(&format!(
                        "{}: cannot count processes in the job; skipping this tick",
                        self.config.name
                    ));
                }
                return Observation::Unknown;
            }
            Some(_) => {
                self.query_failed_logged = false;
            }
        }
        let Some(check) = &self.config.health else {
            self.health_failures = 0;
            return Observation::Alive;
        };

        match health::probe(check) {
            Ok(()) => {
                self.health_failures = 0;
                Observation::Alive
            }
            // URL sai la loi config, khong phai dau hieu service hong. Sinh lai
            // app khong sua duoc URL, chi lam mat mot service dang khoe.
            Err(health::ProbeError::BadUrl(url)) => {
                // Khong kiem tra duoc nghia la khong co bang chung nao ca; giu
                // lai bo dem cu se lam lan hong that su ke tiep bi tinh som.
                self.health_failures = 0;
                if !self.bad_url_logged {
                    self.bad_url_logged = true;
                    logging::warn(&format!(
                        "{}: health check skipped, invalid URL: {url}",
                        self.config.name
                    ));
                }
                Observation::Alive
            }
            Err(e) => {
                self.health_failures += 1;
                // Mot lan nghen tam thoi khong duoc phep giet service dang
                // phuc vu; chi ket luan hong khi that bai lien tiep.
                if self.health_failures >= check.failures_before_restart.max(1) {
                    let reason =
                        format!("health check failed {} times in a row: {e}", self.health_failures);
                    logging::warn(&format!("{}: {reason}", self.config.name));
                    // Bong thoai bao app bo cuoc doc `last_error`, ma truong do
                    // truoc gio chi duoc ghi khi `spawn` hong. Mot app chay len
                    // binh thuong nhung truot health check mai thi user chi
                    // nhan duoc "khong ro nguyen nhan" -- dung luc can biet
                    // nhat, va ly do thi da nam san o day.
                    self.last_error = Some(reason);
                    Observation::Unhealthy
                } else {
                    Observation::Degraded
                }
            }
        }
    }

    fn status(&self) -> AppStatus {
        AppStatus {
            id: self.config.id,
            name: self.config.name.clone(),
            kind: StatusKind::from(&self.state),
            active_procs: self.active_procs(),
            launch_count: self.launch_count,
            attempts: match &self.state {
                AppState::Backoff { attempt, .. } => *attempt,
                AppState::CrashLooping { attempts } => *attempts,
                _ => 0,
            },
            last_error: self.last_error.clone(),
        }
    }
}

/// Anh chup trang thai dung chung giua supervisor va UI.
pub type SharedStatus = Arc<Mutex<Vec<AppStatus>>>;

pub struct Supervisor {
    runtimes: Vec<Runtime>,
    status: SharedStatus,
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl Supervisor {
    pub fn new(config: AppConfig, status: SharedStatus, notify: Arc<dyn Fn() + Send + Sync>) -> Self {
        Supervisor {
            runtimes: config.apps.into_iter().map(Runtime::new).collect(),
            status,
            notify,
        }
    }

    /// Vong lap chinh. Ket thuc khi nhan `Shutdown` hoac kenh lenh dong.
    pub fn run(mut self, rx: Receiver<Command>) {
        logging::info(&format!("supervisor started with {} apps", self.runtimes.len()));
        loop {
            match self.drain_commands(&rx) {
                ControlFlow::Stop => break,
                ControlFlow::Continue => {}
            }
            // `tick` co the ton nhieu giay khi nhieu app cung den han va
            // probe HTTP bi treo; no tu doi lenh giua cac app de bam "Exit"
            // khong phai cho het luot.
            if let ControlFlow::Stop = self.tick(&rx) {
                break;
            }
            self.publish();
            std::thread::sleep(TICK);
        }
        for rt in &mut self.runtimes {
            rt.stop();
        }
        logging::info("supervisor stopped, child processes cleaned up");
    }

    fn drain_commands(&mut self, rx: &Receiver<Command>) -> ControlFlow {
        let mut pending = Vec::new();
        let flow = collect_commands(rx, &mut pending);
        // Dang thoat thi khoi sinh ra thu se bi giet ngay o dong ke tiep.
        if let ControlFlow::Continue = flow {
            for cmd in pending {
                self.apply(cmd);
            }
        }
        flow
    }

    fn apply(&mut self, cmd: Command) {
        match cmd {
            Command::Shutdown => {}
            Command::Reload(cfg) => self.reload(*cfg),
            Command::StartNow(id) => self.start(id, false),
            Command::StopNow(id) => self.stop(id),
            Command::RestartNow(id) => self.start(id, true),
        }
    }

    /// Ap dung config moi, giu nguyen tien trinh dang chay cua app khong doi.
    fn reload(&mut self, config: AppConfig) {
        let mut old: HashMap<u64, Runtime> =
            self.runtimes.drain(..).map(|rt| (rt.config.id, rt)).collect();

        for app in config.apps {
            match old.remove(&app.id) {
                Some(mut rt) => {
                    // Doi tham so khoi chay thi phai sinh lai moi co hieu luc;
                    // vong lap se lo o nhip sau qua trang thai Pending.
                    //
                    // Ap dung cho ca app dang cho thu lai va app da bo cuoc:
                    // sua config sai chinh la cach user go mot app crash-loop.
                    // App user tu dung hoac dang tam dung thi khong dung toi.
                    let needs_restart = restart_relevant(&rt.config) != restart_relevant(&app);
                    let should_be_alive = matches!(
                        rt.state,
                        AppState::Running { .. }
                            | AppState::Backoff { .. }
                            | AppState::CrashLooping { .. }
                    );
                    // Health vua doi thi lich su that bai cua ban cu khong con
                    // y nghia: giu lai se lam lan hong dau tien cua ban moi bi
                    // tinh cong don va restart som hon nguong user dat.
                    if rt.config.health != app.health {
                        rt.bad_url_logged = false;
                        rt.health_failures = 0;
                    }
                    // App da bo cuoc chi song lai khi co gi do thay doi, ma
                    // `restart_relevant` lai khong ke health va chinh sach thu
                    // lai. Thieu nhanh nay thi viec user sua dung URL hong --
                    // chinh la cach go mot app crash-loop -- khong go duoc gi.
                    let gave_up_and_changed = matches!(rt.state, AppState::CrashLooping { .. })
                        && (rt.config.health != app.health || rt.config.restart != app.restart);
                    rt.config = app;
                    if needs_restart && should_be_alive {
                        rt.stop();
                        rt.state = AppState::Pending;
                    } else if gave_up_and_changed {
                        rt.state = AppState::Pending;
                    } else if let AppState::Running { next_check_at, attempt } = &rt.state {
                        // Rut ngan chu ky ma khong keo lich hen lai thi han cu
                        // van con hieu luc: user chon 30 giay nhung mot su co co
                        // the khong bi phat hien trong ca tieng dong ho.
                        let capped = Instant::now() + check_interval(&rt.config);
                        if *next_check_at > capped {
                            rt.state = AppState::Running {
                                next_check_at: capped,
                                attempt: *attempt,
                            };
                        }
                    }
                    self.runtimes.push(rt);
                }
                None => self.runtimes.push(Runtime::new(app)),
            }
        }

        // App bi xoa khoi config: dung han tien trinh cua no.
        for (_, mut rt) in old {
            rt.stop();
        }
    }

    /// Khoi dong theo yeu cau tu UI.
    ///
    /// `force` phan biet hai nut khac nhau: "Start" khong duoc dong den app
    /// dang chay (giet mot service dang phuc vu la mat du lieu dang xu ly), con
    /// "Restart" thi luon sinh ban moi.
    fn start(&mut self, id: u64, force: bool) {
        let Some(rt) = self.runtimes.iter_mut().find(|r| r.config.id == id) else {
            return;
        };
        // App tam dung khong duoc chay du chi mot giay: no van kip lam moi thu
        // co tac dung phu -- migrate DB, goi ra ngoai, chiem cong -- truoc khi
        // nhip ke tiep giet no.
        if !rt.config.enabled {
            logging::warn(&format!(
                "{}: paused, ignoring the start command",
                rt.config.name
            ));
            return;
        }
        // Chi khoi chay khi biet chac khong con tien trinh nao. Hoi khong duoc
        // thi coi nhu con song: sinh them mot ban nua te hon la khong sinh.
        if !force && matches!(rt.state, AppState::Running { .. }) && rt.active_procs() != Some(0) {
            return;
        }

        match rt.spawn() {
            Ok(()) => {
                rt.launch_count += 1;
                rt.last_error = None;
                rt.state = AppState::Running {
                    next_check_at: Instant::now() + check_interval(&rt.config),
                    attempt: 0,
                };
            }
            Err(e) => {
                logging::error(&format!("{}: {e}", rt.config.name));
                rt.last_error = Some(e);
                // Cung duong voi that bai tu dong: `Stopped` thi khong bao gio
                // thu lai nua, ma `spawn` da `stop()` truoc do roi. Mot loi
                // thoang qua (vi du `Job::resume` het luot thu) se bien mot cu
                // bam "Restart" tren service dang khoe thanh chet han,
                // im lang -- UI chi hien "stopped".
                rt.state = after_failure(&rt.config, Instant::now(), 1);
            }
        }
    }

    fn stop(&mut self, id: u64) {
        if let Some(rt) = self.runtimes.iter_mut().find(|r| r.config.id == id) {
            rt.stop();
            rt.state = AppState::Stopped;
        }
    }

    fn tick(&mut self, rx: &Receiver<Command>) -> ControlFlow {
        // Lenh den giua chung phai gom lai chu khong ap dung ngay: `reload` co
        // the them bot runtime va lam hong vi tri dang duyet.
        let mut deferred = Vec::new();
        let mut i = 0;
        while i < self.runtimes.len() {
            let rt = &mut self.runtimes[i];
            i += 1;
            // Doc dong ho cho tung app chu khong dung chung moc dau nhip: mot
            // nhip cham (nhieu probe HTTP treo) lam `next_check_at` cua app
            // xet sau da tieu mat phan lon chu ky ngay luc dat, keo chu ky
            // thuc te ve sat nhip 1 giay. `decide` van thuan: `now` la tham so.
            let now = Instant::now();
            let Transition { action, mut next } = {
                let config = rt.config.clone();
                let state = rt.state.clone();
                // `observe` chi chay khi den han, nho closure lazy o `decide`.
                let mut observed = None;
                let t = decide(&state, &config, now, || {
                    let o = rt.observe();
                    observed = Some(o);
                    o
                });
                if let Some(Observation::Dead) = observed {
                    logging::warn(&format!("{}: no processes left", config.name));
                }
                t
            };

            match action {
                Action::Nothing => {}
                Action::Stop => rt.stop(),
                Action::Spawn => match rt.spawn() {
                    Ok(()) => {
                        rt.launch_count += 1;
                        rt.last_error = None;
                        logging::info(&format!(
                            "{}: started (launch #{})",
                            rt.config.name, rt.launch_count
                        ));
                    }
                    Err(e) => {
                        logging::error(&format!("{}: {e}", rt.config.name));
                        rt.last_error = Some(e);
                        // Spawn hong ma van nhan `Running` thi UI bao "running"
                        // voi 0 tien trinh, va lan kiem tra sau moi phat hien
                        // ra. Tinh ngay la mot lan that bai de doi backoff.
                        let attempt = match &next {
                            AppState::Running { attempt, .. } => attempt + 1,
                            _ => 1,
                        };
                        next = after_failure(&rt.config, now, attempt);
                    }
                },
            }
            rt.state = next;

            if let ControlFlow::Stop = collect_commands(rx, &mut deferred) {
                return ControlFlow::Stop;
            }
        }

        for cmd in deferred {
            self.apply(cmd);
        }
        ControlFlow::Continue
    }

    fn publish(&self) {
        let snapshot: Vec<AppStatus> = self.runtimes.iter().map(Runtime::status).collect();
        let changed = match self.status.lock() {
            Ok(mut guard) => {
                let changed = *guard != snapshot;
                if changed {
                    *guard = snapshot;
                }
                changed
            }
            Err(_) => false,
        };
        // Chi danh thuc UI khi co thay doi that, tranh ve lai lien tuc.
        if changed {
            (self.notify)();
        }
    }
}

/// Han cho mot tien trinh chet han sau khi da ra lenh giet.
const REAP_BUDGET: Duration = Duration::from_secs(2);

/// Thu hoi tien trinh con, nhung khong bao gio cho vo han.
///
/// `Child::wait()` khong co han: neu `TerminateJobObject` that bai thi tien
/// trinh khong bao gio chet va vong giam sat mot thread nay treo vinh vien --
/// ke ca tren duong thoat, khien ca manager khong dong duoc.
fn reap(child: &mut Child, budget: Duration) {
    let deadline = Instant::now() + budget;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) => {}
        }
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Cac truong ma doi chung thi phai sinh lai tien trinh moi co hieu luc.
fn restart_relevant(app: &ManagedApp) -> impl PartialEq + '_ {
    (
        &app.exe,
        &app.args,
        &app.working_dir,
        &app.env,
        &app.env_file,
        &app.env_from_files,
    )
}

/// Lay het lenh dang cho ma khong chan. Tra `Stop` ngay khi gap `Shutdown`
/// hoac khi UI da dong kenh.
fn collect_commands(rx: &Receiver<Command>, out: &mut Vec<Command>) -> ControlFlow {
    loop {
        match rx.try_recv() {
            Ok(Command::Shutdown) => return ControlFlow::Stop,
            Ok(cmd) => out.push(cmd),
            Err(TryRecvError::Empty) => return ControlFlow::Continue,
            Err(TryRecvError::Disconnected) => return ControlFlow::Stop,
        }
    }
}

enum ControlFlow {
    Continue,
    Stop,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{HealthCheck, RestartPolicy};

    fn app() -> ManagedApp {
        ManagedApp {
            id: 1,
            name: "test".into(),
            exe: "x.exe".into(),
            check_interval_secs: 60,
            restart: RestartPolicy {
                max_retries: 3,
                backoff_base_secs: 5,
                backoff_max_secs: 100,
            },
            ..Default::default()
        }
    }

    fn config_of(apps: Vec<ManagedApp>) -> AppConfig {
        let mut cfg = AppConfig::default();
        cfg.apps = apps;
        cfg
    }

    fn supervisor_with(app: ManagedApp) -> Supervisor {
        Supervisor::new(
            config_of(vec![app]),
            Arc::new(Mutex::new(Vec::new())),
            Arc::new(|| {}),
        )
    }

    #[test]
    fn khoi_dong_tay_that_bai_van_duoc_thu_lai() {
        // `spawn` da `stop()` truoc do, nen neu that bai ma nhan `Stopped` thi
        // mot loi thoang qua bien cu bam "Restart" tren service dang khoe
        // thanh chet han, khong bao gio tu song lai.
        let mut sup = supervisor_with(ManagedApp { exe: "".into(), ..app() });
        sup.runtimes[0].state = AppState::Stopped;

        sup.start(1, false);

        assert!(
            matches!(sup.runtimes[0].state, AppState::Backoff { attempt: 1, .. }),
            "{:?}",
            sup.runtimes[0].state
        );
        assert!(sup.runtimes[0].last_error.is_some());
    }

    #[test]
    fn sua_health_go_duoc_app_da_bo_cuoc() {
        // URL health tro nham cong -> app bi restart mai roi bo cuoc. Sua lai
        // URL chinh la cach user go, nhung `restart_relevant` khong ke health
        // nen truoc day app nam CrashLooping vinh vien.
        let broken = ManagedApp {
            health: Some(HealthCheck {
                url: "http://127.0.0.1:9/".into(),
                ..Default::default()
            }),
            ..app()
        };
        let mut sup = supervisor_with(broken.clone());
        sup.runtimes[0].state = AppState::CrashLooping { attempts: 4 };

        // Doi mot truong khong lien quan gi den kha nang chay: van bo cuoc.
        sup.reload(config_of(vec![ManagedApp { name: "ten moi".into(), ..broken.clone() }]));
        assert_eq!(sup.runtimes[0].state, AppState::CrashLooping { attempts: 4 });

        // Sua URL: phai duoc chay lai o nhip ke tiep.
        let fixed = ManagedApp {
            health: Some(HealthCheck {
                url: "http://127.0.0.1:8080/".into(),
                ..Default::default()
            }),
            ..broken
        };
        sup.reload(config_of(vec![fixed]));
        assert_eq!(sup.runtimes[0].state, AppState::Pending);
    }

    #[test]
    fn noi_long_so_lan_thu_cung_go_duoc_app_da_bo_cuoc() {
        let mut sup = supervisor_with(app());
        sup.runtimes[0].state = AppState::CrashLooping { attempts: 4 };

        let relaxed = ManagedApp {
            restart: RestartPolicy { max_retries: 10, ..app().restart },
            ..app()
        };
        sup.reload(config_of(vec![relaxed]));
        assert_eq!(sup.runtimes[0].state, AppState::Pending);
    }

    /// `observe` khong duoc goi trong tinh huong nay.
    fn never() -> Observation {
        panic!("khong duoc quan sat khi chua den han");
    }

    #[test]
    fn app_tam_dung_khong_bao_gio_duoc_sinh_lai() {
        // A3: day la bao dam quan trong nhat cua tinh nang tam dung.
        let now = Instant::now();
        let paused = ManagedApp { enabled: false, ..app() };

        // Dang chay ma bi tam dung -> phai dung han.
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 0 },
            &paused,
            now,
            never,
        );
        assert_eq!(
            t,
            Transition {
                action: Action::Stop,
                next: AppState::Paused { was_running: true }
            }
        );

        // Da tam dung roi thi khong lam gi ca, ke ca khi den han kiem tra.
        let t = decide(
            &AppState::Paused { was_running: true },
            &paused,
            now + Duration::from_secs(3600),
            never,
        );
        assert_eq!(t.action, Action::Nothing);
        assert_eq!(t.next, AppState::Paused { was_running: true });

        // Ngay ca khi dang trong backoff cung khong duoc sinh lai.
        let t = decide(
            &AppState::Backoff { retry_at: now, attempt: 1 },
            &paused,
            now + Duration::from_secs(60),
            never,
        );
        assert_eq!(t.action, Action::Stop);
        assert_eq!(t.next, AppState::Paused { was_running: true });
    }

    #[test]
    fn khong_quan_sat_duoc_thi_khong_giet_app() {
        // Mot loi Win32 thoang qua khong duoc doc thanh "app da chet": ket luan
        // nham o day giet ca cay tien trinh dang phuc vu, dung kieu sinh lai
        // oan ma job accounting sinh ra de chan.
        let now = Instant::now();
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 2 },
            &app(),
            now,
            || Observation::Unknown,
        );
        assert_eq!(t.action, Action::Nothing);
        assert!(
            matches!(t.next, AppState::Running { attempt: 2, .. }),
            "khong ket luan duoc thi cung khong duoc xoa lich su that bai"
        );
    }

    #[test]
    fn bat_lai_sau_khi_tam_dung_thi_sinh_lai() {
        let now = Instant::now();
        let t = decide(&AppState::Paused { was_running: true }, &app(), now, never);
        assert_eq!(t.action, Action::Spawn);
        assert!(matches!(t.next, AppState::Running { attempt: 0, .. }));
    }

    #[test]
    fn tiep_tuc_tat_ca_khong_danh_thuc_app_dang_nam_yen() {
        // User bam "Stop" cho app A, roi "Pause all" va "Resume all".
        // A phai van dung -- lenh do go tam dung, khong phai lenh khoi dong.
        let now = Instant::now();
        let stopped = ManagedApp { enabled: false, ..app() };
        let t = decide(&AppState::Stopped, &stopped, now, never);
        assert_eq!(t.next, AppState::Paused { was_running: false });

        let t = decide(&AppState::Paused { was_running: false }, &app(), now, never);
        assert_eq!(t.action, Action::Nothing);
        assert_eq!(t.next, AppState::Stopped);
    }

    #[test]
    fn app_da_bo_cuoc_khong_tu_chay_lai_sau_mot_vong_tam_dung() {
        let now = Instant::now();
        let paused = ManagedApp { enabled: false, ..app() };
        let t = decide(&AppState::CrashLooping { attempts: 9 }, &paused, now, never);
        assert_eq!(t.next, AppState::Paused { was_running: false });
    }

    #[test]
    fn chua_den_han_thi_khong_quan_sat() {
        // Quan trong ve hieu nang: moi nhip deu quan sat se ton mot loi goi
        // Win32 va mot request HTTP moi giay cho moi app.
        let now = Instant::now();
        let t = decide(
            &AppState::Running { next_check_at: now + Duration::from_secs(30), attempt: 0 },
            &app(),
            now,
            never,
        );
        assert_eq!(t.action, Action::Nothing);
    }

    #[test]
    fn con_song_thi_hen_lich_moi_va_xoa_lich_su_that_bai() {
        let now = Instant::now();
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 2 },
            &app(),
            now,
            || Observation::Alive,
        );
        assert_eq!(t.action, Action::Nothing);
        match t.next {
            AppState::Running { next_check_at, attempt } => {
                assert_eq!(attempt, 0, "song tron chu ky phai xoa lich su that bai");
                assert_eq!(next_check_at, now + Duration::from_secs(60));
            }
            other => panic!("mong doi Running, nhan {other:?}"),
        }
    }

    #[test]
    fn chet_thi_vao_backoff_voi_gian_cach_tang_dan() {
        // A5 + A6: backoff la thu chan vong sinh lai vo han.
        let now = Instant::now();
        let a = app();

        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 0 },
            &a,
            now,
            || Observation::Dead,
        );
        assert_eq!(t.action, Action::Stop);
        assert_eq!(
            t.next,
            AppState::Backoff { retry_at: now + Duration::from_secs(5), attempt: 1 }
        );

        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 1 },
            &a,
            now,
            || Observation::Dead,
        );
        assert_eq!(
            t.next,
            AppState::Backoff { retry_at: now + Duration::from_secs(10), attempt: 2 }
        );
    }

    #[test]
    fn het_backoff_thi_sinh_lai_va_giu_so_lan_da_thu() {
        let now = Instant::now();
        let t = decide(
            &AppState::Backoff { retry_at: now, attempt: 2 },
            &app(),
            now,
            never,
        );
        assert_eq!(t.action, Action::Spawn);
        assert!(matches!(t.next, AppState::Running { attempt: 2, .. }));
    }

    #[test]
    fn chua_het_backoff_thi_cho() {
        let now = Instant::now();
        let retry_at = now + Duration::from_secs(5);
        let t = decide(
            &AppState::Backoff { retry_at, attempt: 1 },
            &app(),
            now,
            never,
        );
        assert_eq!(t.action, Action::Nothing);
        assert_eq!(t.next, AppState::Backoff { retry_at, attempt: 1 });
    }

    #[test]
    fn vuot_nguong_thi_bo_cuoc_va_khong_sinh_lai_nua() {
        // A6: app hong vinh vien phai ngung lam ton CPU.
        let now = Instant::now();
        let a = app(); // max_retries = 3

        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 3 },
            &a,
            now,
            || Observation::Dead,
        );
        assert_eq!(t.action, Action::Stop);
        assert_eq!(t.next, AppState::CrashLooping { attempts: 4 });

        // Da bo cuoc thi du bao lau cung khong tu sinh lai.
        let t = decide(
            &AppState::CrashLooping { attempts: 4 },
            &a,
            now + Duration::from_secs(86_400),
            never,
        );
        assert_eq!(t.action, Action::Nothing);
        assert_eq!(t.next, AppState::CrashLooping { attempts: 4 });
    }

    #[test]
    fn max_retries_bang_khong_thi_khong_bao_gio_bo_cuoc() {
        let now = Instant::now();
        let a = ManagedApp {
            restart: RestartPolicy { max_retries: 0, backoff_base_secs: 5, backoff_max_secs: 100 },
            ..app()
        };
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 9_999 },
            &a,
            now,
            || Observation::Dead,
        );
        assert!(matches!(t.next, AppState::Backoff { .. }));
    }

    #[test]
    fn unhealthy_duoc_xu_ly_nhu_da_chet() {
        // A12: tien trinh con song nhung service treo van phai duoc sinh lai.
        let now = Instant::now();
        let a = ManagedApp {
            health: Some(HealthCheck {
                url: "http://127.0.0.1:8787/health".into(),
                ..Default::default()
            }),
            ..app()
        };
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 0 },
            &a,
            now,
            || Observation::Unhealthy,
        );
        assert_eq!(t.action, Action::Stop);
        assert!(matches!(t.next, AppState::Backoff { attempt: 1, .. }));
    }

    #[test]
    fn khong_tu_chay_thi_khoi_dau_o_trang_thai_dung() {
        let a = ManagedApp { launch_on_start: false, ..app() };
        assert_eq!(Runtime::new(a).state, AppState::Stopped);
        assert_eq!(Runtime::new(app()).state, AppState::Pending);
    }

    #[test]
    fn da_dung_thi_khong_bao_gio_tu_sinh_lai() {
        // App dung theo y user phai nam yen ke ca khi no bat tu chay cung
        // Windows, neu khong nut "Stop" bi nhip ke tiep xoa ngay.
        let a = ManagedApp { launch_on_start: true, ..app() };
        let t = decide(&AppState::Stopped, &a, Instant::now(), never);
        assert_eq!(t.action, Action::Nothing);
        assert_eq!(t.next, AppState::Stopped);
    }

    #[test]
    fn hong_duoi_nguong_khong_xoa_lich_su_that_bai() {
        // Neu mot lan hong duoi nguong duoc coi la Alive, bo dem `attempt` bi
        // reset moi chu ky va app hong kinh nien duoc sinh lai vo han --
        // `max_retries` khong bao gio co hieu luc tren duong health check.
        let a = app();
        let now = Instant::now();
        let t = decide(
            &AppState::Running { next_check_at: now, attempt: 2 },
            &a,
            now,
            || Observation::Degraded,
        );
        assert_eq!(t.action, Action::Nothing);
        assert!(matches!(t.next, AppState::Running { attempt: 2, .. }));
    }

    #[test]
    fn cho_khoi_chay_thi_sinh_ngay() {
        let t = decide(&AppState::Pending, &app(), Instant::now(), never);
        assert_eq!(t.action, Action::Spawn);
        assert!(matches!(t.next, AppState::Running { attempt: 0, .. }));
    }

    #[test]
    fn chu_ky_qua_ngan_bi_nang_len_muc_toi_thieu() {
        // Chu ky 1 giay chi ton CPU chu khong phat hien duoc gi som hon.
        let a = ManagedApp { check_interval_secs: 1, ..app() };
        assert_eq!(
            check_interval(&a),
            Duration::from_secs(crate::config::model::MIN_CHECK_INTERVAL_SECS)
        );
    }
}

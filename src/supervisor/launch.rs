//! Dung `Command` de spawn mot app duoc quan ly.
//!
//! Chien luoc chay suy ra tu duoi file. Target khong nhat thiet la `.exe`:
//! co the la `.cmd`, hoac mot interpreter (node/bun/python) voi script nam
//! trong `args`.

use std::ffi::OsString;
use std::fmt;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{env as cfg_env, ManagedApp};
use crate::paths;

/// Chay khong tao cua so console. Thay the hoan toan lop `.vbs` bao ngoai ma
/// cac he keepalive cu phai dung chi de giau cua so.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Tien trinh sinh ra o trang thai treo, chi chay sau khi da duoc gan vao job.
/// Nguoi goi **phai** goi `Job::resume`, neu khong app se dung im mai mai.
const CREATE_SUSPENDED: u32 = 0x0000_0004;

#[derive(Debug)]
pub enum LaunchError {
    NoExecutable,
    Env(cfg_env::EnvError),
    Io(std::io::Error),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::NoExecutable => write!(f, "no executable path configured"),
            LaunchError::Env(e) => write!(f, "{e}"),
            LaunchError::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Chuong trinh se chay va tham so dat truoc tham so cua user.
///
/// Script `.js`, `.ps1` va `.vbs` khong tu chay duoc nen phai co interpreter
/// dung truoc: `CreateProcessW` tu choi chung bang `ERROR_BAD_EXE_FORMAT`, va
/// form van cho user chon nhung file do. Cac duoi con lai giao cho Windows tu xu ly:
/// `Command` biet dinh tuyen `.cmd` qua `cmd.exe`, va ten khong duoi se duoc
/// tim theo `PATH`.
fn program_for(exe: &Path) -> (OsString, Vec<OsString>) {
    let ext = exe
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());

    match ext.as_deref() {
        Some("js") | Some("mjs") | Some("cjs") => {
            ("node".into(), vec![exe.as_os_str().to_os_string()])
        }
        // `.vbs` cung bi `CreateProcessW` tu choi. Dang chu y trong du an nay
        // vi he cu duoc dung bang chinh cac launcher `.vbs`, nen user rat de
        // tro thang vao mot file nhu vay theo thoi quen.
        Some("vbs") | Some("vbe") | Some("wsf") => (
            "wscript".into(),
            vec![
                "//B".into(),
                "//Nologo".into(),
                exe.as_os_str().to_os_string(),
            ],
        ),
        Some("ps1") => (
            "powershell".into(),
            vec![
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                exe.as_os_str().to_os_string(),
            ],
        ),
        _ => (exe.as_os_str().to_os_string(), Vec::new()),
    }
}

/// Dung `Command` da cau hinh day du, san sang `spawn()`.
pub fn build_command(app: &ManagedApp) -> Result<Command, LaunchError> {
    if app.exe.as_os_str().is_empty() {
        return Err(LaunchError::NoExecutable);
    }

    let (program, mut args) = program_for(&app.exe);
    args.extend(split_args(&app.args).into_iter().map(OsString::from));

    let mut cmd = Command::new(program);
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW | CREATE_SUSPENDED);

    if let Some(dir) = &app.working_dir {
        cmd.current_dir(dir);
    }

    // Env cua manager duoc giu lam nen; khai bao cua app ghi de len tren.
    let env = cfg_env::resolve(app).map_err(LaunchError::Env)?;
    cmd.envs(env);

    // Gom stdout/stderr vao log rieng cua app. Khong co no thi mot app Node
    // chet lien tuc se khong de lai dau vet nao de chan doan.
    //
    // Mo khong duoc thi bo qua chu khong hong ca lenh khoi chay: dia day hay
    // mot editor dang giu file log deu khong phai ly do de ngung giam sat
    // service -- day cung la nguyen tac `logging` tu dat cho chinh no.
    let (out, err) = match open_app_log(app.id).and_then(|f| Ok((f.try_clone()?, f))) {
        Ok((a, b)) => (Stdio::from(a), Stdio::from(b)),
        Err(e) => {
            crate::logging::warn(&format!(
                "app {}: cannot open its own log ({e}); dropping stdout/stderr",
                app.id
            ));
            (Stdio::null(), Stdio::null())
        }
    };
    cmd.stdout(out);
    cmd.stderr(err);
    cmd.stdin(Stdio::null());

    Ok(cmd)
}

/// Log cua app noi hon log cua manager nhieu vi no chua ca stdout lan stderr
/// cua tien trinh con, nen nguong cat cao hon.
const MAX_APP_LOG_BYTES: u64 = 4 * 1024 * 1024;

fn open_app_log(app_id: u64) -> std::io::Result<std::fs::File> {
    paths::ensure_dirs()?;
    let path = paths::app_log_file(app_id);
    // Chi xoay duoc o day. Tien trinh con giu handle cua chinh file nay suot
    // vong doi cua no: doi ten giua chung thi no ghi tiep vao ban `.log.1` va
    // file that dung im, tuc la khong chan duoc gi ma con lam mat dau vet.
    // He qua phai chap nhan: mot app noi nhieu ma khong bao gio restart van co
    // the phinh log den luc duoc khoi dong lai.
    crate::logging::rotate_if_needed(&path, MAX_APP_LOG_BYTES);
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// Tach chuoi tham so theo quy tac dong lenh Windows, o muc du dung:
/// khoang trang ngan cach, nhay kep gom nhom, `\"` la nhay kep nguyen van.
///
/// Can tu tach vi config luu `args` dang chuoi tho cho de sua tay, trong khi
/// `Command` nhan tung tham so roi.
fn split_args(raw: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut has_content = false;
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'"') => {
                chars.next();
                current.push('"');
                has_content = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                // Nhay kep rong van tao ra mot tham so rong hop le.
                has_content = true;
            }
            c if c.is_whitespace() && !in_quotes => {
                if has_content {
                    out.push(std::mem::take(&mut current));
                    has_content = false;
                }
            }
            c => {
                current.push(c);
                has_content = true;
            }
        }
    }
    if has_content {
        out.push(current);
    }
    out
}

use std::os::windows::process::CommandExt;

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn exe_chay_truc_tiep() {
        let (prog, pre) = program_for(Path::new(r"C:\app\x.exe"));
        assert_eq!(prog, OsString::from(r"C:\app\x.exe"));
        assert!(pre.is_empty());
    }

    #[test]
    fn cmd_va_bat_giao_cho_windows_dinh_tuyen() {
        // `Command` tu chay `.cmd` qua cmd.exe, khong can tu boc.
        for p in [r"C:\a\run.cmd", r"C:\a\run.bat", r"C:\a\RUN.CMD"] {
            let (prog, pre) = program_for(Path::new(p));
            assert_eq!(prog, OsString::from(p));
            assert!(pre.is_empty(), "{p}");
        }
    }

    #[test]
    fn script_js_duoc_dat_sau_node() {
        for p in [r"C:\a\cli.js", r"C:\a\cli.mjs", r"C:\a\cli.cjs"] {
            let (prog, pre) = program_for(Path::new(p));
            assert_eq!(prog, OsString::from("node"), "{p}");
            assert_eq!(pre, vec![OsString::from(p)], "{p}");
        }
    }

    #[test]
    fn ten_khong_duoi_de_path_tu_tim() {
        let (prog, pre) = program_for(Path::new("node"));
        assert_eq!(prog, OsString::from("node"));
        assert!(pre.is_empty());
    }

    #[test]
    fn tach_args_don_gian() {
        assert_eq!(split_args("-n -t --skip-update"), vec!["-n", "-t", "--skip-update"]);
        assert_eq!(split_args(""), Vec::<String>::new());
        assert_eq!(split_args("   "), Vec::<String>::new());
    }

    #[test]
    fn nhay_kep_giu_duong_dan_co_dau_cach() {
        // Truong hop that: script 9Router nam trong duong dan co dau cach.
        let args = split_args(r#""C:\Program Files\a b\cli.js" -n -t"#);
        assert_eq!(args, vec![r"C:\Program Files\a b\cli.js", "-n", "-t"]);
    }

    #[test]
    fn nhay_kep_thoat_va_tham_so_rong() {
        assert_eq!(split_args(r#"--msg "xin \"chao\"""#), vec!["--msg", r#"xin "chao""#]);
        assert_eq!(split_args(r#"a "" b"#), vec!["a", "", "b"]);
    }

    #[test]
    fn thieu_exe_bao_loi_thay_vi_spawn_bua() {
        let err = build_command(&ManagedApp::default()).unwrap_err();
        assert!(matches!(err, LaunchError::NoExecutable));
    }

    #[test]
    fn build_command_dung_chuong_trinh_va_tham_so() {
        let app = ManagedApp {
            id: 9_001,
            exe: PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
            args: r#""C:\a\cli.js" -n -t --skip-update"#.into(),
            ..Default::default()
        };
        let cmd = build_command(&app).unwrap();
        assert_eq!(cmd.get_program(), OsString::from(r"C:\Program Files\nodejs\node.exe"));

        let args: Vec<_> = cmd.get_args().map(|a| a.to_owned()).collect();
        assert_eq!(
            args,
            vec![
                OsString::from(r"C:\a\cli.js"),
                OsString::from("-n"),
                OsString::from("-t"),
                OsString::from("--skip-update"),
            ]
        );
        std::fs::remove_file(paths::app_log_file(9_001)).ok();
    }

    #[test]
    fn build_command_gan_env_da_gom() {
        let app = ManagedApp {
            id: 9_002,
            exe: PathBuf::from("cmd.exe"),
            env: [("MY_VAR".to_string(), "gia-tri".to_string())].into(),
            ..Default::default()
        };
        let cmd = build_command(&app).unwrap();
        let found = cmd
            .get_envs()
            .any(|(k, v)| k == "MY_VAR" && v == Some(std::ffi::OsStr::new("gia-tri")));
        assert!(found, "env cua app phai duoc gan vao Command");
        std::fs::remove_file(paths::app_log_file(9_002)).ok();
    }

    #[test]
    fn env_file_thieu_lam_build_that_bai_ro_rang() {
        let app = ManagedApp {
            id: 9_003,
            exe: PathBuf::from("cmd.exe"),
            env_from_files: [("T".to_string(), PathBuf::from("Z:/khong/ton/tai"))].into(),
            ..Default::default()
        };
        assert!(matches!(build_command(&app), Err(LaunchError::Env(_))));
    }
}

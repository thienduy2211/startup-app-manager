//! Form them/sua mot app.
//!
//! Khong dung vong lap su kien long nhau de lam modal: nwg chi co mot vong
//! dispatch cho ca thread, `stop_thread_dispatch` se giet luon vong ngoai.
//! Thay vao do cua so cha bi vo hieu hoa trong luc form mo (xem `ui::mod`),
//! cho hieu ung tuong duong ma khong co rui ro treo.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;

use native_windows_gui as nwg;

use crate::config::{env as cfg_env, HealthCheck, ManagedApp, RestartPolicy};
use crate::supervisor::health;

use super::format;

const LABEL_W: i32 = 120;
const FIELD_X: i32 = 136;
const FIELD_W: i32 = 430;
const ROW_H: i32 = 30;

#[derive(Default)]
pub struct Editor {
    pub window: nwg::Window,

    in_name: nwg::TextInput,
    in_exe: nwg::TextInput,
    btn_browse_exe: nwg::Button,
    in_args: nwg::TextInput,
    in_workdir: nwg::TextInput,
    btn_browse_dir: nwg::Button,
    cb_interval: nwg::ComboBox<String>,
    chk_launch: nwg::CheckBox,

    lbl_warning: nwg::Label,

    in_env: nwg::TextBox,
    in_env_file: nwg::TextInput,
    in_env_from_files: nwg::TextBox,
    in_max_retries: nwg::TextInput,

    chk_health: nwg::CheckBox,
    in_health_url: nwg::TextInput,
    in_health_fails: nwg::TextInput,

    pub btn_ok: nwg::Button,
    pub btn_cancel: nwg::Button,

    dlg_exe: nwg::FileDialog,
    dlg_dir: nwg::FileDialog,

    /// Nhan tinh; giu lai chi de chung khong bi huy khi build ket thuc.
    labels: Vec<nwg::Label>,

    /// `None` nghia la dang them moi.
    editing_id: Cell<Option<u64>>,
    /// Cac truong khong hien tren form (id, enabled, backoff...) duoc giu lai
    /// de luu khong lam mat gia tri user da dat trong file config.
    carried: RefCell<ManagedApp>,
}

impl Editor {
    pub fn build(&mut self) -> Result<(), nwg::NwgError> {
        nwg::Window::builder()
            .size((600, 640))
            .title("App")
            .flags(nwg::WindowFlags::WINDOW)
            .build(&mut self.window)?;

        let mut y = 14;

        self.labels.push(label(&self.window, "Name", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W, 24))
            .build(&mut self.in_name)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "Executable", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W - 92, 24))
            .build(&mut self.in_exe)?;
        nwg::Button::builder()
            .parent(&self.window)
            .text("Browse...")
            .position((FIELD_X + FIELD_W - 86, y - 1))
            .size((86, 26))
            .build(&mut self.btn_browse_exe)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "Arguments", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W, 24))
            .build(&mut self.in_args)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "Working folder", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W - 92, 24))
            .build(&mut self.in_workdir)?;
        nwg::Button::builder()
            .parent(&self.window)
            .text("Browse...")
            .position((FIELD_X + FIELD_W - 86, y - 1))
            .size((86, 26))
            .build(&mut self.btn_browse_dir)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "Check interval", y)?);
        nwg::ComboBox::builder()
            .parent(&self.window)
            .collection(format::interval_labels())
            .selected_index(Some(format::default_interval_index()))
            .position((FIELD_X, y))
            .size((150, 24))
            .build(&mut self.cb_interval)?;
        nwg::CheckBox::builder()
            .parent(&self.window)
            .text("Launch when the manager starts")
            .position((FIELD_X + 162, y + 2))
            .size((260, 22))
            .build(&mut self.chk_launch)?;
        y += ROW_H + 6;

        // Canh bao hien khi target la script co the tu khoi dong lai ben trong.
        nwg::Label::builder()
            .parent(&self.window)
            .text("")
            .position((14, y))
            .size((FIELD_X + FIELD_W - 14, 34))
            .build(&mut self.lbl_warning)?;
        y += 42;

        self.labels.push(label(&self.window, "Env (KEY=VALUE)", y)?);
        nwg::TextBox::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W, 84))
            .flags(text_box_flags())
            .build(&mut self.in_env)?;
        y += 92;

        self.labels.push(label(&self.window, "Env file", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W, 24))
            .build(&mut self.in_env_file)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "VAR=value file", y)?);
        nwg::TextBox::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W, 60))
            .flags(text_box_flags())
            .build(&mut self.in_env_from_files)?;
        y += 68;

        self.labels.push(label(&self.window, "Max retries", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((80, 24))
            .build(&mut self.in_max_retries)?;
        nwg::CheckBox::builder()
            .parent(&self.window)
            .text("HTTP check")
            .position((FIELD_X + 96, y + 2))
            .size((130, 22))
            .build(&mut self.chk_health)?;
        y += ROW_H;

        self.labels.push(label(&self.window, "Health URL", y)?);
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X, y))
            .size((FIELD_W - 108, 24))
            .build(&mut self.in_health_url)?;
        nwg::TextInput::builder()
            .parent(&self.window)
            .position((FIELD_X + FIELD_W - 100, y))
            .size((100, 24))
            .build(&mut self.in_health_fails)?;
        y += 26;

        self.labels.push(label(
            &self.window,
            "Right-hand box: consecutive failures before a restart",
            y,
        )?);
        // Nhan giai thich dai hon o nhan thuong nen noi rong ra het hang.
        if let Some(last) = self.labels.last() {
            last.set_size((FIELD_X + FIELD_W - 14) as u32, 20);
        }
        y += 34;

        nwg::Button::builder()
            .parent(&self.window)
            .text("Save")
            .position((FIELD_X + FIELD_W - 192, y))
            .size((92, 30))
            .build(&mut self.btn_ok)?;
        nwg::Button::builder()
            .parent(&self.window)
            .text("Cancel")
            .position((FIELD_X + FIELD_W - 94, y))
            .size((92, 30))
            .build(&mut self.btn_cancel)?;

        nwg::FileDialog::builder()
            .action(nwg::FileDialogAction::Open)
            .title("Select executable")
            .filters("App(*.exe;*.cmd;*.bat;*.js)|All files(*.*)")
            .build(&mut self.dlg_exe)?;
        nwg::FileDialog::builder()
            .action(nwg::FileDialogAction::OpenDirectory)
            .title("Select working folder")
            .build(&mut self.dlg_dir)?;

        Ok(())
    }

    pub fn is_browse_exe(&self, h: &nwg::ControlHandle) -> bool {
        *h == self.btn_browse_exe.handle
    }

    pub fn is_browse_dir(&self, h: &nwg::ControlHandle) -> bool {
        *h == self.btn_browse_dir.handle
    }

    pub fn is_exe_field(&self, h: &nwg::ControlHandle) -> bool {
        *h == self.in_exe.handle
    }

    pub fn is_visible(&self) -> bool {
        self.window.visible()
    }

    /// Dua form len truoc. Cua so cha dang bi vo hieu hoa de gia lam modal, nen
    /// day la cua duy nhat con nhan duoc thao tac.
    pub fn focus(&self) {
        self.window.restore();
        self.window.set_focus();
    }

    /// Mo form o che do them moi.
    pub fn open_new(&self, default_interval: u64) {
        self.editing_id.set(None);
        let blank = ManagedApp {
            check_interval_secs: default_interval,
            ..Default::default()
        };
        *self.carried.borrow_mut() = blank.clone();
        self.fill(&blank);
        self.window.set_text("Add app");
        self.show();
    }

    /// Mo form o che do sua.
    pub fn open_edit(&self, app: &ManagedApp) {
        self.editing_id.set(Some(app.id));
        *self.carried.borrow_mut() = app.clone();
        self.fill(app);
        self.window.set_text(&format!("Edit: {}", app.name));
        self.show();
    }

    fn show(&self) {
        self.refresh_warning();
        self.window.set_visible(true);
        self.in_name.set_focus();
    }

    pub fn hide(&self) {
        self.window.set_visible(false);
    }

    pub fn editing_id(&self) -> Option<u64> {
        self.editing_id.get()
    }

    fn fill(&self, app: &ManagedApp) {
        self.in_name.set_text(&app.name);
        self.in_exe.set_text(&app.exe.to_string_lossy());
        self.in_args.set_text(&app.args);
        self.in_workdir.set_text(&opt_path_text(&app.working_dir));
        self.cb_interval
            .set_selection(Some(format::interval_index(app.check_interval_secs)));
        self.chk_launch.set_check_state(check_state(app.launch_on_start));

        self.in_env.set_text(&kv_to_lines(&app.env));
        self.in_env_file.set_text(&opt_path_text(&app.env_file));
        self.in_env_from_files.set_text(&paths_to_lines(&app.env_from_files));
        self.in_max_retries.set_text(&app.restart.max_retries.to_string());

        match &app.health {
            Some(h) => {
                self.chk_health.set_check_state(nwg::CheckBoxState::Checked);
                self.in_health_url.set_text(&h.url);
                self.in_health_fails
                    .set_text(&h.failures_before_restart.to_string());
            }
            None => {
                self.chk_health.set_check_state(nwg::CheckBoxState::Unchecked);
                self.in_health_url.set_text("http://127.0.0.1:8080/health");
                self.in_health_fails
                    .set_text(&HealthCheck::default().failures_before_restart.to_string());
            }
        }
    }

    /// Canh bao khi target la script co the chua vong tu khoi dong lai.
    ///
    /// Script kieu do khong bao gio thoat, nen job luon con tien trinh va
    /// keepalive tro thanh vo dung. Day la cai bay that da gap o he cu.
    pub fn refresh_warning(&self) {
        let exe = self.in_exe.text().to_ascii_lowercase();
        let risky = [".cmd", ".bat", ".ps1", ".vbs", ".vbe", ".wsf"]
            .iter()
            .any(|ext| exe.ends_with(ext));
        self.lbl_warning.set_text(if risky {
            "Note: if this script has its own restart loop inside, it never exits\r\nand supervision stops working. Point straight at the real process (node/bun/python)."
        } else {
            ""
        });
    }

    pub fn browse_exe(&self) {
        if !self.dlg_exe.run(Some(&self.window)) {
            return;
        }
        let Ok(picked) = self.dlg_exe.get_selected_item() else {
            return;
        };
        let path = PathBuf::from(picked);
        self.in_exe.set_text(&path.to_string_lossy());

        // Thu muc lam viec gan nhu luon la thu muc chua file chay, va ten app
        // thuong trung ten file: dien san de bot thao tac, nhung khong ghi de
        // gia tri user da tu nhap.
        if self.in_workdir.text().trim().is_empty() {
            if let Some(dir) = path.parent() {
                self.in_workdir.set_text(&dir.to_string_lossy());
            }
        }
        if self.in_name.text().trim().is_empty() {
            if let Some(stem) = path.file_stem() {
                self.in_name.set_text(&stem.to_string_lossy());
            }
        }
        self.refresh_warning();
    }

    pub fn browse_dir(&self) {
        if !self.dlg_dir.run(Some(&self.window)) {
            return;
        }
        if let Ok(picked) = self.dlg_dir.get_selected_item() {
            self.in_workdir
                .set_text(&PathBuf::from(picked).to_string_lossy());
        }
    }

    /// Doc form ra `ManagedApp`, hoac tra ve thong bao loi de hien cho user.
    pub fn collect(&self) -> Result<ManagedApp, String> {
        let original = self.carried.borrow().clone();
        let name = keep_if_untouched(self.in_name.text().trim(), &original.name);
        if name.is_empty() {
            return Err("Name must not be empty.".into());
        }

        let exe_text = keep_if_untouched(
            self.in_exe.text().trim(),
            &original.exe.to_string_lossy(),
        );
        if exe_text.is_empty() {
            return Err("No executable selected.".into());
        }
        let exe = PathBuf::from(&exe_text);
        // Chuoi khong co dau tach thu muc duoc coi la lenh tim theo PATH
        // (vi du `node`), nen khong bat buoc phai ton tai tren dia.
        let looks_like_path = exe_text.contains('\\') || exe_text.contains('/');
        if looks_like_path && !exe.exists() {
            return Err(format!("File not found: {exe_text}"));
        }

        let workdir_text = keep_if_untouched(
            self.in_workdir.text().trim(),
            &opt_path_text(&original.working_dir),
        );
        let working_dir = match workdir_text.trim() {
            "" => None,
            dir => {
                let path = PathBuf::from(dir);
                if !path.is_dir() {
                    return Err(format!("Working folder does not exist: {dir}"));
                }
                Some(path)
            }
        };

        let max_retries: u32 = self
            .in_max_retries
            .text()
            .trim()
            .parse()
            .map_err(|_| "Max retries must be a non-negative integer (0 = unlimited).")?;

        // File env thieu lam `cfg_env::resolve` hong o **moi** lan spawn, nen
        // app dot het so lan thu roi nam CrashLooping vi mot cai go nham duong
        // dan. Kiem ngay tai day, giong `exe` va thu muc lam viec.
        let env_file = match keep_if_untouched(
            self.in_env_file.text().trim(),
            &opt_path_text(&original.env_file),
        )
        .trim()
        {
            "" => None,
            f => {
                let path = PathBuf::from(f);
                if !path.is_file() {
                    return Err(format!("Env file not found: {f}"));
                }
                Some(path)
            }
        };

        let env_from_files = lines_to_paths(&self.in_env_from_files.text());
        for (var, path) in &env_from_files {
            if !path.is_file() {
                return Err(format!(
                    "Value file not found for {var}: {}",
                    path.display()
                ));
            }
        }

        let health = self.collect_health()?;

        let carried = self.carried.borrow();
        Ok(ManagedApp {
            id: carried.id,
            name,
            exe,
            args: keep_if_untouched(self.in_args.text().trim(), &original.args),
            working_dir,
            enabled: carried.enabled,
            launch_on_start: self.chk_launch.check_state() == nwg::CheckBoxState::Checked,
            check_interval_secs: format::interval_or_original(
                self.cb_interval.selection(),
                carried.check_interval_secs,
            ),
            restart: RestartPolicy {
                max_retries,
                ..carried.restart.clone()
            },
            env: {
                let text = self.in_env.text();
                let mut typed = cfg_env::parse_kv_lines(&text);
                merge_multiline_env(&mut typed, &original.env, &text);
                typed
            },
            env_file,
            env_from_files,
            health,
        })
    }

    fn collect_health(&self) -> Result<Option<HealthCheck>, String> {
        if self.chk_health.check_state() != nwg::CheckBoxState::Checked {
            return Ok(None);
        }

        let base_url = self
            .carried
            .borrow()
            .health
            .as_ref()
            .map(|h| h.url.clone())
            .unwrap_or_default();
        let url = keep_if_untouched(self.in_health_url.text().trim(), &base_url);
        // Kiem tra bang chinh bo phan tich cua probe. Chi so khop tien to
        // `http://` thi `"http://"` tran cung lot qua, roi moi lan probe deu
        // hong am tham thay vi bao loi ngay tai form.
        if let Err(e) = health::validate_url(&url) {
            return Err(format!(
                "Invalid health URL: {e}. The probe does no TLS, so only http:// is accepted."
            ));
        }
        let failures_before_restart: u32 = self
            .in_health_fails
            .text()
            .trim()
            .parse()
            .map_err(|_| "Failures before restart must be an integer.")?;
        if failures_before_restart == 0 {
            return Err("Failures before restart must be greater than 0.".into());
        }

        // Cac truong con lai giu nguyen tu config cu de gia tri chinh tay
        // trong file khong bi form ghi de ve mac dinh.
        let base = self.carried.borrow().health.clone().unwrap_or_default();
        Ok(Some(HealthCheck {
            url,
            failures_before_restart,
            ..base
        }))
    }
}

fn text_box_flags() -> nwg::TextBoxFlags {
    nwg::TextBoxFlags::VISIBLE | nwg::TextBoxFlags::TAB_STOP | nwg::TextBoxFlags::VSCROLL
}

fn label(parent: &nwg::Window, text: &str, y: i32) -> Result<nwg::Label, nwg::NwgError> {
    let mut out = nwg::Label::default();
    nwg::Label::builder()
        .parent(parent)
        .text(text)
        .position((14, y + 4))
        .size((LABEL_W, 20))
        .build(&mut out)?;
    Ok(out)
}

fn check_state(on: bool) -> nwg::CheckBoxState {
    if on {
        nwg::CheckBoxState::Checked
    } else {
        nwg::CheckBoxState::Unchecked
    }
}

fn opt_path_text(path: &Option<PathBuf>) -> String {
    path.as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// TextBox cua Win32 chi xuong dong voi `\r\n`, khong phai `\n`.
fn kv_to_lines(map: &BTreeMap<String, String>) -> String {
    join_lines(map.iter().map(|(k, v)| {
        if is_multiline(v) {
            // Dinh dang mot dong mot cap khong cho duoc gia tri co xuong dong:
            // in ra thi lan doc lai cat mat phan sau va them mot dau nhay lac.
            // Hien thanh dong ghi chu (`parse_kv_lines` bo qua dong `#`) roi
            // ghep tra o `on_editor_save` de mot lan Luu khong dung toi no
            // khong lam hong gia tri.
            multiline_marker(k)
        } else {
            format!("{k}={}", quote_if_needed(v))
        }
    }))
}

/// Gia tri co xuong dong thi khong the di qua o van ban mot dong mot cap.
pub fn is_multiline(value: &str) -> bool {
    value.contains('\n') || value.contains('\r')
}

/// Dong ghi chu dai dien cho mot cap env nhieu dong tren form.
///
/// Vua la thong bao cho user, vua la dau moc: con dong nay thi cap do duoc giu
/// nguyen, xoa dong nay di la xoa han bien do. Khong co dau moc thi thao tac
/// xoa cua user bi nuot im lang.
fn multiline_marker(key: &str) -> String {
    format!("# {key}: multi-line, preserved, not editable here")
}

/// Ghep tra nhung cap nhieu dong ma user chua xoa dau moc cua chung.
fn merge_multiline_env(
    typed: &mut BTreeMap<String, String>,
    original: &BTreeMap<String, String>,
    text: &str,
) {
    for (key, value) in original {
        if !is_multiline(value) || typed.contains_key(key) {
            continue;
        }
        let marker = multiline_marker(key);
        if text.lines().any(|line| line.trim() == marker) {
            typed.insert(key.clone(), value.clone());
        }
    }
}

/// Cac o mot dong dang giu gia tri nhieu dong, tra ve nhan de bao cho user.
///
/// `keep_if_untouched` giu lai ban goc de mot lan Luu khong lam cut du lieu,
/// nhung giu am tham thi user khong hieu vi sao sua khong an. Bao ngay luc mo
/// form, kem chi dan sua o dau.
pub fn multiline_fields(app: &ManagedApp) -> Vec<&'static str> {
    let exe = app.exe.to_string_lossy();
    [
        ("Name", app.name.as_str()),
        ("Executable", exe.as_ref()),
        ("Arguments", app.args.as_str()),
    ]
    .into_iter()
    .filter(|(_, value)| is_multiline(value))
    .map(|(label, _)| label)
    .collect()
}

/// Tra lai gia tri goc khi user khong dung toi o do.
///
/// Mot `TextInput` la EDIT mot dong cua Win32: dat text co xuong dong vao thi
/// no cat tai ky tu xuong dong dau tien. Doc lai roi luu se ghi ban cut xuong
/// dia, du user chi vao form de doi mot truong khac han.
fn keep_if_untouched(typed: &str, original: &str) -> String {
    let first_line = original.lines().next().unwrap_or("").trim();
    if is_multiline(original) && typed == first_line {
        original.to_string()
    } else {
        typed.to_string()
    }
}

/// Boc nhay nhung gia tri ma `parse_kv_lines` se doi khi doc lai.
///
/// Doc lai co `trim` va `unquote`, nen mot gia tri co khoang trang dau/cuoi
/// hoac von da boc nhay se bi sua ngay lan dau user mo app roi bam Luu -- ke ca
/// khi ho chi doi moi cai ten.
fn quote_if_needed(value: &str) -> String {
    let altered = value != value.trim() || cfg_env::parse_kv_lines(&format!("K={value}"))
        .get("K")
        .map(|v| v != value)
        .unwrap_or(false);
    if altered {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

fn paths_to_lines(map: &BTreeMap<String, PathBuf>) -> String {
    join_lines(map.iter().map(|(k, v)| format!("{k}={}", v.display())))
}

fn join_lines(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join("\r\n")
}

fn lines_to_paths(text: &str) -> BTreeMap<String, PathBuf> {
    cfg_env::parse_kv_lines(text)
        .into_iter()
        .map(|(k, v)| (k, PathBuf::from(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_di_qua_lai_giua_map_va_van_ban_khong_mat_du_lieu() {
        let mut map = BTreeMap::new();
        map.insert("OCX_SERVICE".to_string(), "1".to_string());
        map.insert("PATH".to_string(), r"C:\a;C:\b".to_string());

        assert_eq!(cfg_env::parse_kv_lines(&kv_to_lines(&map)), map);
    }

    #[test]
    fn xoa_dau_moc_la_xoa_han_bien_nhieu_dong() {
        let mut original = BTreeMap::new();
        original.insert("PROMPT".to_string(), "dong 1
dong 2".to_string());

        // Con dau moc tren form -> cap duoc giu nguyen.
        let text = kv_to_lines(&original);
        let mut typed = cfg_env::parse_kv_lines(&text);
        merge_multiline_env(&mut typed, &original, &text);
        assert_eq!(typed.get("PROMPT"), original.get("PROMPT"));

        // User xoa dong dau moc -> phai xoa that, khong duoc ghep tra.
        let mut typed = cfg_env::parse_kv_lines("");
        merge_multiline_env(&mut typed, &original, "");
        assert!(typed.is_empty(), "{typed:?}");
    }

    #[test]
    fn o_mot_dong_khong_lam_cut_gia_tri_nhieu_dong() {
        // `TextInput` la EDIT mot dong: dat text nhieu dong vao thi Win32 cat
        // tai ky tu xuong dong dau tien.
        let goc = "--a
--b";
        assert_eq!(keep_if_untouched("--a", goc), goc, "khong dung toi thi giu nguyen");
        assert_eq!(keep_if_untouched("--c", goc), "--c", "go that thi lay theo user");
        assert_eq!(keep_if_untouched("x", "y"), "x", "gia tri mot dong khong bi dung toi");
    }

    #[test]
    fn gia_tri_nhieu_dong_khong_bi_cat_cut_khi_di_qua_form() {
        // `config.toml` viet tay cho phep chuoi nhieu dong. Truoc day mot vong
        // di-ve bien no thanh hai dong roi doc lai chi lay duoc nua dau kem mot
        // dau nhay lac -- va user chi vua doi moi cai ten.
        let mut map = BTreeMap::new();
        map.insert("PROMPT".to_string(), "dong 1
dong 2".to_string());
        map.insert("PLAIN".to_string(), "plain".to_string());

        let back = cfg_env::parse_kv_lines(&kv_to_lines(&map));
        assert_eq!(back.get("PLAIN").map(String::as_str), Some("plain"));
        assert!(!back.contains_key("PROMPT"), "{back:?}");
        assert_eq!(back.len(), 1, "khong duoc de lai manh vun: {back:?}");
    }

    #[test]
    fn gia_tri_co_nhay_hoac_khoang_trang_khong_bi_sua_khi_di_qua_form() {
        // Mo mot app roi bam Luu ma khong doi gi thi env phai y nguyen. Truoc
        // day `"quoted"` bi boc mat nhay va `" pad "` bi cat khoang trang.
        let mut map = BTreeMap::new();
        map.insert("QUOTED".to_string(), "\"quoted\"".to_string());
        map.insert("PADDED".to_string(), "  pad  ".to_string());
        map.insert("PLAIN".to_string(), "plain".to_string());
        assert_eq!(cfg_env::parse_kv_lines(&kv_to_lines(&map)), map);
    }

    #[test]
    fn duong_dan_di_qua_lai_khong_mat_du_lieu() {
        let mut map = BTreeMap::new();
        map.insert("TOKEN".to_string(), PathBuf::from(r"C:\x\service-api-token"));
        map.insert("KEY".to_string(), PathBuf::from(r"C:\y\key"));

        assert_eq!(lines_to_paths(&paths_to_lines(&map)), map);
    }

    #[test]
    fn text_box_xuong_dong_kieu_windows() {
        let mut map = BTreeMap::new();
        map.insert("A".to_string(), "1".to_string());
        map.insert("B".to_string(), "2".to_string());
        assert_eq!(kv_to_lines(&map), "A=1\r\nB=2");
    }

    #[test]
    fn map_rong_cho_van_ban_rong() {
        assert_eq!(kv_to_lines(&BTreeMap::new()), "");
        assert!(lines_to_paths("").is_empty());
    }
}

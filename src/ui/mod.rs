//! Giao dien: cua so quan ly + bieu tuong khay he thong.
//!
//! UI khong tu chay tien trinh nao. No chi sua config, ghi xuong dia va gui
//! lenh sang supervisor; moi thay doi trang thai deu ve nguoc lai qua
//! `SharedStatus` kem mot `Notice` de danh thuc vong su kien.
//!
//! Han che da biet cua nwg 1.0.13: `MenuItem` khong doi duoc nhan sau khi tao,
//! nen menu khay la co dinh. Thong tin song/chet duoc dua ra tooltip va bong
//! bao, con thao tac tren tung app nam trong cua so quan ly.

pub mod editor;
pub mod format;
mod taskbar;

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::mpsc::Sender;

use native_windows_gui as nwg;

use crate::config::{store, AppConfig, ManagedApp};
use crate::supervisor::{AppStatus, Command, SharedStatus, StatusKind};
use crate::{autostart, logging};

/// Id rieng cho subclass nghe `TaskbarCreated`. nwg giu rieng vung `<= 0xFFFF`
/// va panic neu bi lan.
const TASKBAR_HANDLER_ID: usize = 0x5341_4D01;

const WINDOW_W: i32 = 900;
const WINDOW_H: i32 = 520;
const TOOLBAR_Y: i32 = 430;
const BTN_H: i32 = 30;

/// Cot cua bang. Thu tu o day la thu tu hien tren man hinh.
const COLUMNS: [(&str, i32); 6] = [
    ("Name", 170),
    ("Status", 130),
    ("Processes", 80),
    ("Interval", 80),
    ("Restarts", 70),
    ("Executable", 330),
];

pub struct Manager {
    window: nwg::Window,
    list: nwg::ListView,

    btn_add: nwg::Button,
    btn_edit: nwg::Button,
    btn_delete: nwg::Button,
    btn_toggle: nwg::Button,
    btn_start: nwg::Button,
    btn_stop: nwg::Button,
    btn_restart: nwg::Button,
    chk_autostart: nwg::CheckBox,
    lbl_hint: nwg::Label,

    /// Dung lai duoc: Explorer khoi dong lai la phai them bieu tuong moi.
    tray: RefCell<nwg::TrayNotification>,
    tray_menu: nwg::Menu,
    mi_open: nwg::MenuItem,
    mi_pause_all: nwg::MenuItem,
    mi_resume_all: nwg::MenuItem,
    mi_exit: nwg::MenuItem,

    notice: nwg::Notice,
    editor: editor::Editor,

    /// Giu icon song suot vong doi cua so; Windows khong sao chep no.
    _icon: nwg::Icon,

    config: RefCell<AppConfig>,
    status: SharedStatus,
    tx: Sender<Command>,

    /// id cua app theo tung hang, de anh xa nguoc tu dong duoc chon.
    rows: RefCell<Vec<u64>>,
    /// App da bao bong; tranh bao lai moi lan refresh.
    warned: RefCell<HashSet<u64>>,
    /// Chan viec an vao khay khi user that su muon thoat.
    exiting: Cell<bool>,
}

/// Dung UI va tra ve dau gui `Notice` cho supervisor.
///
/// Supervisor duoc khoi dong sau UI vi no can dau gui nay; nguoc lai UI can
/// `tx` de gui lenh, nen kenh lenh phai duoc tao truoc ca hai.
pub fn build(
    config: AppConfig,
    status: SharedStatus,
    tx: Sender<Command>,
) -> Result<(Rc<Manager>, nwg::NoticeSender), nwg::NwgError> {
    let mut manager = Manager {
        window: Default::default(),
        list: Default::default(),
        btn_add: Default::default(),
        btn_edit: Default::default(),
        btn_delete: Default::default(),
        btn_toggle: Default::default(),
        btn_start: Default::default(),
        btn_stop: Default::default(),
        btn_restart: Default::default(),
        chk_autostart: Default::default(),
        lbl_hint: Default::default(),
        tray: Default::default(),
        tray_menu: Default::default(),
        mi_open: Default::default(),
        mi_pause_all: Default::default(),
        mi_resume_all: Default::default(),
        mi_exit: Default::default(),
        notice: Default::default(),
        editor: Default::default(),
        _icon: nwg::Icon::from_system(nwg::OemIcon::Sample),
        config: RefCell::new(config),
        status,
        tx,
        rows: RefCell::new(Vec::new()),
        warned: RefCell::new(HashSet::new()),
        exiting: Cell::new(false),
    };

    manager.build_controls()?;
    manager.editor.build()?;

    let sender = manager.notice.sender();
    let manager = Rc::new(manager);
    bind_events(&manager);
    manager.refresh();
    Ok((manager, sender))
}

/// Chay vong su kien cho den khi user thoat.
pub fn run_loop() {
    nwg::dispatch_thread_events();
}

impl Manager {
    fn build_controls(&mut self) -> Result<(), nwg::NwgError> {
        nwg::Window::builder()
            .size((WINDOW_W, WINDOW_H))
            .center(true)
            .title("Startup App Manager")
            .icon(Some(&self._icon))
            // Bo trong co dinh theo toa do nen khong cho keo gian: keo ra se
            // chi tao khoang trong chu khong lam bang rong ra.
            .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::MINIMIZE_BOX)
            .build(&mut self.window)?;

        nwg::ListView::builder()
            .parent(&self.window)
            .position((12, 12))
            .size((WINDOW_W - 40, TOOLBAR_Y - 24))
            .list_style(nwg::ListViewStyle::Detailed)
            .ex_flags(nwg::ListViewExFlags::FULL_ROW_SELECT | nwg::ListViewExFlags::GRID)
            .build(&mut self.list)?;
        for (index, (title, width)) in COLUMNS.iter().enumerate() {
            self.list.insert_column(nwg::InsertListViewColumn {
                index: Some(index as i32),
                fmt: None,
                width: Some(*width),
                text: Some((*title).to_string()),
            });
        }
        self.list.set_headers_enabled(true);

        let buttons: [(&str, &mut nwg::Button); 7] = [
            ("Add", &mut self.btn_add),
            ("Edit", &mut self.btn_edit),
            ("Delete", &mut self.btn_delete),
            ("Pause", &mut self.btn_toggle),
            ("Start", &mut self.btn_start),
            ("Stop", &mut self.btn_stop),
            ("Restart", &mut self.btn_restart),
        ];
        let mut x = 12;
        for (text, control) in buttons {
            let width = if text.len() > 8 { 110 } else { 92 };
            nwg::Button::builder()
                .parent(&self.window)
                .text(text)
                .position((x, TOOLBAR_Y))
                .size((width, BTN_H))
                .build(control)?;
            x += width + 6;
        }

        nwg::CheckBox::builder()
            .parent(&self.window)
            .text("Start with Windows")
            .position((12, TOOLBAR_Y + 40))
            .size((190, 22))
            .build(&mut self.chk_autostart)?;
        self.chk_autostart
            .set_check_state(if autostart::is_enabled() {
                nwg::CheckBoxState::Checked
            } else {
                nwg::CheckBoxState::Unchecked
            });

        nwg::Label::builder()
            .parent(&self.window)
            .text("Closing this window minimizes it to the tray. Exit from the tray right-click menu.")
            .position((214, TOOLBAR_Y + 42))
            .size((WINDOW_W - 250, 20))
            .build(&mut self.lbl_hint)?;

        nwg::TrayNotification::builder()
            .parent(&self.window)
            .icon(Some(&self._icon))
            .tip(Some("Startup App Manager"))
            .build(&mut self.tray.borrow_mut())?;

        nwg::Menu::builder()
            .popup(true)
            .parent(&self.window)
            .build(&mut self.tray_menu)?;
        for (text, item) in [
            ("Open manager window", &mut self.mi_open),
            ("Pause all", &mut self.mi_pause_all),
            ("Resume all", &mut self.mi_resume_all),
        ] {
            nwg::MenuItem::builder()
                .parent(&self.tray_menu)
                .text(text)
                .build(item)?;
        }
        let mut separator = nwg::MenuSeparator::default();
        nwg::MenuSeparator::builder()
            .parent(&self.tray_menu)
            .build(&mut separator)?;
        nwg::MenuItem::builder()
            .parent(&self.tray_menu)
            .text("Exit")
            .build(&mut self.mi_exit)?;
        // Separator khong can thao tac sau nay nhung phai song cung menu.
        std::mem::forget(separator);

        nwg::Notice::builder()
            .parent(&self.window)
            .build(&mut self.notice)?;

        Ok(())
    }

    /// Hien cua so quan ly (dung khi khoi dong khong co `--tray`).
    ///
    /// `restore` chu khong chi `set_visible`: `set_visible(true)` la `SW_SHOW`,
    /// khong bung mot cua so da thu nho, va `set_focus` cung khong. Cua so co
    /// nut thu nho nen day la duong cut that -- bam tray khong con cach nao
    /// goi lai cua so.
    pub fn show_window(&self) {
        // Form dang mo thi cua so chinh dang bi `set_enabled(false)` de gia lam
        // modal. Dua no len truoc se che mat form, va user nhin thay mot cua so
        // khong an bat ky cu bam nao -- giong het app treo.
        if self.editor.is_visible() {
            self.editor.focus();
            return;
        }
        self.window.set_visible(true);
        self.window.restore();
        self.window.set_focus();
    }

    // -- doc trang thai -----------------------------------------------------

    fn statuses(&self) -> Vec<AppStatus> {
        // Mutex bi nhiem doc chi khi supervisor panic; khi do UI van nen chay
        // tiep de user con thoat duoc, nen lay du lieu con lai thay vi panic.
        match self.status.lock() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    fn selected_id(&self) -> Option<u64> {
        let row = self.list.selected_item()?;
        self.rows.borrow().get(row).copied()
    }

    // -- ve lai bang --------------------------------------------------------

    fn refresh(&self) {
        let statuses = self.statuses();
        self.refresh_list(&statuses);
        self.tray.borrow().set_tip(&format::tray_tip(&statuses));
        self.notify_crash_loops(&statuses);
    }

    fn refresh_list(&self, statuses: &[AppStatus]) {
        let selected = self.selected_id();
        let config = self.config.borrow();

        // Ve lai toan bo thay vi cap nhat tung o: so app luon nho, con logic
        // dong bo tung dong thi de sinh loi lech hang.
        self.list.set_redraw(false);
        self.list.clear();
        let mut rows = Vec::with_capacity(config.apps.len());

        for (row, app) in config.apps.iter().enumerate() {
            let status = statuses.iter().find(|s| s.id == app.id);
            let cells = [
                app.name.clone(),
                status.map(format::status_text).unwrap_or_else(|| "-".into()),
                status
                    .map(|s| match s.active_procs {
                        Some(n) => n.to_string(),
                        None => "?".to_string(),
                    })
                    .unwrap_or_default(),
                format::interval_text(app.effective_check_interval_secs()),
                status.map(|s| s.restarts().to_string()).unwrap_or_default(),
                app.exe.to_string_lossy().into_owned(),
            ];
            for (column, text) in cells.into_iter().enumerate() {
                self.list.insert_item(nwg::InsertListViewItem {
                    index: Some(row as i32),
                    column_index: column as i32,
                    text: Some(text),
                });
            }
            rows.push(app.id);
        }

        let restored = selected.and_then(|id| rows.iter().position(|x| *x == id));
        *self.rows.borrow_mut() = rows;
        self.list.set_redraw(true);
        self.list.invalidate();

        if let Some(row) = restored {
            self.list.select_item(row, true);
        }
        drop(config);
        self.sync_toggle_label();
    }

    /// Nut tam dung doi nghia theo app dang chon nen phai doi nhan theo.
    fn sync_toggle_label(&self) {
        let paused = self
            .selected_id()
            .and_then(|id| self.config.borrow().find(id).map(|a| !a.enabled))
            .unwrap_or(false);
        self.btn_toggle
            .set_text(if paused { "Resume" } else { "Pause" });
    }

    /// Bao bong mot lan cho moi app roi vao trang thai bo cuoc.
    ///
    /// Day la trang thai duy nhat can den su chu y cua user: cac trang thai
    /// khac deu tu hoi phuc.
    fn notify_crash_loops(&self, statuses: &[AppStatus]) {
        let mut warned = self.warned.borrow_mut();
        for status in statuses {
            let broken = status.kind == StatusKind::CrashLooping;
            if broken && warned.insert(status.id) {
                let detail = status.last_error.as_deref().unwrap_or("unknown reason");
                self.tray.borrow().show(
                    &format!("{} gave up after {} attempts.\n{detail}", status.name, status.attempts),
                    Some("App failed to start"),
                    Some(nwg::TrayNotificationFlags::ERROR_ICON),
                    None,
                );
            } else if !broken {
                // Cho phep bao lai neu lan sau app lai hong.
                warned.remove(&status.id);
            }
        }
    }

    // -- thay doi config ----------------------------------------------------

    /// Ghi ban sua xuong dia, roi moi nhan no lam ban hien hanh.
    ///
    /// Ghi truoc, nhan sau: neu ghi that bai thi ca supervisor lan UI deu giu
    /// nguyen ban cu, dung bang voi file. Sua `self.config` truoc roi moi ghi
    /// se de lai canh te nhat khi ghi hong -- vi du xoa mot app: dong bien mat
    /// khoi bang nen khong con chon lai duoc, nhung file van con no va lan mo
    /// sau no hien lai.
    /// Tra `false` khi khong ghi duoc dia; khi do config song van y nguyen.
    fn commit(&self, next: AppConfig) -> bool {
        if let Err(e) = store::save(&next) {
            logging::error(&format!("cannot save config: {e}"));
            self.error(&format!("Cannot save config:\n{e}"));
            return false;
        }
        *self.config.borrow_mut() = next.clone();
        self.send(Command::Reload(Box::new(next)));
        self.refresh();
        true
    }

    fn send(&self, cmd: Command) {
        if self.tx.send(cmd).is_err() {
            logging::error("supervisor stopped, command not executed");
        }
    }

    fn error(&self, message: &str) {
        // Form dang mo thi cua so chinh dang bi `set_enabled(false)`. Gan hop
        // thoai vao no thi bam OK xong tieu diem khong quay ve form duoc, dung
        // luc user can thu luu lai.
        if self.editor.is_visible() {
            nwg::modal_error_message(&self.editor.window, "Startup App Manager", message);
            return;
        }
        nwg::modal_error_message(&self.window, "Startup App Manager", message);
    }

    // -- thao tac tu nut ----------------------------------------------------

    fn open_editor(&self, app: Option<ManagedApp>) {
        // Vo hieu hoa cua so cha thay cho modal that: nwg khong ho tro vong
        // dispatch long nhau. Phai chan **truoc** moi hop thoai ben duoi:
        // `modal_info_message` tu bom message cua thread va chi khoa cua so
        // chu cua no, nen cua so chinh con bam duoc -- user bam "Delete" luc do
        // la editor treo vao mot id da chet, luu xong mat trang khong bao gi.
        self.window.set_enabled(false);
        match app {
            Some(app) => {
                self.editor.open_edit(&app);
                let multiline = editor::multiline_fields(&app);
                if !multiline.is_empty() {
                    nwg::modal_info_message(
                        &self.editor.window,
                        "Startup App Manager",
                        &format!(
                            "These fields hold multi-line values, so only the first line is shown: {}.\n                             The form keeps the old values; edit config.toml to change them.",
                            multiline.join(", ")
                        ),
                    );
                }
            }
            None => {
                let interval = self.config.borrow().settings.default_check_interval_secs;
                self.editor.open_new(interval);
            }
        }
    }

    fn close_editor(&self) {
        self.editor.hide();
        self.window.set_enabled(true);
        self.window.set_focus();
    }

    fn on_add(&self) {
        self.open_editor(None);
    }

    fn on_edit(&self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let app = self.config.borrow().find(id).cloned();
        if let Some(app) = app {
            self.open_editor(Some(app));
        }
    }

    fn on_editor_save(&self) {
        let collected = match self.editor.collect() {
            Ok(app) => app,
            Err(message) => {
                nwg::modal_error_message(&self.editor.window, "Missing information", &message);
                return;
            }
        };

        let mut next = self.config.borrow().clone();
        match self.editor.editing_id() {
            Some(id) => {
                if let Some(slot) = next.apps.iter_mut().find(|a| a.id == id) {
                    // `enabled` khong co tren form; editor chi mang theo gia tri
                    // luc mo. Menu tray "Pause all" van bam duoc trong
                    // luc form dang mo, nen ghi lai anh chup cu se lang le huy
                    // thao tac do cho rieng app nay.
                    *slot = ManagedApp {
                        id,
                        enabled: slot.enabled,
                        ..collected
                    };
                }
            }
            None => {
                let id = next.allocate_id();
                next.apps.push(ManagedApp { id, ..collected });
            }
        }

        // Chi dong form khi da ghi duoc dia. Dong truoc roi moi luu thi mot
        // lan `save` hong (dia day, config.toml bi khoa) lam bay het nhung gi
        // user vua go: mo lai bang "Edit" se `fill()` de len tu config cu.
        if self.commit(next) {
            self.close_editor();
        }
    }

    fn on_delete(&self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let name = match self.config.borrow().find(id) {
            Some(app) => app.name.clone(),
            None => return,
        };

        let choice = nwg::modal_message(
            &self.window,
            &nwg::MessageParams {
                title: "Delete app",
                content: &format!("Remove \"{name}\" from the list?\nAny running process will be stopped."),
                buttons: nwg::MessageButtons::YesNo,
                icons: nwg::MessageIcons::Warning,
            },
        );
        if choice != nwg::MessageChoice::Yes {
            return;
        }

        // Dung truoc khi go khoi config: sau khi go, supervisor khong con biet
        // app nay de ma don tien trinh con cua no. Ghi hong thi app van con
        // trong danh sach va bam "Restart" duoc.
        self.send(Command::StopNow(id));
        self.warned.borrow_mut().remove(&id);

        let mut next = self.config.borrow().clone();
        next.apps.retain(|a| a.id != id);
        let _ = self.commit(next);
    }

    fn on_toggle(&self) {
        let Some(id) = self.selected_id() else {
            return;
        };
        let mut next = self.config.borrow().clone();
        let Some(app) = next.apps.iter_mut().find(|a| a.id == id) else {
            return;
        };
        app.enabled = !app.enabled;
        let _ = self.commit(next);
    }

    fn set_all_enabled(&self, enabled: bool) {
        let mut next = self.config.borrow().clone();
        for app in &mut next.apps {
            app.enabled = enabled;
        }
        let _ = self.commit(next);
    }

    fn on_selected_command(&self, make: fn(u64) -> Command) {
        let Some(id) = self.selected_id() else {
            return;
        };
        // Supervisor bo qua lenh khoi dong len app dang tam dung, con lenh dung
        // thi khong: khong noi gi o day thi hai nut tra ve hai ket qua khac
        // nhau tren cung mot dong ma man hinh khong he doi.
        let cmd = make(id);
        let starting = matches!(cmd, Command::StartNow(_) | Command::RestartNow(_));
        if starting && !self.config.borrow().find(id).is_some_and(|a| a.enabled) {
            nwg::modal_info_message(
                &self.window,
                "Startup App Manager",
                "This app is paused. Resume it before starting.",
            );
            return;
        }
        // "Start" co y khong dung toi app dang chay (giet mot service dang
        // phuc vu la mat du lieu dang xu ly), nhung im lang thi user chi thay
        // mot nut khong lam gi ca.
        // Phai trung dieu kien cua `Supervisor::start`: no chi tu choi khi
        // **con** tien trinh. Chan rong hon thi mot app vua chet trong chu ky
        // (bang hien "running" voi 0 tien trinh) khong con duong khoi dong tay,
        // phai cho het chu ky -- toi 24 gio o muc dai nhat.
        if matches!(cmd, Command::StartNow(_))
            && self.statuses().iter().any(|s| {
                s.id == id && s.kind == StatusKind::Running && s.active_procs != Some(0)
            })
        {
            nwg::modal_info_message(
                &self.window,
                "Startup App Manager",
                "This app is already running. Use \"Restart\" to spawn a fresh instance.",
            );
            return;
        }
        self.send(cmd);
    }

    fn on_autostart_toggle(&self) {
        let wanted = self.chk_autostart.check_state() == nwg::CheckBoxState::Checked;
        let result = if wanted {
            autostart::enable()
        } else {
            autostart::disable()
        };

        if let Err(e) = result {
            self.error(&format!("Cannot change the start-with-Windows setting:\n{e}"));
            // Tra o danh dau ve dung trang thai that trong registry.
            self.chk_autostart
                .set_check_state(if autostart::is_enabled() {
                    nwg::CheckBoxState::Checked
                } else {
                    nwg::CheckBoxState::Unchecked
                });
            return;
        }

        // Khong luu vao config: registry da la nguon su that, ghi them mot ban
        // sao chi tao co hoi cho hai noi noi khac nhau.
        logging::info(&format!(
            "start with Windows: {}",
            if wanted { "on" } else { "off" }
        ));
    }

    /// Them lai bieu tuong khay sau khi Explorer song lai.
    fn restore_tray(&self) {
        let Ok(mut tray) = self.tray.try_borrow_mut() else {
            // Dang o giua mot thao tac khay khac. Bo qua con hon panic: crate
            // build voi `panic = "abort"`, ma abort thi `KILL_ON_JOB_CLOSE`
            // giet luon moi service dang duoc giam sat.
            logging::warn("tray icon busy, skipping one redraw");
            return;
        };
        // Phai bo ban cu **truoc**: `TrayNotification::build` goi `NIM_ADD`
        // roi moi ghi de `out`, nen `Drop` cua ban cu chay sau va se xoa dung
        // bieu tuong vua them (cung hwnd, cung uid).
        *tray = Default::default();
        if let Err(e) = nwg::TrayNotification::builder()
            .parent(&self.window)
            .icon(Some(&self._icon))
            .tip(Some("Startup App Manager"))
            .build(&mut tray)
        {
            logging::error(&format!("cannot redraw the tray icon: {e}"));
            return;
        }
        drop(tray);
        // Tooltip chi duoc lam moi khi snapshot doi, nen dat lai ngay bay gio.
        self.tray
            .borrow()
            .set_tip(&format::tray_tip(&self.statuses()));
        logging::info("Explorer restarted, tray icon redrawn");
    }

    fn on_exit(&self) {
        self.exiting.set(true);
        self.send(Command::Shutdown);
        self.tray.borrow().set_visibility(false);
        // An cua so truoc khi bo vong dispatch: sau day `main` con ket o
        // `worker.join()` toi vai chuc giay (mot probe dang bay cong voi ngan
        // sach thu hoi cua tung app), ma cua so con hien thi thi khong ai bom
        // message nua -- Windows to xam no lai va ghi "Not Responding".
        self.editor.hide();
        self.window.set_visible(false);
        nwg::stop_thread_dispatch();
    }
}

fn bind_events(manager: &Rc<Manager>) {
    let ui = Rc::downgrade(manager);
    nwg::full_bind_event_handler(&manager.window.handle, move |event, data, handle| {
        let Some(ui) = ui.upgrade() else {
            return;
        };
        handle_main_event(&ui, event, &data, handle);
    });

    let ui = Rc::downgrade(manager);
    nwg::full_bind_event_handler(&manager.editor.window.handle, move |event, data, handle| {
        let Some(ui) = ui.upgrade() else {
            return;
        };
        handle_editor_event(&ui, event, &data, handle);
    });

    bind_taskbar_created(manager);
}

/// nwg khong dua `TaskbarCreated` ra thanh `Event` nao, nen phai nghe o muc
/// message tho. Khong nghe thi bieu tuong khay mat han sau lan Explorer khoi
/// dong lai dau tien, keo theo mat luon duong vao app.
fn bind_taskbar_created(manager: &Rc<Manager>) {
    let msg_id = taskbar::taskbar_created_message();
    if msg_id == 0 {
        logging::error("cannot register the TaskbarCreated message");
        return;
    }

    let ui = Rc::downgrade(manager);
    let bound = nwg::bind_raw_event_handler(
        &manager.window.handle,
        TASKBAR_HANDLER_ID,
        move |_hwnd, msg, _w, _l| {
            if msg == msg_id {
                if let Some(ui) = ui.upgrade() {
                    ui.restore_tray();
                }
            }
            // Tra `None` de nwg va Windows van xu ly message nhu thuong.
            None
        },
    );
    // `RawEventHandler` khong co `Drop`, nen tha roi no van giu nguyen subclass
    // -- dung y do: handler phai song het vong doi tien trinh.
    if let Err(e) = bound {
        logging::error(&format!("cannot listen for TaskbarCreated: {e}"));
    }
}

fn handle_main_event(
    ui: &Rc<Manager>,
    event: nwg::Event,
    data: &nwg::EventData,
    handle: nwg::ControlHandle,
) {
    use nwg::Event as E;

    match event {
        E::OnNotice if handle == ui.notice.handle => ui.refresh(),

        E::OnWindowClose if handle == ui.window.handle => {
            // Bam X la thu nho xuong khay chu khong phai thoat: keepalive phai
            // tiep tuc chay. Thoat han di qua menu "Exit" o khay.
            if let (false, nwg::EventData::OnWindowClose(close)) = (ui.exiting.get(), data) {
                close.close(false);
                ui.window.set_visible(false);
            }
        }

        E::OnButtonClick => match handle {
            h if h == ui.btn_add.handle => ui.on_add(),
            h if h == ui.btn_edit.handle => ui.on_edit(),
            h if h == ui.btn_delete.handle => ui.on_delete(),
            h if h == ui.btn_toggle.handle => ui.on_toggle(),
            h if h == ui.btn_start.handle => ui.on_selected_command(Command::StartNow),
            h if h == ui.btn_stop.handle => ui.on_selected_command(Command::StopNow),
            h if h == ui.btn_restart.handle => ui.on_selected_command(Command::RestartNow),
            h if h == ui.chk_autostart.handle => ui.on_autostart_toggle(),
            _ => {}
        },

        E::OnListViewDoubleClick if handle == ui.list.handle => ui.on_edit(),
        E::OnListViewItemChanged if handle == ui.list.handle => ui.sync_toggle_label(),

        E::OnMousePress(nwg::MousePressEvent::MousePressRightUp)
            if handle == ui.tray.borrow().handle =>
        {
            let (x, y) = nwg::GlobalCursor::position();
            ui.tray_menu.popup(x, y);
        }
        E::OnMousePress(nwg::MousePressEvent::MousePressLeftUp)
            if handle == ui.tray.borrow().handle =>
        {
            ui.show_window();
        }

        E::OnMenuItemSelected => match handle {
            h if h == ui.mi_open.handle => ui.show_window(),
            h if h == ui.mi_pause_all.handle => ui.set_all_enabled(false),
            h if h == ui.mi_resume_all.handle => ui.set_all_enabled(true),
            h if h == ui.mi_exit.handle => ui.on_exit(),
            _ => {}
        },

        _ => {}
    }
}

fn handle_editor_event(
    ui: &Rc<Manager>,
    event: nwg::Event,
    data: &nwg::EventData,
    handle: nwg::ControlHandle,
) {
    use nwg::Event as E;

    match event {
        E::OnWindowClose if handle == ui.editor.window.handle => {
            // Cua so cha dang bi vo hieu hoa; phai bat lai qua `close_editor`,
            // neu khong app se ket cung khong bam duoc gi.
            if let nwg::EventData::OnWindowClose(close) = data {
                close.close(false);
            }
            ui.close_editor();
        }

        E::OnButtonClick => match handle {
            h if h == ui.editor.btn_ok.handle => ui.on_editor_save(),
            h if h == ui.editor.btn_cancel.handle => ui.close_editor(),
            h if ui.editor.is_browse_exe(&h) => ui.editor.browse_exe(),
            h if ui.editor.is_browse_dir(&h) => ui.editor.browse_dir(),
            _ => {}
        },

        // Go tay duong dan cung phai lam canh bao wrapper cap nhat theo.
        E::OnTextInput if ui.editor.is_exe_field(&handle) => ui.editor.refresh_warning(),

        _ => {}
    }
}

//! Diem vao cua manager.
//!
//! Luong: chan ban trung -> doc config -> dung UI -> chay supervisor o thread
//! rieng -> vong su kien -> tat co trat tu.
//!
//! UI phai duoc dung truoc supervisor vi supervisor can `NoticeSender` de danh
//! thuc vong su kien moi khi trang thai doi.

// An cua so console o ban release. Ban debug giu console de doc `println!`
// va thong bao panic khi phat trien.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use startup_app_manager::supervisor::{Command, Supervisor};
use startup_app_manager::{autostart, config, logging, paths, single_instance, ui};

fn main() {
    let _instance = match single_instance::acquire() {
        Ok(guard) => guard,
        Err(e) => {
            // Ban thu hai khong duoc phep chay: hai supervisor cung giam sat
            // mot app se nhan doi so tien trinh moi lan sinh lai.
            logging::warn(&format!("{e}, exiting"));
            let message = match e {
                single_instance::AcquireError::AlreadyRunning => {
                    "The app is already running. Look for its system tray icon.".to_string()
                }
                // Khong noi doi rang app dang chay khi that ra he thong loi:
                // user se di tim mot bieu tuong khong ton tai.
                single_instance::AcquireError::Failed(e) => {
                    format!("Cannot start: {e}")
                }
            };
            // Ban `modal_*` doi mot cua so cha va panic neu khong co; o day
            // chua cua so nao ton tai nen phai dung ban khong cha.
            native_windows_gui::simple_message("Startup App Manager", &message);
            return;
        }
    };

    if let Err(e) = paths::ensure_dirs() {
        fatal(&format!(
            "Cannot create the data folder {}:\n{e}",
            paths::config_dir().display()
        ));
        return;
    }

    let cfg = config::store::load();
    logging::info(&format!(
        "startup: {} apps in config, start with Windows={}",
        cfg.apps.len(),
        autostart::is_enabled()
    ));

    if let Err(e) = native_windows_gui::init() {
        fatal(&format!("Cannot initialize the UI:\n{e}"));
        return;
    }
    native_windows_gui::Font::set_global_family("Segoe UI").ok();

    let status = Arc::new(Mutex::new(Vec::new()));
    let (tx, rx) = mpsc::channel::<Command>();

    let (manager, notify) = match ui::build(cfg.clone(), Arc::clone(&status), tx.clone()) {
        Ok(pair) => pair,
        Err(e) => {
            fatal(&format!("Cannot build the UI:\n{e}"));
            return;
        }
    };

    let supervisor = Supervisor::new(cfg, status, Arc::new(move || notify.notice()));
    let worker = std::thread::Builder::new()
        .name("supervisor".to_string())
        .spawn(move || supervisor.run(rx))
        .expect("cannot create the supervisor thread");

    if !started_in_tray() {
        manager.show_window();
    }

    ui::run_loop();

    // Vong su kien ket thuc nghia la user da chon Thoat. Bao supervisor dung
    // roi cho no don sach: `Supervisor::run` giet moi cay tien trinh con truoc
    // khi tra ve, nen bo qua buoc nay se de lai app mo coi.
    let _ = tx.send(Command::Shutdown);
    if worker.join().is_err() {
        logging::error("supervisor thread ended abnormally");
    }
    logging::info("exited");
}

/// `--tray` bao rang lan chay nay den tu Run key, khong phai user tu mo.
fn started_in_tray() -> bool {
    std::env::args().skip(1).any(|arg| arg == "--tray")
}

/// Bao loi khi chua co cua so nao de lam cha.
fn fatal(message: &str) {
    logging::error(message);
    native_windows_gui::error_message("Startup App Manager", message);
}

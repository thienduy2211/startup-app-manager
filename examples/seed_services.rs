//! Sinh config cho bon service dang chay that tren may nay.
//!
//! Cac gia tri o day duoc rut ra tu dinh nghia scheduled task va cac launcher
//! `.vbs`/`.cmd` cua he keepalive cu, va tro **thang vao tien trinh la** chu
//! khong qua wrapper: wrapper cua OpenCodex co vong `:loop` tu khoi dong lai
//! nen no khong bao gio thoat, tro vao do se lam keepalive mat tac dung.
//!
//! Khong file nao cua he cu bi sua; chung chi duoc doc de lay tham so.
//!
//! ```text
//! cargo run --example seed_services            # in ra TOML de xem truoc
//! cargo run --example seed_services -- --apply # ghi vao config that
//! ```

use std::collections::BTreeMap;
use std::path::PathBuf;

use startup_app_manager::config::{store, AppConfig, HealthCheck, ManagedApp};
use startup_app_manager::paths;

fn main() {
    let config = build_config();

    if std::env::args().any(|a| a == "--apply") {
        paths::ensure_dirs().expect("cannot create the config folder");
        let path = paths::config_file();
        if path.exists() {
            // Ten sao luu phai duy nhat: dung mot ten co dinh thi lan `--apply`
            // thu hai se ghi ban seed de len ban sao luu, va config goc cua
            // user mat han.
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let backup = path.with_extension(format!("toml.{stamp}.bak"));
            std::fs::copy(&path, &backup).expect("cannot back up the old config");
            println!("backed up the old config -> {}", backup.display());
        }
        store::save(&config).expect("cannot write the config");
        println!("wrote {} apps to {}", config.apps.len(), path.display());
    } else {
        let preview = std::env::temp_dir().join("sam-seed-preview.toml");
        store::save_to(&config, &preview).expect("cannot write the preview");
        println!("{}", std::fs::read_to_string(&preview).unwrap());
        println!("# preview; add --apply to write to {}", paths::config_file().display());
        std::fs::remove_file(preview).ok();
    }

    for app in &config.apps {
        report(app);
    }
}

fn build_config() -> AppConfig {
    let mut config = AppConfig::default();
    for app in [
        nine_router(),
        opencodex(),
        hermes_gateway(),
        hermes_webui(),
    ] {
        let id = config.allocate_id();
        config.apps.push(ManagedApp { id, ..app });
    }
    config
}

/// Nguon: task `9Router Background` -> `9Router-TaskLauncher.vbs` -> node cli.js.
fn nine_router() -> ManagedApp {
    ManagedApp {
        name: "9Router Background".into(),
        exe: PathBuf::from(r"C:\Program Files\nodejs\node.exe"),
        args: format!(
            "\"{}\" -n -t --skip-update",
            appdata().join(r"npm\node_modules\9router\cli.js").display()
        ),
        working_dir: Some(appdata().join(r"npm\node_modules\9router")),
        ..Default::default()
    }
}

/// Nguon: task `OpenCodex Service` -> `.vbs` -> `opencodex-service.cmd`.
///
/// Tro thang vao `bun.exe`, bo qua `opencodex-service.cmd`: script do co vong
/// `:loop` ngu 5 giay roi chay lai, nen tien trinh cmd ton tai vinh vien va
/// job se luon bao "con song" du service ben trong da hong.
fn opencodex() -> ManagedApp {
    let home = user_profile();
    let token_file = home.join(r".opencodex\service-api-token");

    let mut env = BTreeMap::new();
    env.insert("OCX_SERVICE".to_string(), "1".to_string());
    env.insert(
        "OCX_API_TOKEN_FILE".to_string(),
        token_file.display().to_string(),
    );
    // Wrapper dung `set "PATH=..."` nen cmd tu bung `%VAR%`; CreateProcess thi
    // khong, phai bung san truoc khi luu.
    env.insert("PATH".to_string(), opencodex_path());

    ManagedApp {
        name: "OpenCodex Service".into(),
        exe: home.join(r"Tools\opencodex\node_modules\bun\bin\bun.exe"),
        args: format!(
            "\"{}\" start --port 10100",
            home.join(r"Tools\opencodex\src\cli\index.ts").display()
        ),
        working_dir: Some(home.join(r"Tools\opencodex")),
        env,
        // `env_from_files` de trong co chu dich: wrapper cu boc viec nap token
        // trong `if exist`, va file token hien khong ton tai, nen service dang
        // chay khong co bien nay. Khai bao mot file khong co se lam spawn hong
        // ngay. Khi nao file duoc tao, them dong
        // `OPENCODEX_API_AUTH_TOKEN = <duong dan>` vao `[apps.env_from_files]`.
        health: Some(HealthCheck {
            url: "http://127.0.0.1:10100/health".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Nguon: task `Hermes_Gateway` -> `Hermes_Gateway.vbs`.
fn hermes_gateway() -> ManagedApp {
    let hermes = local_appdata().join("hermes");
    let agent = hermes.join("hermes-agent");

    let env = [
        ("HERMES_HOME", hermes.display().to_string()),
        ("PYTHONIOENCODING", "utf-8".to_string()),
        ("HERMES_GATEWAY_DETACHED", "1".to_string()),
        ("VIRTUAL_ENV", agent.join("venv").display().to_string()),
        ("PYTHONPATH", agent.display().to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();

    ManagedApp {
        name: "Hermes Gateway".into(),
        exe: agent.join(r"venv\Scripts\python.exe"),
        args: "-m hermes_cli.main gateway run".into(),
        working_dir: Some(hermes),
        env,
        ..Default::default()
    }
}

/// Nguon: task `Hermes_WebUI` -> `HermesWebUI-Supervisor.ps1`.
///
/// Script supervisor do co vong `while ($true)` ngu 5 giay roi chay lai, giong
/// het wrapper cua OpenCodex: tro vao no thi job luon con tien trinh va viec
/// giam sat mat tac dung. Tro thang vao `python.exe` cua moi truong ao.
///
/// Cau hinh doc tu `.env` cua repo qua `env_file` chu khong chep vao config:
/// file do chua `HERMES_WEBUI_PASSWORD`, nhan doi mot bi mat sang file thu hai
/// la tu tao ra hai noi co the lech nhau.
fn hermes_webui() -> ManagedApp {
    let repo = user_profile().join(r"Tools\hermes-webui");
    let agent = local_appdata().join(r"hermes\hermes-agent");

    ManagedApp {
        name: "Hermes WebUI".into(),
        exe: agent.join(r"venv\Scripts\python.exe"),
        args: format!("\"{}\"", repo.join("server.py").display()),
        working_dir: Some(repo.clone()),
        // Supervisor cu dat bien nay truoc khi chay; thieu no thi log cua
        // server bi dem theo bo dem va `app-<id>.log` gan nhu luon rong.
        env: [("PYTHONUNBUFFERED".to_string(), "1".to_string())]
            .into_iter()
            .collect(),
        env_file: Some(repo.join(".env")),
        health: Some(HealthCheck {
            url: "http://127.0.0.1:8787/health".into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// PATH tuy bien ma `opencodex-service.cmd` dung, da bung bien moi truong.
fn opencodex_path() -> String {
    let home = user_profile();
    let local = local_appdata();
    let roaming = appdata();

    let entries: Vec<String> = [
        home.join("bin").display().to_string(),
        r"C:\Program Files\Git\mingw64\bin".to_string(),
        r"C:\Program Files\Git\usr\local\bin".to_string(),
        r"C:\Program Files\Git\usr\bin".to_string(),
        local.join(r"hermes\node\bin").display().to_string(),
        local
            .join(r"hermes\hermes-agent\venv\Scripts")
            .display()
            .to_string(),
        roaming.join("npm").display().to_string(),
        roaming.join(r"Composer\vendor\bin").display().to_string(),
        home.join(r".cargo\bin").display().to_string(),
        home.join(r"go\bin").display().to_string(),
        r"C:\WINDOWS\system32".to_string(),
        r"C:\WINDOWS".to_string(),
        r"C:\WINDOWS\System32\Wbem".to_string(),
        r"C:\WINDOWS\System32\WindowsPowerShell\v1.0".to_string(),
        r"C:\WINDOWS\System32\OpenSSH".to_string(),
        r"C:\Program Files\Go\bin".to_string(),
        r"C:\Program Files\Git\cmd".to_string(),
        r"C:\ffmpeg\bin".to_string(),
        r"C:\Program Files\nodejs".to_string(),
        r"C:\Program Files\GitHub CLI".to_string(),
        local.join(r"agy\bin").display().to_string(),
        local.join(r"hermes\bin").display().to_string(),
        home.join(r".local\bin").display().to_string(),
        local.join(r"Microsoft\WindowsApps").display().to_string(),
        local.join(r"hermes\node").display().to_string(),
        r"C:\Program Files\Git\usr\bin\vendor_perl".to_string(),
        r"C:\Program Files\Git\usr\bin\core_perl".to_string(),
    ]
    .into_iter()
    .collect();

    entries.join(";")
}

fn env_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("thieu bien moi truong {name}")))
}

fn user_profile() -> PathBuf {
    env_path("USERPROFILE")
}

fn appdata() -> PathBuf {
    env_path("APPDATA")
}

fn local_appdata() -> PathBuf {
    env_path("LOCALAPPDATA")
}

/// Bao ngay neu duong dan trong config khong ton tai: sai o day thi service se
/// khong bao gio chay len duoc, va loi chi lo ra sau khi da go he cu.
fn report(app: &ManagedApp) {
    println!("# [{}] {}", app.id, app.name);
    println!("#   exe        : {} ({})", app.exe.display(), exists(&app.exe));
    if let Some(dir) = &app.working_dir {
        println!("#   working_dir: {} ({})", dir.display(), exists(dir));
    }
    if let Some(file) = &app.env_file {
        println!("#   env_file   : {} ({})", file.display(), exists(file));
    }
    for (var, path) in &app.env_from_files {
        println!("#   {var:<11}: {} ({})", path.display(), exists(path));
    }
}

fn exists(path: &std::path::Path) -> &'static str {
    if path.exists() {
        "OK"
    } else {
        "MISSING"
    }
}

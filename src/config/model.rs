//! Kieu du lieu cua config. Moi field deu co default de config cu van load
//! duoc khi phien ban moi them field.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

pub const DEFAULT_CHECK_INTERVAL_SECS: u64 = 300;
/// Chu ky ngan hon muc nay khong mang lai gi ngoai viec ton CPU vo ich.
pub const MIN_CHECK_INTERVAL_SECS: u64 = 10;

/// Chan tren cho chu ky kiem tra. `config.toml` duoc thiet ke de sua tay, va
/// `Instant + Duration` **panic** khi tran -- release dat `panic = "abort"` con
/// job co `KILL_ON_JOB_CLOSE`, nen mot con so go nham se giet luon moi service
/// dang duoc giam sat. Mot ngay la du dai cho bat ky chu ky hop ly nao.
pub const MAX_CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub settings: Settings,
    pub apps: Vec<ManagedApp>,
    /// Bo dem id, luu cung config. Dung counter thay vi `max(id) + 1` de id da
    /// xoa khong bao gio duoc cap lai: log `app-<id>.log` cua app cu se lan voi
    /// app moi neu id bi tai su dung.
    next_app_id: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            apps: Vec::new(),
            next_app_id: 1,
        }
    }
}

impl AppConfig {
    /// Cap phat id moi va tang bo dem. Goi khi them app.
    pub fn allocate_id(&mut self) -> u64 {
        // Config chinh tay co the dat `next_app_id` thap hon id dang dung;
        // nhay qua vung da dung de tranh trung.
        //
        // `saturating_add` vi mot id go nham sat tran se lam `highest + 1`
        // tran ve `0`: release tat `overflow-checks` nen khong co panic nao,
        // chi co mot id im lang trung voi app dang song.
        let highest = self.apps.iter().map(|a| a.id).max().unwrap_or(0);
        let mut id = self.next_app_id.max(highest.saturating_add(1));
        // Sat tran thi khong con id nao lon hon nua: lui ve khe trong nho nhat
        // con hon giao ra mot id trung -- hai app trung id la hai dong cung
        // dieu khien mot tien trinh, dung thu `dedupe_ids` sinh ra de go.
        if self.apps.iter().any(|a| a.id == id) {
            id = (0u64..)
                .find(|c| !self.apps.iter().any(|a| a.id == *c))
                .unwrap_or(id);
        }
        self.next_app_id = id.saturating_add(1);
        id
    }

    /// Cap id moi cho nhung app trung id, tra ve `(ten, id cu, id moi)`.
    ///
    /// `Supervisor::reload` gom runtime vao mot map theo id, nen hai app trung
    /// id thi mot cai lang le bien mat khoi vong giam sat: hai dong tren bang
    /// cung dieu khien mot tien trinh, va ca hai ghi chung mot `app-<id>.log`.
    /// Nhan doi mot khoi `[[apps]]` roi quen doi id la cach sua tay rat de xay
    /// ra, nen phai chua ngay luc nap.
    pub fn dedupe_ids(&mut self) -> Vec<(String, u64, u64)> {
        let mut seen = std::collections::HashSet::new();
        let mut changed = Vec::new();
        for i in 0..self.apps.len() {
            let id = self.apps[i].id;
            if seen.insert(id) {
                continue;
            }
            let fresh = self.allocate_id();
            changed.push((self.apps[i].name.clone(), id, fresh));
            self.apps[i].id = fresh;
            seen.insert(fresh);
        }
        changed
    }

    pub fn find(&self, id: u64) -> Option<&ManagedApp> {
        self.apps.iter().find(|a| a.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
/// Thiet lap chung, khong gan voi app nao.
///
/// Trang thai tu khoi dong **khong** nam o day: nguon su that duy nhat cua no
/// la HKCU Run key. Luu them mot ban sao trong config chi tao ra kha nang hai
/// noi noi khac nhau, va nguoi sua config bang tay se tuong minh doi duoc no.
pub struct Settings {
    /// Gia tri dien san khi them app moi.
    pub default_check_interval_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            default_check_interval_secs: DEFAULT_CHECK_INTERVAL_SECS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ManagedApp {
    pub id: u64,
    pub name: String,
    /// File thuc thi truc tiep (.exe), script (.cmd/.bat), hoac interpreter
    /// (node.exe, bun.exe, python.exe) voi script nam trong `args`.
    pub exe: PathBuf,
    /// Tham so dang raw, duoc tach khi spawn.
    pub args: String,
    pub working_dir: Option<PathBuf>,
    /// `false` nghia la tam dung: supervisor bo qua hoan toan.
    pub enabled: bool,
    /// Spawn ngay khi manager khoi dong.
    pub launch_on_start: bool,
    pub check_interval_secs: u64,
    pub restart: RestartPolicy,
    /// Env vars khai bao truc tiep. Uu tien cao nhat.
    pub env: BTreeMap<String, String>,
    /// File dang `KEY=VALUE` moi dong. Uu tien thap nhat.
    pub env_file: Option<PathBuf>,
    /// `VAR` nhan toan bo noi dung file lam gia tri (da trim). Danh cho file
    /// chua gia tri tran nhu token, khong phai dang `KEY=VALUE`. File goc
    /// khong bao gio bi sua.
    pub env_from_files: BTreeMap<String, PathBuf>,
    /// `None` nghia la chi kiem tra process con song hay khong.
    pub health: Option<HealthCheck>,
}

impl ManagedApp {
    /// Chu ky kiem tra thuc su duoc dung, sau khi chan hai dau.
    ///
    /// Bang cua UI phai doc qua day chu khong doc thang `check_interval_secs`:
    /// mot config sua tay ghi `2` van duoc kiem tra moi 10 giay, va hien "2
    /// giay" la noi doi voi user ve thu ho vua dat.
    pub fn effective_check_interval_secs(&self) -> u64 {
        self.check_interval_secs
            .clamp(MIN_CHECK_INTERVAL_SECS, MAX_CHECK_INTERVAL_SECS)
    }
}

impl Default for ManagedApp {
    fn default() -> Self {
        Self {
            id: 0,
            name: String::new(),
            exe: PathBuf::new(),
            args: String::new(),
            working_dir: None,
            enabled: true,
            launch_on_start: true,
            check_interval_secs: DEFAULT_CHECK_INTERVAL_SECS,
            restart: RestartPolicy::default(),
            env: BTreeMap::new(),
            env_file: None,
            env_from_files: BTreeMap::new(),
            health: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RestartPolicy {
    /// So lan thu lai lien tiep truoc khi bo cuoc. `0` nghia la khong gioi han.
    pub max_retries: u32,
    pub backoff_base_secs: u64,
    pub backoff_max_secs: u64,
}

impl Default for RestartPolicy {
    fn default() -> Self {
        Self {
            max_retries: 5,
            backoff_base_secs: 5,
            backoff_max_secs: 300,
        }
    }
}

/// Kiem tra suc khoe qua HTTP. Can cho app co the treo ma process van song,
/// vi du web server khong con phan hoi request nhung tien trinh chua chet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthCheck {
    /// Chi ho tro `http://`. Probe khong lam TLS.
    pub url: String,
    pub timeout_secs: u64,
    pub expect_status: u16,
    /// So lan fail lien tiep truoc khi restart. Mot lan nghen tam thoi khong
    /// duoc phep giet service.
    pub failures_before_restart: u32,
}

impl Default for HealthCheck {
    fn default() -> Self {
        Self {
            url: String::new(),
            timeout_secs: 3,
            expect_status: 200,
            failures_before_restart: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_moi_mac_dinh_la_bat_va_tu_chay() {
        let app = ManagedApp::default();
        assert!(app.enabled);
        assert!(app.launch_on_start);
        assert_eq!(app.check_interval_secs, DEFAULT_CHECK_INTERVAL_SECS);
        assert!(app.health.is_none());
    }

    #[test]
    fn id_khong_bao_gio_duoc_cap_lai_sau_khi_xoa() {
        let mut cfg = AppConfig::default();
        let a = cfg.allocate_id();
        let b = cfg.allocate_id();
        assert_eq!((a, b), (1, 2));

        cfg.apps.push(ManagedApp { id: a, ..Default::default() });
        cfg.apps.push(ManagedApp { id: b, ..Default::default() });

        // Xoa app co id cao nhat roi them app moi: id cu khong duoc dung lai,
        // neu khong log `app-2.log` cua hai app khac nhau se lan vao nhau.
        cfg.apps.retain(|x| x.id != b);
        assert_eq!(cfg.allocate_id(), 3);
    }

    #[test]
    fn id_sat_tran_khong_tran_ve_id_dang_dung() {
        let mut cfg = AppConfig {
            next_app_id: 1,
            apps: vec![
                ManagedApp { id: u64::MAX, ..Default::default() },
                ManagedApp { id: 1, ..Default::default() },
            ],
            ..Default::default()
        };
        let id = cfg.allocate_id();
        assert!(!cfg.apps.iter().any(|a| a.id == id), "id {id} trung app dang co");
    }

    #[test]
    fn allocate_id_nhay_qua_id_dang_dung_khi_config_bi_sua_tay() {
        let mut cfg = AppConfig {
            apps: vec![ManagedApp { id: 42, ..Default::default() }],
            ..Default::default()
        };
        // `next_app_id` mac dinh la 1 nhung id 42 dang duoc dung.
        assert_eq!(cfg.allocate_id(), 43);
    }

    #[test]
    fn app_trung_id_duoc_cap_id_moi() {
        let mut cfg = AppConfig {
            apps: vec![
                ManagedApp { id: 7, name: "goc".into(), ..Default::default() },
                ManagedApp { id: 7, name: "ban sao".into(), ..Default::default() },
                ManagedApp { id: 8, name: "khac".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let changed = cfg.dedupe_ids();

        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].0, "ban sao");
        let ids: Vec<u64> = cfg.apps.iter().map(|a| a.id).collect();
        assert_eq!(ids[0], 7, "app dau giu nguyen id, log cu khong bi lan");
        assert!(ids[1] > 8 && ids[1] != ids[2]);
    }

    #[test]
    fn chu_ky_hien_ra_la_chu_ky_that_su_duoc_dung() {
        let qua_ngan = ManagedApp { check_interval_secs: 2, ..Default::default() };
        assert_eq!(qua_ngan.effective_check_interval_secs(), MIN_CHECK_INTERVAL_SECS);

        let qua_dai = ManagedApp { check_interval_secs: 100_000, ..Default::default() };
        assert_eq!(qua_dai.effective_check_interval_secs(), MAX_CHECK_INTERVAL_SECS);

        let vua = ManagedApp { check_interval_secs: 300, ..Default::default() };
        assert_eq!(vua.effective_check_interval_secs(), 300);
    }

    #[test]
    fn find_tra_ve_dung_app() {
        let cfg = AppConfig {
            apps: vec![
                ManagedApp { id: 1, name: "a".into(), ..Default::default() },
                ManagedApp { id: 2, name: "b".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(cfg.find(2).unwrap().name, "b");
        assert!(cfg.find(99).is_none());
    }
}

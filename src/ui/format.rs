//! Chuyen doi giua gia tri trong config va chuoi hien tren man hinh.
//!
//! Tach rieng khoi phan dung control de kiem chung duoc bang test thuan,
//! khong can tao cua so.

use crate::config::model::DEFAULT_CHECK_INTERVAL_SECS;
use crate::supervisor::{AppStatus, StatusKind};

/// Cac chu ky cho chon, tinh bang giay.
///
/// Muc 30 giay chi de thu nghiem nhanh; cac muc con lai bam theo nhu cau thuc
/// te la vai phut mot lan, du de bat app chet ma khong ton CPU.
pub const INTERVAL_CHOICES: [u64; 8] = [30, 60, 120, 300, 600, 900, 1800, 3600];

pub fn interval_labels() -> Vec<String> {
    INTERVAL_CHOICES.iter().map(|s| interval_text(*s)).collect()
}

/// Chia het thi lam tron len don vi lon; con du thi ghi ca phan du.
///
/// Cot "Interval" doc qua day, nen lam tron xuong se noi doi dung kieu ma
/// `effective_check_interval_secs` sinh ra de chan: `119` tung hien "1 min".
pub fn interval_text(secs: u64) -> String {
    match secs {
        s if s < 60 => format!("{s} sec"),
        s if s % 3600 == 0 => format!("{} hr", s / 3600),
        s if s % 60 == 0 => format!("{} min", s / 60),
        s => format!("{} min {} sec", s / 60, s % 60),
    }
}

pub fn default_interval_index() -> usize {
    interval_index(DEFAULT_CHECK_INTERVAL_SECS)
}

/// Chon muc gan nhat voi `secs`.
///
/// Config sua bang tay co the chua gia tri khong nam trong danh sach; khi do
/// form hien muc gan nhat va luu lai theo muc do. Tha lam tron con hon hien
/// mot o trong khien user vo tinh luu gia tri rong.
pub fn interval_index(secs: u64) -> usize {
    INTERVAL_CHOICES
        .iter()
        .enumerate()
        .min_by_key(|(_, choice)| choice.abs_diff(secs))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

pub fn interval_from_index(index: Option<usize>) -> u64 {
    index
        .and_then(|i| INTERVAL_CHOICES.get(i).copied())
        .unwrap_or(DEFAULT_CHECK_INTERVAL_SECS)
}

/// Giu nguyen gia tri goc khi user khong dong den o chu ky.
///
/// Combo chi co 8 muc, con `config.toml` sua tay dat duoc bat ky so nao. Vi
/// `fill` hien muc gan nhat, ghi thang lua chon xuong se bien `7200` thanh
/// `3600` va `45` thanh `30` chi vi user mo form sua mot cai ten -- dung kieu
/// mat du lieu ma `keep_if_untouched` sinh ra de chan cho cac o van ban.
pub fn interval_or_original(selected: Option<usize>, original: u64) -> u64 {
    match selected {
        // Van dung muc ma `fill` da chon: user chua dong toi o nay.
        Some(i) if i == interval_index(original) => original,
        other => interval_from_index(other),
    }
}

/// Nhan trang thai kem chi tiet dang co ich khi doc bang.
pub fn status_text(status: &AppStatus) -> String {
    match status.kind {
        StatusKind::CrashLooping => {
            format!("{} ({} attempts)", status.kind.label(), status.attempts)
        }
        _ => status.kind.label().to_string(),
    }
}

/// Tom tat cho tooltip khay he thong.
///
/// Tooltip cua Windows bi cat o 127 ky tu nen chi dua so lieu tong, con chi
/// tiet tung app xem trong cua so quan ly.
pub fn tray_tip(statuses: &[AppStatus]) -> String {
    if statuses.is_empty() {
        return "Startup App Manager - no apps yet".to_string();
    }

    let running = statuses.iter().filter(|s| s.kind == StatusKind::Running).count();
    let paused = statuses.iter().filter(|s| s.kind == StatusKind::Paused).count();
    let broken = statuses
        .iter()
        .filter(|s| s.kind == StatusKind::CrashLooping)
        .count();

    let mut tip = format!("Startup App Manager\n{running}/{} running", statuses.len());
    if paused > 0 {
        tip.push_str(&format!(", {paused} paused"));
    }
    if broken > 0 {
        tip.push_str(&format!(", {broken} failing"));
    }
    truncate_chars(&tip, 127)
}

/// Cat theo ky tu chu khong theo byte: cat giua mot ky tu nhieu byte se tao
/// chuoi UTF-8 khong hop le va lam panic.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    s.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(kind: StatusKind, attempts: u32) -> AppStatus {
        AppStatus {
            id: 1,
            name: "x".into(),
            kind,
            active_procs: Some(0),
            launch_count: 0,
            attempts,
            last_error: None,
        }
    }

    #[test]
    fn chu_ky_le_phut_khong_bi_lam_tron_xuong() {
        assert_eq!(interval_text(30), "30 sec");
        assert_eq!(interval_text(60), "1 min");
        assert_eq!(interval_text(119), "1 min 59 sec");
        assert_eq!(interval_text(100), "1 min 40 sec");
        assert_eq!(interval_text(3600), "1 hr");
    }

    #[test]
    fn nhan_chu_ky_doc_duoc_theo_don_vi_tu_nhien() {
        assert_eq!(interval_text(30), "30 sec");
        assert_eq!(interval_text(60), "1 min");
        assert_eq!(interval_text(300), "5 min");
        assert_eq!(interval_text(1800), "30 min");
        assert_eq!(interval_text(3600), "1 hr");
    }

    #[test]
    fn moi_muc_deu_tro_ve_chinh_no() {
        for (i, secs) in INTERVAL_CHOICES.iter().enumerate() {
            assert_eq!(interval_index(*secs), i, "muc {secs}s");
            assert_eq!(interval_from_index(Some(i)), *secs);
        }
    }

    #[test]
    fn gia_tri_ngoai_danh_sach_duoc_giu_khi_user_khong_dong_toi() {
        // 7200 hien o muc 3600, 45 hien o muc 30; khong dong toi thi khong
        // duoc ghi de.
        assert_eq!(interval_or_original(Some(interval_index(7200)), 7200), 7200);
        assert_eq!(interval_or_original(Some(interval_index(45)), 45), 45);
        // Doi sang muc khac thi lay dung muc user chon.
        assert_eq!(interval_or_original(Some(0), 7200), 30);
    }

    #[test]
    fn gia_tri_la_duoc_lam_tron_ve_muc_gan_nhat() {
        assert_eq!(INTERVAL_CHOICES[interval_index(301)], 300);
        assert_eq!(INTERVAL_CHOICES[interval_index(7)], 30);
        assert_eq!(INTERVAL_CHOICES[interval_index(99999)], 3600);
    }

    #[test]
    fn khong_chon_gi_thi_lay_mac_dinh() {
        assert_eq!(interval_from_index(None), DEFAULT_CHECK_INTERVAL_SECS);
        assert_eq!(interval_from_index(Some(999)), DEFAULT_CHECK_INTERVAL_SECS);
        assert_eq!(
            INTERVAL_CHOICES[default_interval_index()],
            DEFAULT_CHECK_INTERVAL_SECS
        );
    }

    #[test]
    fn trang_thai_hong_kem_so_lan_da_thu() {
        assert_eq!(status_text(&status(StatusKind::Running, 0)), "running");
        assert_eq!(
            status_text(&status(StatusKind::CrashLooping, 6)),
            "crash-looping (6 attempts)"
        );
    }

    #[test]
    fn tooltip_dem_dung_tung_nhom() {
        let list = vec![
            status(StatusKind::Running, 0),
            status(StatusKind::Running, 0),
            status(StatusKind::Paused, 0),
            status(StatusKind::CrashLooping, 5),
        ];
        let tip = tray_tip(&list);
        assert!(tip.contains("2/4 running"), "{tip}");
        assert!(tip.contains("1 paused"), "{tip}");
        assert!(tip.contains("1 failing"), "{tip}");
    }

    #[test]
    fn tooltip_khong_vuot_gioi_han_cua_windows() {
        let list: Vec<_> = (0..500).map(|_| status(StatusKind::Paused, 0)).collect();
        assert!(tray_tip(&list).chars().count() <= 127);
    }

    #[test]
    fn tooltip_khi_chua_co_app_nao() {
        assert!(tray_tip(&[]).contains("no apps"));
    }
}

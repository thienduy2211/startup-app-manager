//! Gian cach giua cac lan thu khoi dong lai, va nguong bo cuoc.
//!
//! Khong co backoff thi mot app hong vinh vien se bi sinh lai moi chu ky, dot
//! CPU vo han. Ham o day thuan tuy nen kiem chung duoc bang test, khong can
//! cho doi thuc te.

use std::time::Duration;

use crate::config::RestartPolicy;

/// Gian cach truoc lan thu thu `attempt` (dem tu 1).
///
/// `base * 2^(attempt-1)`, chan tren boi `backoff_max_secs`.
pub fn delay_for(attempt: u32, policy: &RestartPolicy) -> Duration {
    let base = policy.backoff_base_secs;
    let max = policy.backoff_max_secs;

    if attempt <= 1 {
        return Duration::from_secs(base.min(max));
    }

    // `checked_shl` chan tran so mu; tran thi lay thang chan tren.
    let secs = match 1u64.checked_shl(attempt - 1) {
        Some(factor) => base.saturating_mul(factor).min(max),
        None => max,
    };
    Duration::from_secs(secs)
}

/// App da thu du so lan cho phep chua.
///
/// `max_retries == 0` nghia la thu mai khong bo cuoc.
pub fn is_crash_looping(attempt: u32, policy: &RestartPolicy) -> bool {
    policy.max_retries != 0 && attempt > policy.max_retries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> RestartPolicy {
        RestartPolicy {
            max_retries: 5,
            backoff_base_secs: 5,
            backoff_max_secs: 300,
        }
    }

    #[test]
    fn day_gian_cach_tang_gap_doi() {
        let p = policy();
        let secs: Vec<u64> = (1..=6).map(|a| delay_for(a, &p).as_secs()).collect();
        assert_eq!(secs, vec![5, 10, 20, 40, 80, 160]);
    }

    #[test]
    fn bi_chan_tren_boi_backoff_max() {
        let p = policy();
        assert_eq!(delay_for(7, &p).as_secs(), 300);
        assert_eq!(delay_for(8, &p).as_secs(), 300);
        assert_eq!(delay_for(1_000, &p).as_secs(), 300, "so mu lon phai chan lai");
    }

    #[test]
    fn khong_tran_so_voi_attempt_rat_lon() {
        let p = RestartPolicy {
            max_retries: 0,
            backoff_base_secs: u64::MAX / 2,
            backoff_max_secs: 600,
        };
        // Nhan tran phai roi ve chan tren thay vi wrap quanh.
        assert_eq!(delay_for(64, &p).as_secs(), 600);
        assert_eq!(delay_for(u32::MAX, &p).as_secs(), 600);
    }

    #[test]
    fn base_lon_hon_max_thi_lay_max_ngay_tu_lan_dau() {
        let p = RestartPolicy {
            max_retries: 5,
            backoff_base_secs: 900,
            backoff_max_secs: 300,
        };
        assert_eq!(delay_for(1, &p).as_secs(), 300);
    }

    #[test]
    fn bo_cuoc_sau_khi_vuot_max_retries() {
        let p = policy();
        assert!(!is_crash_looping(5, &p));
        assert!(is_crash_looping(6, &p));
    }

    #[test]
    fn max_retries_bang_khong_la_thu_mai() {
        let p = RestartPolicy {
            max_retries: 0,
            ..policy()
        };
        assert!(!is_crash_looping(1, &p));
        assert!(!is_crash_looping(10_000, &p));
    }
}

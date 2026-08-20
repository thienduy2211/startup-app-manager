//! Kiem tra suc khoe qua HTTP.
//!
//! Can thiet vi "tien trinh con song" khong dong nghia voi "service con phuc
//! vu duoc": mot web server co the treo ma tien trinh van ton tai.
//!
//! Dung `TcpStream` va tu viet request thay vi keo `reqwest`/`hyper` vao: cac
//! endpoint deu o `127.0.0.1` nen khong can TLS, va mot dependency HTTP day du
//! se lam phinh binary gap nhieu lan cho mot viec nho.

use std::io::{self, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::config::HealthCheck;

#[derive(Debug, PartialEq, Eq)]
pub enum ProbeError {
    /// URL sai dinh dang hoac dung scheme khong ho tro.
    BadUrl(String),
    /// Khong ket noi / khong doc duoc / het thoi gian cho.
    Unreachable(String),
    /// Ket noi duoc nhung ma trang thai khong nhu mong doi.
    UnexpectedStatus { got: u16, want: u16 },
}

impl std::fmt::Display for ProbeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProbeError::BadUrl(u) => write!(f, "invalid URL: {u}"),
            ProbeError::Unreachable(e) => write!(f, "unreachable: {e}"),
            ProbeError::UnexpectedStatus { got, want } => {
                write!(f, "status code {got}, expected {want}")
            }
        }
    }
}

/// `Ok(())` nghia la khoe.
/// Chan tren cho `timeout_secs`, la truong chi sua duoc bang tay trong
/// `config.toml` (form khong co o nay).
///
/// Chan nay phai nho: probe chay tren chinh luong giam sat duy nhat, nen suot
/// thoi gian cho mot endpoint nuot SYN thi khong app nao duoc quan sat va lenh
/// Thoat khong duoc doc -- tien trinh nan lai vo hinh sau khi UI da tat.
/// `MAX_INTERVAL_SECS` (mot ngay) chan duoc tran `Instant + Duration` nhung
/// khong chan duoc dieu do. Mac dinh la 3 giay, nen mot phut van rat rong rai.
const MAX_PROBE_TIMEOUT_SECS: u64 = 60;

pub fn probe(check: &HealthCheck) -> Result<(), ProbeError> {
    let target = parse_url(&check.url)?;
    let timeout = Duration::from_secs(check.timeout_secs.clamp(1, MAX_PROBE_TIMEOUT_SECS));

    let status = request_status(&target, timeout)
        .map_err(|e| ProbeError::Unreachable(e.to_string()))?;

    if status == check.expect_status {
        Ok(())
    } else {
        Err(ProbeError::UnexpectedStatus {
            got: status,
            want: check.expect_status,
        })
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Target {
    host: String,
    port: u16,
    path: String,
}

/// Kiem tra URL ngay luc user luu, thay vi de no hong am tham moi lan probe.
pub fn validate_url(url: &str) -> Result<(), ProbeError> {
    parse_url(url).map(|_| ())
}

/// Chi chap nhan `http://`. `https://` bi tu choi ro rang thay vi that bai kho
/// hieu luc chay, vi probe khong lam TLS.
fn parse_url(url: &str) -> Result<Target, ProbeError> {
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| ProbeError::BadUrl(url.to_string()))?;

    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(ProbeError::BadUrl(url.to_string()));
    }

    // IPv6 phai nam trong ngoac vuong; neu khong `rsplit_once(':')` se cat
    // ngay giua dia chi va bien mot URL hop le thanh loi.
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => {
            let (h, tail) = rest
                .split_once(']')
                .ok_or_else(|| ProbeError::BadUrl(url.to_string()))?;
            let port = match tail {
                "" => 80,
                t => t
                    .strip_prefix(':')
                    .and_then(|p| p.parse::<u16>().ok())
                    .ok_or_else(|| ProbeError::BadUrl(url.to_string()))?,
            };
            (h, port)
        }
        None => match authority.rsplit_once(':') {
            Some((h, p)) => {
                let port = p
                    .parse::<u16>()
                    .map_err(|_| ProbeError::BadUrl(url.to_string()))?;
                (h, port)
            }
            None => (authority, 80),
        },
    };
    if host.is_empty() {
        return Err(ProbeError::BadUrl(url.to_string()));
    }

    Ok(Target {
        host: host.to_string(),
        port,
        path: path.to_string(),
    })
}

/// Phan giai host thanh danh sach dia chi, khong bao gio cho lau hon `timeout`.
///
/// `to_socket_addrs` khong nhan timeout va co the treo vai giay khi DNS im
/// lang. Supervisor chi co mot thread: mot lan treo o day lam dong bang toan bo
/// viec giam sat va keo dai ca luc thoat, nen phai chan cung thoi gian cho.
fn resolve(host: &str, port: u16, timeout: Duration) -> io::Result<Vec<SocketAddr>> {
    // Duong pho bien nhat (`127.0.0.1`) khong can hoi he thong ten mien.
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let (tx, rx) = mpsc::channel();
    let owned = host.to_string();
    // Thread nay co the song lau hon lan cho; no tu ket thuc khi DNS tra loi,
    // va `send` that bai mot cach vo hai vi dau nhan da bi tha.
    std::thread::spawn(move || {
        let resolved = (owned.as_str(), port)
            .to_socket_addrs()
            .map(|addrs| addrs.collect::<Vec<_>>());
        let _ = tx.send(resolved);
    });

    let addrs = match rx.recv_timeout(timeout) {
        Ok(result) => result?,
        Err(mpsc::RecvTimeoutError::Disconnected) => Vec::new(),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "host resolution timed out",
            ))
        }
    };
    if addrs.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "cannot resolve host",
        ));
    }
    Ok(addrs)
}

/// Thoi gian con lai truoc han chot, toi thieu 1ms de khong bien thanh "cho
/// mai mai" -- `connect_timeout` tu choi Duration bang khong.
fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        return Err(io::Error::new(io::ErrorKind::TimedOut, "health check timed out"));
    }
    Ok(left.max(Duration::from_millis(1)))
}

/// Ket noi toi dia chi dau tien nhan ket noi.
///
/// Tren Windows `localhost` phan giai ra `::1` truoc `127.0.0.1`. Chi thu dia
/// chi dau tien nghia la mot service chi lang nghe tren IPv4 se luon bi tu choi
/// ket noi, va supervisor se giet mot service hoan toan khoe manh.
///
/// Moi dia chi chia nhau mot han chot chung: cap cho tung dia chi tron ven
/// `timeout` se lam ca lan kiem tra dai gap so dia chi phan giai ra.
fn connect_any(addrs: &[SocketAddr], deadline: Instant) -> io::Result<TcpStream> {
    let mut last = None;
    for (i, addr) in addrs.iter().enumerate() {
        let left = match remaining(deadline) {
            Ok(d) => d,
            Err(e) => return Err(last.unwrap_or(e)),
        };
        // Chia deu phan con lai cho cac dia chi chua thu. Cap tron ngan sach
        // cho dia chi dau thi mot `::1` bi chan (im lang chu khong tu choi) se
        // an het thoi gian va `127.0.0.1` khong bao gio duoc thu.
        let share = (left / (addrs.len() - i) as u32).max(Duration::from_millis(1));
        match TcpStream::connect_timeout(addr, share) {
            Ok(stream) => return Ok(stream),
            Err(e) => last = Some(e),
        }
    }
    Err(last.unwrap_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "no address to connect to")
    }))
}

/// `Host:` header phai boc IPv6 trong ngoac vuong, neu khong server co the tu
/// choi request.
fn host_header(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// `timeout` la han chot cho **ca** lan kiem tra, khong phai cho tung loi goi.
///
/// Han theo tung loi goi khong chan duoc gi: mot server nho giot mot byte ngay
/// truoc moi lan het gio se lam moi lan `read` deu "thanh cong" va giu vong
/// giam sat mot thread nay ban hang chuc phut -- khong app nao khac duoc quan
/// sat va lenh Thoat khong duoc doc.
fn request_status(target: &Target, timeout: Duration) -> io::Result<u16> {
    let deadline = Instant::now() + timeout;
    let addrs = resolve(&target.host, target.port, remaining(deadline)?)?;
    let mut stream = connect_any(&addrs, deadline)?;
    stream.set_read_timeout(Some(remaining(deadline)?))?;
    stream.set_write_timeout(Some(remaining(deadline)?))?;

    // `Connection: close` de server dong ngay, khoi phai doi keep-alive.
    let request = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: startup-app-manager\r\n\r\n",
        target.path,
        host_header(&target.host, target.port)
    );
    stream.write_all(request.as_bytes())?;
    stream.flush()?;

    // Doc den het dong dau tien: mot lan `read` khong bao dam mang du dong
    // trang thai, va `"HTTP/1.1 20"` bi cat giua se doc ra ma 20. Gioi han
    // buffer de mot response khong lo khong lam treo vong kiem tra.
    let mut buf = Vec::with_capacity(MAX_STATUS_BYTES);
    let mut chunk = [0u8; 256];
    loop {
        if let Some(status) = parse_status_line(&buf) {
            return Ok(status);
        }
        if buf.len() >= MAX_STATUS_BYTES {
            break;
        }
        stream.set_read_timeout(Some(remaining(deadline)?))?;
        match stream.read(&mut chunk)? {
            0 => break,
            n => buf.extend_from_slice(&chunk[..n]),
        }
    }
    parse_status_line(&buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "response is not HTTP"))
}

/// Dong trang thai HTTP dai nhat co the gap tren thuc te chi vai chuc byte;
/// qua nguong nay thi phan hoi khong phai HTTP.
const MAX_STATUS_BYTES: usize = 512;

/// Tach ma trang thai tu dong dau: `HTTP/1.1 200 OK`.
fn parse_status_line(bytes: &[u8]) -> Option<u16> {
    // Chi doc phan truoc CRLF dau tien. Giai ma UTF-8 ca buffer se hong khi
    // body la nhi phan, hoac khi mot ky tu nhieu byte bi cat doi o bien
    // buffer -- va mot service khoe manh se bi ket luan la hong.
    let end = bytes.windows(2).position(|w| w == b"\r\n")?;
    let line = std::str::from_utf8(&bytes[..end]).ok()?;
    let mut parts = line.split_whitespace();
    let version = parts.next()?;
    if !version.starts_with("HTTP/") {
        return None;
    }
    parts.next()?.parse::<u16>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn parse_url_day_du() {
        let t = parse_url("http://127.0.0.1:8787/health").unwrap();
        assert_eq!(t.host, "127.0.0.1");
        assert_eq!(t.port, 8787);
        assert_eq!(t.path, "/health");
    }

    #[test]
    fn parse_url_thieu_port_va_path() {
        let t = parse_url("http://localhost").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.path.as_str()), ("localhost", 80, "/"));
    }

    #[test]
    fn tu_choi_https_va_url_rac() {
        // Probe khong lam TLS nen phai bao loi ro thay vi that bai kho hieu.
        assert!(matches!(
            parse_url("https://127.0.0.1/health"),
            Err(ProbeError::BadUrl(_))
        ));
        assert!(matches!(parse_url("127.0.0.1:8787"), Err(ProbeError::BadUrl(_))));
        assert!(matches!(parse_url("http://"), Err(ProbeError::BadUrl(_))));
        assert!(matches!(
            parse_url("http://host:khong-phai-so/x"),
            Err(ProbeError::BadUrl(_))
        ));
    }

    #[test]
    fn dia_chi_ip_khong_phai_hoi_dns() {
        // Duong pho bien nhat phai tra loi ngay ca khi timeout bang khong,
        // chung to no khong di qua duong phan giai co the treo.
        let addrs = resolve("127.0.0.1", 8787, Duration::ZERO).unwrap();
        assert_eq!(addrs.len(), 1);
        assert_eq!(addrs[0].to_string(), "127.0.0.1:8787");
    }

    #[test]
    fn localhost_giu_du_ca_ipv4_lan_ipv6() {
        // Tren Windows `localhost` tra `::1` truoc. Neu chi giu dia chi dau
        // tien, mot service chi nghe IPv4 se bi coi la chet vinh vien.
        let addrs = resolve("localhost", 8787, Duration::from_secs(3)).unwrap();
        assert!(
            addrs.iter().any(|a| a.is_ipv4()),
            "mat dia chi IPv4 cua localhost: {addrs:?}"
        );
    }

    #[test]
    fn ten_mien_khong_phan_giai_duoc_bao_loi_thay_vi_treo() {
        let started = std::time::Instant::now();
        let err = resolve("khong-ton-tai.invalid", 80, Duration::from_secs(2)).unwrap_err();
        assert!(
            matches!(err.kind(), io::ErrorKind::TimedOut | io::ErrorKind::NotFound)
                || err.raw_os_error().is_some(),
            "loi la {err:?}"
        );
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "phan giai vuot qua thoi gian cho: vong giam sat se bi dong bang"
        );
    }

    #[test]
    fn parse_status_line_cac_truong_hop() {
        assert_eq!(parse_status_line(b"HTTP/1.1 200 OK\r\n\r\n"), Some(200));
        assert_eq!(parse_status_line(b"HTTP/1.0 503 Unavailable\r\n"), Some(503));
        assert_eq!(parse_status_line(b"khong phai http"), None);
        assert_eq!(parse_status_line(b""), None);
    }

    #[test]
    fn dong_trang_thai_cut_khong_duoc_doc_thanh_ma_khac() {
        // `"HTTP/1.1 20"` tung doc ra ma 20 va lam supervisor giet mot service
        // khoe manh. Chua thay CRLF thi chua duoc ket luan.
        assert_eq!(parse_status_line(b"HTTP/1.1 20"), None);
        assert_eq!(parse_status_line(b"HTTP/1.1"), None);
    }

    #[test]
    fn body_nhi_phan_khong_lam_hong_viec_doc_ma_trang_thai() {
        // Giai ma UTF-8 ca buffer se tra None o day, va mot service khoe manh
        // bi ket luan la hong.
        let mut bytes = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
        bytes.extend_from_slice(&[0xff, 0xfe, 0x00, 0x80]);
        assert_eq!(parse_status_line(&bytes), Some(200));
    }

    #[test]
    fn ipv6_trong_ngoac_vuong_duoc_chap_nhan() {
        let t = parse_url("http://[::1]/health").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", 80));
        let t = parse_url("http://[::1]:8787/health").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", 8787));
        // Host header phai giu ngoac vuong, neu khong server co the tu choi.
        assert_eq!(host_header("::1", 8787), "[::1]:8787");
        assert_eq!(host_header("127.0.0.1", 80), "127.0.0.1:80");
    }

    #[test]
    fn url_rong_bi_tu_choi_ngay_tai_form() {
        // `HealthCheck::default()` co url rong; chi so khop tien to `http://`
        // se cho `"http://"` lot qua roi hong am tham moi lan probe.
        assert!(validate_url("").is_err());
        assert!(validate_url("http://").is_err());
        assert!(validate_url("http://127.0.0.1:8787/health").is_ok());
    }

    /// Server tra dung mot ma trang thai, gui nho giot de mo phong dong bi cat.
    fn fake_server_nho_giot(status_line: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                drain_request(&mut stream);
                let full = format!("{status_line}\r\nConnection: close\r\n\r\nok");
                for chunk in full.as_bytes().chunks(5) {
                    if stream.write_all(chunk).is_err() {
                        return;
                    }
                    let _ = stream.flush();
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        (port, handle)
    }

    #[test]
    fn phan_hoi_den_tung_manh_van_doc_dung_ma() {
        let (port, h) = fake_server_nho_giot("HTTP/1.1 200 OK");
        let check = HealthCheck {
            url: format!("http://127.0.0.1:{port}/health"),
            ..Default::default()
        };
        assert_eq!(probe(&check), Ok(()));
        h.join().unwrap();
    }

    /// Doc het request truoc khi tra loi. Bo qua buoc nay thi Windows dong
    /// ket noi bang RST vi con du lieu chua doc, va client co the mat luon
    /// phan hoi da gui -- test se bap benh.
    fn drain_request(stream: &mut std::net::TcpStream) {
        let mut req = [0u8; 1024];
        let _ = stream.read(&mut req);
    }

    /// Server gia tra ve dung mot ma trang thai roi dong.
    fn fake_server(status_line: &'static str) -> (u16, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                drain_request(&mut stream);
                let body = "ok";
                let _ = write!(
                    stream,
                    "{status_line}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
            }
        });
        (port, handle)
    }

    #[test]
    fn server_tra_200_thi_khoe() {
        let (port, h) = fake_server("HTTP/1.1 200 OK");
        let check = HealthCheck {
            url: format!("http://127.0.0.1:{port}/health"),
            ..Default::default()
        };
        assert_eq!(probe(&check), Ok(()));
        h.join().unwrap();
    }

    #[test]
    fn server_tra_503_thi_bao_sai_trang_thai() {
        // Day chinh la truong hop tien trinh con song nhung service da treo.
        let (port, h) = fake_server("HTTP/1.1 503 Service Unavailable");
        let check = HealthCheck {
            url: format!("http://127.0.0.1:{port}/health"),
            ..Default::default()
        };
        assert_eq!(
            probe(&check),
            Err(ProbeError::UnexpectedStatus { got: 503, want: 200 })
        );
        h.join().unwrap();
    }

    #[test]
    fn server_nho_giot_vo_tan_van_bi_cat_theo_han_chot() {
        // Han theo tung loi goi khong chan duoc kieu server nay: moi `read`
        // deu "thanh cong" nen vong giam sat mot thread se bi giu hang chuc
        // phut. Phai co han chot cho ca lan kiem tra.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let done = Arc::new(AtomicBool::new(false));
        let stop = Arc::clone(&done);
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                drain_request(&mut stream);
                // Mot byte moi 100ms, khong bao gio den CRLF.
                while !stop.load(Ordering::Relaxed) {
                    if stream.write_all(b"H").is_err() {
                        return;
                    }
                    let _ = stream.flush();
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });

        let check = HealthCheck {
            url: format!("http://127.0.0.1:{port}/health"),
            timeout_secs: 1,
            ..Default::default()
        };
        let started = Instant::now();
        assert!(matches!(probe(&check), Err(ProbeError::Unreachable(_))));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "probe chay {:?}, dai hon nhieu so voi timeout 1 giay",
            started.elapsed()
        );

        done.store(true, Ordering::Relaxed);
        let _ = handle.join();
    }

    #[test]
    fn khong_ai_lang_nghe_thi_bao_unreachable() {
        // Cong 1 nam ngoai dai cong tam thoi cua Windows nen khong test nao
        // trong file nay bind trung. Lay mot cong tam thoi roi tha ra thi mot
        // test chay song song co the nhan dung cong do va lam test nay bap benh.
        let check = HealthCheck {
            url: "http://127.0.0.1:1/health".to_string(),
            timeout_secs: 1,
            ..Default::default()
        };
        assert!(matches!(probe(&check), Err(ProbeError::Unreachable(_))));
    }
}

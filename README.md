# Startup App Manager

App nhỏ cho Windows, giữ cho các app/service của bạn luôn chạy: tự khởi động
cùng Windows, chạy nền, kiểm tra định kỳ và tự mở lại khi service không còn.

Một file `.exe` duy nhất (~580 KB), không cần .NET, Python hay runtime nào khác.

## Làm được gì

- Thêm / sửa / xoá / tạm dừng app trong danh sách được quản lý
- Tự chạy cùng Windows qua `HKCU\...\Run` (không cần quyền admin)
- Chạy nền dưới biểu tượng khay, không hiện cửa sổ console
- Kiểm tra định kỳ theo chu kỳ tự chọn: 30 giây → 1 giờ (mặc định 5 phút)
- Service chết → tự mở lại, có backoff tăng dần và ngưỡng dừng khi hỏng liên tục
- Kiểm tra HTTP tuỳ chọn: process còn sống nhưng không phản hồi thì vẫn restart
- Tiêm biến môi trường, kể cả kiểu đọc nguyên nội dung một file làm giá trị
  (dành cho file token)

## Chạy được loại nào

| Loại | Khai báo |
|------|----------|
| `.exe` thường | `exe` = đường dẫn tới exe |
| Gói Node.js | `exe` = `node.exe`, `args` = `"...\cli.js" --flag`, hoặc trỏ thẳng `.js` |
| bun / python | `exe` = `bun.exe` / `python.exe`, script nằm trong `args` |
| `.cmd` / `.bat` | Chạy được, nhưng xem cảnh báo bên dưới |

> **Cảnh báo về script wrapper.** Nếu file `.cmd`/`.bat`/`.ps1` có vòng lặp tự
> khởi động lại bên trong thì nó **không bao giờ thoát**. Với app này, "còn
> tiến trình trong job" nghĩa là "còn sống", nên một wrapper như vậy sẽ luôn
> báo khoẻ kể cả khi service bên trong đã hỏng — keepalive mất hoàn toàn tác
> dụng. Hãy trỏ thẳng vào tiến trình thật (`node.exe`, `bun.exe`, `python.exe`).
> Form thêm/sửa sẽ nhắc bạn khi chọn loại file này.

## Cài

```powershell
cargo build --release
# copy target\release\startup-app-manager.exe tới nơi bạn muốn giữ cố định
```

Đường dẫn file `.exe` được ghi vào Run key, nên **đừng di chuyển file sau khi
bật tự khởi động** — nếu lỡ di chuyển, ô "Start with Windows" sẽ tự về trạng
thái tắt để bạn bật lại cho đúng chỗ mới.

## Dùng

Mở app → **Add** → chọn file chạy → đặt chu kỳ kiểm tra → **Save**.

| Thao tác | Ở đâu |
|----------|-------|
| Thêm/sửa/xoá/tạm dừng | Nút trên thanh công cụ của cửa sổ chính |
| Khởi động / dừng / khởi động lại ngay | Nút trên thanh công cụ |
| Tự chạy cùng Windows | Ô đánh dấu góc dưới trái |
| Thu nhỏ xuống khay | Bấm `X` trên cửa sổ |
| Pause all / Resume all, Exit | Chuột phải vào biểu tượng khay |

Bấm `X` là **thu nhỏ xuống khay**, không phải thoát — keepalive vẫn chạy. Muốn
thoát hẳn thì dùng **Exit** trong menu khay; khi đó mọi tiến trình con do app
sinh ra đều bị dọn sạch.

Chi tiết từng trường của form, cách trỏ đúng file chạy, env, health check và cách
sửa `config.toml` bằng tay: [`docs/huong-dan-su-dung.md`](docs/huong-dan-su-dung.md).

## File trên đĩa

```
%APPDATA%\StartupAppManager\
  config.toml           cấu hình, sửa tay được
  logs\manager.log      log của chính manager
  logs\app-<id>.log     stdout + stderr của từng app được quản lý
```

Sửa `config.toml` bằng tay cũng được; app đọc lại khi khởi động. Trường thiếu sẽ
lấy giá trị mặc định nên file cũ vẫn dùng được sau khi nâng cấp.

## Gỡ

1. Bỏ đánh dấu "Start with Windows" (hoặc xoá value `StartupAppManager` trong
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)
2. **Exit** từ menu khay
3. Xoá `%APPDATA%\StartupAppManager\` và file `.exe`

## Phát triển

```powershell
cargo test          # unit + integration; nhóm vòng đời chạy tiến trình thật, ~35 giây
cargo clippy --all-targets -- -D warnings
cargo build --release
```

Kiến trúc và mô hình luồng: xem [`docs/architecture.md`](docs/architecture.md).

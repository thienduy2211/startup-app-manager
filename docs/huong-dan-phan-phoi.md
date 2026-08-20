# Phân phối Startup App Manager

## Thành phần cần gửi

Bản release Windows x64 là binary portable. Thành phần bắt buộc duy nhất để
chạy manager là `startup-app-manager.exe`. Binary đã liên kết các thư viện Rust
cần thiết và không cần .NET, Python, Node.js hay một runtime ứng dụng khác.

Package chuẩn được tạo bởi `scripts/package.ps1` gồm:

- `startup-app-manager.exe` — chương trình chạy chính;
- `readme.txt` — hướng dẫn cài/chạy nhanh cho người nhận;
- `huong-dan-su-dung.md` — hướng dẫn cấu hình đầy đủ;
- `config.example.toml` — mẫu tùy chọn, không chứa đường dẫn hay secret của máy
  hiện tại;
- `sha256.txt` — checksum để kiểm tra binary sau khi sao chép.

Không đưa vào package:

- `target/`, `Cargo.toml`, `Cargo.lock`, source code, `.pdb` hoặc thư mục
  dependency build;
- `%APPDATA%\StartupAppManager\config.toml` của người gửi, vì file này có thể
  chứa đường dẫn tuyệt đối, env và secret;
- các file token, `.env`, service/app mà manager sẽ theo dõi. Những phần đó phải
  được cài và cấu hình riêng trên máy nhận.

## Tạo package

Thoát app nếu đang chạy, rồi chạy từ PowerShell tại thư mục project:

```powershell
.\scripts\package.ps1
```

Kết quả nằm trong `dist/`:

```text
dist/
  startup-app-manager-v0.1.0-windows-x64/
  startup-app-manager-v0.1.0-windows-x64.zip
```

Muốn đóng gói lại binary đã build mà không biên dịch lại:

```powershell
.\scripts\package.ps1 -SkipBuild
```

## Cách người nhận sử dụng

1. Giải nén ZIP vào một thư mục cố định, ví dụ
   `%LOCALAPPDATA%\Programs\StartupAppManager`.
2. Chạy `startup-app-manager.exe`.
3. Dùng **Add** để thêm các app/service trên máy nhận.
4. Chỉ bật **Start with Windows** sau khi đã đặt EXE ở vị trí cố định.

Manager tự tạo config và log tại `%APPDATA%\StartupAppManager`. Đường dẫn exe,
working folder, env file và token file trong config phải tồn tại trên máy nhận;
không thể bê nguyên config của máy khác nếu các đường dẫn không giống nhau.

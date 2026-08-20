# Kiến trúc

## Vấn đề cần giải

Giữ cho một tập app luôn chạy trên Windows, và phát hiện đúng lúc chúng không
còn chạy. Phần khó không nằm ở việc khởi động lại, mà ở việc **trả lời đúng câu
hỏi "app này còn sống không?"**.

## Quyết định nền: đếm tiến trình trong Job Object

Cách hiển nhiên là giữ handle của tiến trình con và hỏi `Child::try_wait()`.
Cách đó **sai** với nhiều app thật:

```
cmd /c launcher.cmd      <- tiến trình con trực tiếp, thoát sau 200ms
   └─ node server.js     <- app thật, chạy tiếp hàng giờ
```

`try_wait()` báo "đã chết" ngay khi `cmd.exe` thoát, trong khi app vẫn chạy.
Tin vào nó thì mỗi chu kỳ kiểm tra lại sinh thêm một bản mới → nhân bản vô hạn.

Nên mỗi app được đặt vào một **Job Object** riêng, và câu hỏi "còn sống không"
được trả lời bằng số tiến trình đang hoạt động trong job:

```rust
QueryInformationJobObject(job, JobObjectBasicAccountingInformation).ActiveProcesses > 0
```

Truy vấn **hỏng** thì kết quả là `Observation::Unknown`, không phải `Dead`.
Gộp lỗi thành `0` nghĩa là một trục trặc Win32 thoáng qua đủ để giết cả cây
tiến trình đang phục vụ — đúng kiểu sinh lại oan mà cách đếm này sinh ra để
chặn. Cùng nguyên tắc với nhánh `BadUrl` của health check: không biết thì
không kết luận.

Job cũng giải quyết luôn việc dọn dẹp: cờ `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
khiến đóng handle là cả cây tiến trình con bị giết, không sót tiến trình mồ côi.

Thứ tự tạo tiến trình là **treo → gán job → thả ra**, không phải tạo rồi gán.
`cmd.spawn()` trả về khi tiến trình con đã chạy; đúng loại target mà thiết kế
này phục vụ (`.cmd` chạy `start /b` rồi thoát) có thể kịp sinh tiến trình cháu
nằm ngoài job trong khe đó — và khi `cmd.exe` thoát, số đếm về 0, supervisor
kết luận "đã chết" rồi sinh thêm một bản nữa. `CREATE_SUSPENDED` đóng khe này
lại; `Job::resume` tìm thread chính qua snapshot vì `std::process::Child` không
lộ handle đó ra.

Hành vi này được chốt bằng test tích hợp trong `tests/detach_no_duplicate.rs`
để không ai vô tình đổi ngược lại sau này.

Hệ quả cần biết: một script wrapper tự có vòng khởi động lại bên trong sẽ không
bao giờ thoát, nên job luôn còn tiến trình và app này sẽ luôn coi là khoẻ. Form
thêm/sửa cảnh báo khi phát hiện target dạng `.cmd`/`.bat`/`.ps1`.

## Luồng dữ liệu

```
                    ┌──────────────── UI thread ────────────────┐
                    │                                            │
   config.toml ──► AppConfig ──► ui::Manager ──► ListView/tray   │
        ▲              │              ▲                          │
        │              │              │ Notice (đánh thức)       │
   store::save    Command (mpsc)  SharedStatus                    │
        │              │              │                          │
        └──────────────┼──────────────┼──────────────────────────┘
                       ▼              │
                 ┌──── supervisor thread ────┐
                 │  vòng lặp 1 giây:          │
                 │   drain lệnh               │
                 │   tick(now) ──► decide()   │
                 │   publish() ──► SharedStatus
                 └────────────┬───────────────┘
                              ▼
                   Job Object ──► tiến trình con
```

- **UI → supervisor**: kênh `mpsc` với `Command::{Reload, StartNow, StopNow,
  RestartNow, Shutdown}`. UI không bao giờ tự spawn tiến trình. `StartNow` là
  lệnh **không đụng tới app đang chạy** — giết một service đang phục vụ là mất
  việc đang xử lý; muốn sinh bản mới thì dùng `RestartNow`.
- **supervisor → UI**: `SharedStatus = Arc<Mutex<Vec<AppStatus>>>` cộng một
  `nwg::Notice`. `publish()` chỉ đánh thức UI khi ảnh chụp thật sự đổi, nên
  cửa sổ không bị vẽ lại mỗi giây.
- **Ghi trước, báo sau**: mọi thay đổi từ UI đều `store::save` xuống đĩa rồi mới
  gửi `Reload`. Ghi hỏng thì supervisor giữ nguyên bản cũ, tránh cảnh đang chạy
  một đằng mà file lưu một nẻo.

## Máy trạng thái

Quyết định chuyển trạng thái nằm trong hàm thuần `supervisor::decide` — không
chạm đĩa, không chạm tiến trình, không đọc đồng hồ hệ thống (nhận `now` làm
tham số). Nhờ vậy phần dễ sai nhất kiểm chứng được bằng test tất định.

```
              enabled = false                    was_running = true
   (mọi trạng thái) ─────────────► Paused ─────────────────────► Running
                                      └──was_running = false──► Stopped
                                                                 │
Pending ───────────────────► Running ──quan sát: Alive──────────┘
                                │
                                ├─ Dead / Unhealthy, còn lượt ──► Backoff ──hết hạn──► Running
                                ├─ Dead / Unhealthy, hết lượt ──► CrashLooping (dừng, chờ lệnh tay)
                                └─ nút Stop ───────────────────► Stopped (nằm yên, chờ lệnh tay)
```

`Pending` và `Stopped` với người dùng đều hiện là "stopped", nhưng `decide` đối
xử ngược nhau: `Pending` sinh ngay ở nhịp kế tiếp, `Stopped` không bao giờ tự
sinh. Gộp chung hai cái thì nút "Stop" bị nhịp kế tiếp xoá mất — app chết rồi
sống lại sau ~1 giây.

Nạp lại config còn một lối thoát riêng cho `CrashLooping`: app đã bỏ cuộc được
đưa về `Pending` khi `health` **hoặc** `restart` thay đổi. `restart_relevant` —
tập trường buộc phải sinh lại tiến trình — cố tình không gồm hai trường đó vì
đổi chúng không làm bản đang chạy sai đi. Nhưng đúng hai trường đó lại là thứ
người dùng sửa để gỡ một app crash-loop (URL health trỏ nhầm cổng, hoặc số lần
thử quá thấp), nên nếu chỉ dựa vào `restart_relevant` thì app nằm chết vĩnh
viễn dù người dùng đã sửa đúng.

`Paused` mang theo `was_running`: app trước khi tạm dừng có đang được mong đợi
là sống hay không (`Pending`/`Running`/`Backoff` là có; `Stopped` và
`CrashLooping` là không). Thiếu cờ đó thì "Tiếp tục tất cả" biến thành lệnh
khởi động: một app người dùng vừa bấm Dừng, hoặc đặt `launch_on_start = false`,
bị đánh thức chỉ vì đi qua một vòng tạm dừng.

`launch_on_start` chỉ quyết định trạng thái **khởi đầu** khi manager nạp config
(`Pending` hay `Stopped`); `decide` không đọc lại trường này, nên một app đã
dừng tay không bị cờ đó đánh thức.

- `Alive` trọn một chu kỳ sẽ **xoá lịch sử thất bại**, để một sự cố cũ không làm
  app bị bỏ cuộc sớm ở lần hỏng sau.
- Backoff: `min(base × 2^(lần thử - 1), max)`, mặc định 5s → 300s.
- `max_retries = 0` nghĩa là thử lại vô hạn, không bao giờ vào `CrashLooping`.
- Spawn hỏng tính là **một lần thất bại** và đi vào cùng đường backoff như khi
  quan sát thấy chết. Nếu vẫn nhận `Running`, UI sẽ báo "running" với 0 tiến
  trình và phải chờ hết một chu kỳ mới phát hiện ra.
- Vòng lặp đọc lệnh **giữa các app** chứ không chỉ một lần mỗi nhịp: probe chặn
  có thể ngốn vài giây mỗi app, và bấm Exit không được phép chờ hết lượt. Lệnh
  không phải `Shutdown` gom lại rồi áp dụng sau vòng duyệt, vì `reload` thêm bớt
  runtime sẽ làm hỏng vị trí đang duyệt.

## Quan sát sức khoẻ

Theo thứ tự, dừng ngay ở bước đầu tiên kết luận được:

1. Job còn tiến trình nào không → không còn là `Dead`.
2. Nếu app khai báo `health`: gọi HTTP `GET` và so mã trạng thái.

Bước 2 cần thiết cho web server có thể treo mà tiến trình vẫn sống. Một lần
nghẽn tạm thời không được phép giết service đang phục vụ, nên chỉ kết luận
`Unhealthy` sau `failures_before_restart` lần hỏng **liên tiếp**.

Lần hỏng chưa tới ngưỡng trả `Degraded`, **không phải** `Alive`. `Alive` xoá
lịch sử thất bại, nên nếu gộp chung thì bộ đếm `attempt` bị reset mỗi chu kỳ,
không bao giờ chạm `max_retries`, và một app hỏng kinh niên sẽ được sinh lại
vô hạn.

URL sai định dạng là lỗi config chứ không phải dấu hiệu service hỏng — sinh lại
app không sửa được URL. Probe bỏ qua và cảnh báo một lần; form chặn từ đầu bằng
chính bộ phân tích của probe.

Kết nối thử lần lượt **mọi** địa chỉ host phân giải ra. Trên Windows `localhost`
cho `::1` trước `127.0.0.1`, nên chỉ thử địa chỉ đầu sẽ giết vĩnh viễn một
service chỉ lắng nghe IPv4. Phân giải tên miền chạy trên thread riêng có hạn giờ
— `to_socket_addrs` không nhận timeout và một lần treo sẽ đóng băng cả vòng
giám sát một thread.

Probe tự viết trên `TcpStream` thay vì kéo thêm HTTP client: endpoint đều là
`127.0.0.1` nên không cần TLS, và đây là lý do binary giữ được kích thước nhỏ.
Đổi lại `https://` không được hỗ trợ và form sẽ từ chối.

## Biến môi trường

Gộp từ ba nguồn, nguồn sau ghi đè nguồn trước:

| Thứ tự | Nguồn | Dạng |
|--------|-------|------|
| 1 | `env_file` | file `KEY=VALUE` mỗi dòng |
| 2 | `env_from_files` | `VAR` ← **toàn bộ nội dung** một file, đã trim |
| 3 | `env` | khai báo thẳng trong config |

`env_from_files` sinh ra cho file token: nội dung là giá trị trần chứ không phải
`KEY=VALUE`. Trim là bắt buộc — file token thường có newline cuối, mà newline
lọt vào header HTTP sẽ làm hỏng request. File nguồn **chỉ được đọc, không bao
giờ bị ghi**, và đọc lại ở mỗi lần spawn nên token xoay vòng tự có hiệu lực.

## Mô hình luồng

| Luồng | Việc |
|-------|------|
| main / UI | vòng sự kiện Win32, sở hữu toàn bộ control và `AppConfig` |
| supervisor | vòng lặp 1 giây, sở hữu toàn bộ `Job` và handle tiến trình con |

`AppConfig` không dùng chung: UI giữ bản của mình, supervisor nhận bản sao qua
`Command::Reload`. Chỉ `SharedStatus` là dùng chung, và chỉ theo một chiều.

Nhịp 1 giây ngắn hơn chu kỳ kiểm tra rất nhiều, để lệnh từ UI được phản hồi
nhanh trong khi việc kiểm tra thật vẫn theo chu kỳ riêng của từng app. `decide`
nhận `observe` dưới dạng closure lười, nên mỗi nhịp không tốn một lời gọi Win32
và một request HTTP vô ích.

Thoát: vòng sự kiện dừng → `main` gửi `Shutdown` → `Supervisor::run` giết mọi
cây tiến trình rồi trả về → `join`. Bỏ bước join sẽ để lại tiến trình mồ côi.

## Vì sao không có console

`CREATE_NO_WINDOW` (`0x0800_0000`) khi spawn, thay cho cả tầng `.vbs` mà các hệ
keepalive thường dùng chỉ để giấu cửa sổ đen. Bản thân manager dùng
`#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]`: bản release
không có console, bản debug vẫn giữ để đọc `println!` và thông báo panic.

## Một bản duy nhất

Named mutex `Local\StartupAppManager.SingleInstance`. Hai bản chạy song song sẽ
cùng thấy app chết, cùng sinh lại, và số tiến trình nhân đôi. Bản thứ hai hiện
hộp thoại báo rồi thoát.

Dùng namespace `Local\` chứ không phải `Global\`: nhiều user trên cùng một máy
vẫn chạy được bản riêng.

## Bố cục mã

```
src/
  main.rs              nối chuỗi khởi động, thoát có trật tự
  lib.rs               tách lib để binary và integration test dùng chung API
  paths.rs             vị trí file trên đĩa
  logging.rs           log có xoay vòng, tự tính timestamp UTC
  single_instance.rs   named mutex
  autostart.rs         HKCU Run key
  config/
    model.rs           kiểu dữ liệu, mọi field đều có default
    store.rs           đọc/ghi TOML, ghi nguyên tử qua file tạm + rename
    env.rs             gộp env từ ba nguồn
  supervisor/
    mod.rs             decide() thuần + vòng lặp có side effect
    job.rs             Win32 Job Object — file duy nhất có `unsafe`
    launch.rs          dựng Command, tách tham số kiểu Windows
    backoff.rs         quy luật chờ giữa các lần thử
    health.rs          probe HTTP tối giản
  ui/
    mod.rs             cửa sổ quản lý, khay, điều phối sự kiện
    editor.rs          form thêm/sửa
    format.rs          chuyển đổi giá trị ↔ chuỗi hiển thị
```

## Hạn chế đã biết

- `nwg::MenuItem` không đổi được nhãn sau khi tạo, nên menu khay là cố định.
  Trạng thái từng app hiện qua tooltip khay và cửa sổ quản lý, không nằm trong
  menu.
- Form thêm/sửa vô hiệu hoá cửa sổ cha thay vì làm modal thật: nwg chỉ có một
  vòng dispatch cho cả thread, gọi lồng nhau sẽ giết luôn vòng ngoài.
- Chu kỳ kiểm tra chọn từ danh sách cố định. Giá trị lạ trong config sửa tay sẽ
  được làm tròn về mức gần nhất khi mở form.
- Health check chỉ hỗ trợ `http://`, không có TLS.

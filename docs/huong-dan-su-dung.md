# Hướng dẫn sử dụng

Hướng dẫn chi tiết cho người dùng cuối: cách chạy app, cách **thêm một service**
vào danh sách giám sát, và cách xử lý khi service không lên.

Kiến trúc bên trong và lý do của từng quyết định: [`architecture.md`](architecture.md).

---

## 1. App này làm gì

Startup App Manager là một **keepalive supervisor** cho Windows. Nó:

- khởi chạy các tiến trình bạn khai báo, ngay khi Windows đăng nhập;
- cứ mỗi chu kỳ lại kiểm tra tiến trình còn sống không, nếu chết thì bật lại;
- (tuỳ chọn) gọi một URL HTTP để biết service còn **phục vụ được** hay chỉ còn
  "sống mà treo";
- gom `stdout`/`stderr` của từng service vào log riêng để bạn chẩn đoán.

Nó **không** phải Windows Service. Mọi tiến trình chạy dưới phiên đăng nhập của
bạn, với quyền của bạn. Đăng xuất là mọi thứ dừng.

Điểm khác biệt quan trọng so với Task Scheduler: app đếm số tiến trình còn sống
trong một **Job Object** riêng cho mỗi service, chứ không theo dõi tiến trình con
trực tiếp. Nhờ vậy một launcher sinh ra app thật rồi tự thoát vẫn được tính là
"đang sống", thay vì bị hiểu nhầm là chết và bị sinh lại vô hạn.

---

## 2. Cài đặt và chạy

### File chạy chính thức

Không có trình cài đặt. "Cài" nghĩa là chép file `.exe` vào một chỗ cố định:

```
%LOCALAPPDATA%\Programs\StartupAppManager\startup-app-manager.exe
```

Chỗ này phải **bền**: đường dẫn được ghi cứng vào registry khi bạn bật tự khởi
động (mục 9). Đừng chạy thẳng từ `target\release\` của thư mục build — đó là sản
phẩm biên dịch, chỉ dùng làm nguồn để chép sang.

Chỉ chạy được **một bản** một lúc. Bản thứ hai sẽ báo "The app is already
running." rồi thoát — hai supervisor cùng giám sát một service sẽ nhân đôi số
tiến trình mỗi lần sinh lại.

Tham số `--tray` khiến app khởi động thẳng xuống khay, không mở cửa sổ. Đây là
tham số mà mục tự khởi động dùng; bạn không cần gõ tay.

### App lưu gì, ở đâu

File `.exe` **không chứa cấu hình**. Nó hoàn toàn không có trạng thái — thay file
`.exe` bằng bản mới thì danh sách service vẫn nguyên vẹn. Trạng thái nằm ở ba nơi
tách biệt:

| Cái gì | Ở đâu |
|--------|-------|
| Danh sách service và mọi tham số | `%APPDATA%\StartupAppManager\config.toml` |
| Bật/tắt tự chạy cùng Windows | `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`, value `StartupAppManager` |
| Log | `%APPDATA%\StartupAppManager\logs\` |

Lần chạy đầu app tự tạo:

```
%APPDATA%\StartupAppManager\
  config.toml           cấu hình, sửa tay được
  logs\manager.log      log của chính manager
  logs\app-<id>.log     stdout + stderr của từng service
```

Trạng thái tự khởi động cố ý **không** nằm trong `config.toml` — xem mục 9.

`config.toml` được **đọc một lần lúc khởi động**, và được **ghi lại mỗi lần** bạn
bấm Save / Delete / Pause trong giao diện.

Muốn sao lưu toàn bộ thiết lập thì chỉ cần chép `config.toml`. Lưu ý nó chứa
đường dẫn tuyệt đối (`C:\Users\<tên>\...`), mang sang máy hoặc tài khoản khác thì
phải sửa lại.

### Cập nhật lên bản mới

```powershell
# 1. Thoát app bằng Exit trong menu khay -- BẮT BUỘC.
#    Còn chạy thì bước 2 báo "Access is denied (os error 5)".
cargo build --release

# 2. Chép đè lên bản đang cài
Copy-Item ".\target\release\startup-app-manager.exe" `
          "$env:LOCALAPPDATA\Programs\StartupAppManager\startup-app-manager.exe" -Force

# 3. Mở lại
Start-Process "$env:LOCALAPPDATA\Programs\StartupAppManager\startup-app-manager.exe" -ArgumentList "--tray"
```

Bước 1 cũng **dừng luôn mọi service đang được giám sát** — thoát manager là dọn
sạch cây tiến trình con. Chúng tự lên lại ở bước 3, gián đoạn khoảng 15 giây.

Đừng đổi chỗ file `.exe` chính thức: mục 9 giải thích vì sao.

---

## 3. Cửa sổ chính

### Bảng

| Cột | Nghĩa |
|-----|-------|
| **Name** | Tên bạn đặt, chỉ để nhận diện |
| **Status** | `running` / `stopped` / `paused` / `backoff` / `crash-looping (N attempts)` |
| **Processes** | Số tiến trình đang sống trong Job Object. `?` = chưa hỏi được |
| **Interval** | Chu kỳ kiểm tra thực tế đang áp dụng |
| **Restarts** | Số lần đã phải sinh lại kể từ khi manager lên (lần chạy đầu không tính) |
| **Executable** | File chạy |

Ý nghĩa từng trạng thái:

- **running** — đang chạy, kiểm tra định kỳ. Kèm `Processes = 0` nghĩa là vừa
  spawn hỏng hoặc vừa chết, nhịp kiểm tra kế tiếp sẽ phát hiện.
- **stopped** — không chạy và **không tự khởi động lại**. Đây là kết quả của nút
  Stop, hoặc của `launch_on_start = false` khi manager mới lên.
- **paused** — bị tạm dừng (`enabled = false`). Supervisor bỏ qua hoàn toàn.
- **backoff** — vừa chết, đang đợi hết giãn cách trước khi thử lại.
- **crash-looping** — đã thử đủ số lần cho phép mà vẫn hỏng, đã bỏ cuộc. Đây là
  trạng thái duy nhất app chủ động báo bóng ở khay hệ thống.

### Thanh nút

| Nút | Việc |
|-----|------|
| **Add** | Mở form thêm service mới |
| **Edit** | Sửa dòng đang chọn (bấm đúp vào dòng cũng được) |
| **Delete** | Xoá khỏi danh sách; tiến trình đang chạy bị dừng luôn |
| **Pause** / **Resume** | Tạm dừng giám sát dòng đang chọn. Nhãn tự đổi theo trạng thái dòng |
| **Start** | Khởi chạy ngay. Bị từ chối nếu app đang tạm dừng hoặc **đang có tiến trình sống** |
| **Stop** | Giết cả cây tiến trình và chuyển sang `stopped` — sẽ **không** tự sinh lại |
| **Restart** | Luôn sinh bản mới, kể cả khi đang chạy |

Khác nhau giữa **Start** và **Restart**: Start cố ý không đụng vào app đang khoẻ
(giết một service đang phục vụ là mất dữ liệu đang xử lý), còn Restart thì luôn
dựng bản mới.

### Khay hệ thống

Bấm `X` là **thu nhỏ xuống khay**, không phải thoát — giám sát vẫn chạy. Chuột
phải vào biểu tượng khay:

- **Open manager window** — mở lại cửa sổ
- **Pause all** / **Resume all** — tạm dừng, rồi phục hồi đúng những app trước
  đó đang được mong đợi là sống
- **Exit** — thoát hẳn. Mọi cây tiến trình do app sinh ra đều bị dọn sạch

Tooltip khay hiện tổng quan: `3/4 running, 1 paused`.

---

## 4. Thêm service bằng form

Bấm **Add**. Form có các trường sau.

### Name

Tên hiển thị, tuỳ ý. Bắt buộc.

Nếu bạn dùng nút **Browse...** để chọn file chạy khi ô này còn trống, app tự điền
tên file (không đuôi) vào đây.

### Executable

File sẽ được chạy. Bắt buộc. Có hai cách khai báo:

1. **Đường dẫn đầy đủ** — `C:\Program Files\nodejs\node.exe`. Có chứa `\` hoặc
   `/` nên form kiểm tra file phải tồn tại; không tồn tại thì báo lỗi ngay khi
   Save.
2. **Tên lệnh trần** — `node`, `python`. Không có dấu tách thư mục nên được coi
   là lệnh tìm theo `PATH`, form **không** bắt phải tồn tại trên đĩa.

Cách 1 an toàn hơn: `PATH` mà Windows đưa cho phiên đăng nhập lúc khởi động có
thể khác `PATH` trong terminal của bạn.

Một số đuôi không tự chạy được nên app tự chèn interpreter phía trước:

| Đuôi file | Thực tế được chạy |
|-----------|-------------------|
| `.js` `.mjs` `.cjs` | `node <file>` |
| `.ps1` | `powershell -NoProfile -ExecutionPolicy Bypass -File <file>` |
| `.vbs` `.vbe` `.wsf` | `wscript //B //Nologo <file>` |
| `.exe`, `.cmd`, `.bat`, tên trần | giao thẳng cho Windows |

#### Cảnh báo script — đọc kỹ phần này

Khi bạn gõ hoặc chọn một file `.cmd`, `.bat`, `.ps1`, `.vbs`, `.vbe`, `.wsf`,
form hiện cảnh báo màu chữ thường:

> Note: if this script has its own restart loop inside, it never exits and
> supervision stops working. Point straight at the real process (node/bun/python).

Lý do: nhiều wrapper được viết dạng

```bat
:loop
node cli.js
timeout /t 5
goto loop
```

Script kiểu đó **không bao giờ thoát**, nên Job Object luôn đếm được ít nhất một
tiến trình và luôn báo "còn sống" — kể cả khi service bên trong đã hỏng từ lâu.
Giám sát trở thành vô dụng.

**Cách làm đúng:** mở wrapper ra, đọc xem nó thực sự gọi gì, rồi trỏ thẳng vào
tiến trình thật.

| Thay vì | Hãy dùng |
|---------|----------|
| `Exec: start-service.cmd` | `Exec: C:\Program Files\nodejs\node.exe`<br>`Args: "C:\...\cli.js" --port 8080` |
| `Exec: launcher.vbs` | tiến trình mà `.vbs` đó gọi |

Cảnh báo chỉ là cảnh báo — nếu bạn chắc chắn script của mình chạy một lần rồi
thoát (hoặc chạy foreground không loop), cứ Save bình thường.

### Arguments

Tham số truyền cho chương trình, dạng một dòng thô. Cách tách:

- ngăn cách bằng khoảng trắng;
- bọc `"..."` để giữ nguyên phần có khoảng trắng — **luôn bọc đường dẫn**;
- `\"` cho một dấu nháy kép thật.

```
"C:\Users\Me\AppData\Roaming\npm\node_modules\9router\cli.js" -n -t --skip-update
```

Ô này là ô một dòng. Nếu trong `config.toml` bạn từng đặt giá trị nhiều dòng, form
sẽ báo cho bạn lúc mở và **giữ nguyên** giá trị cũ thay vì cắt cụt.

### Working folder

Thư mục làm việc của tiến trình. Không bắt buộc, nhưng nên đặt: nhiều app đọc
file cấu hình hoặc `node_modules` theo đường dẫn tương đối.

Phải là thư mục **đang tồn tại**, nếu không Save sẽ báo lỗi. Khi bạn chọn file
chạy bằng **Browse...** mà ô này còn trống, app tự điền thư mục chứa file đó.

### Check interval

Bao lâu kiểm tra một lần. Chọn từ danh sách: 30 sec, 1 min, 2 min, 5 min, 10 min,
15 min, 30 min, 1 hr. Mặc định **5 min**.

Đây là chu kỳ *phát hiện*, không phải chu kỳ *thăm dò liên tục*. Chọn ngắn thì
service chết được phát hiện nhanh hơn, đổi lại tốn thêm một lần đếm tiến trình
(và một request HTTP, nếu bật health check) mỗi lượt. Mức 30 giây hợp cho lúc thử
nghiệm; chạy thật thì vài phút là đủ.

Nếu `config.toml` của bạn có giá trị nằm ngoài danh sách (ví dụ `45`), combo hiện
mức gần nhất nhưng **không** ghi đè — miễn là bạn không đụng vào ô đó.

### Launch when the manager starts

Bật (mặc định): service được chạy ngay khi manager khởi động.

Tắt: service nằm ở `stopped` cho đến khi bạn bấm **Start** bằng tay. Dùng cho
những thứ chỉ thỉnh thoảng mới cần.

### Env (KEY=VALUE)

Biến môi trường khai báo trực tiếp, mỗi dòng một cặp:

```
OCX_SERVICE=1
NODE_ENV=production
PORT=10100
```

Quy tắc parse:

- dòng trống và dòng bắt đầu bằng `#` bị bỏ qua;
- dòng không có dấu `=` bị bỏ qua (không làm hỏng cả ô);
- khoảng trắng hai đầu bị cắt;
- giá trị bọc `"..."` hoặc `'...'` sẽ được bỏ nháy — muốn giữ khoảng trắng đầu/cuối
  thì bọc nháy.

**Không có thứ tự bung biến.** `CreateProcess` không hiểu `%VAR%`. Nếu wrapper cũ
của bạn viết `set "PATH=%PATH%;C:\extra"` thì bạn phải tự bung ra thành giá trị
đầy đủ trước khi dán vào đây.

Giá trị nhiều dòng không đi qua ô này được, nên nếu `config.toml` có sẵn một biến
nhiều dòng, ô sẽ hiện một dòng đánh dấu:

```
# PROMPT: multi-line, preserved, not editable here
```

Còn dòng đó thì biến được giữ nguyên. **Xoá dòng đó đi là xoá hẳn biến.**

### Env file

Đường dẫn tới một file `KEY=VALUE` (kiểu `.env`). Cùng luật parse như ô trên.

File **phải tồn tại** lúc Save — kiểm tra ngay tại form là có chủ đích: thiếu file
env sẽ làm mọi lần spawn đều hỏng, và service sẽ đốt hết số lần thử rồi nằm
`crash-looping` chỉ vì một cái gõ nhầm đường dẫn.

### VAR=value file

Dành cho file chứa **một giá trị trần** — token, API key, secret — chứ không phải
dạng `KEY=VALUE`. Mỗi dòng:

```
OPENCODEX_API_AUTH_TOKEN=C:\Users\Me\.opencodex\service-api-token
GITHUB_TOKEN=C:\secrets\gh.txt
```

Biến `OPENCODEX_API_AUTH_TOKEN` sẽ nhận **toàn bộ nội dung file**, đã cắt khoảng
trắng và newline cuối (newline lạc trong header HTTP là một lỗi rất khó tìm).

File gốc không bao giờ bị sửa. Mọi file khai báo ở đây đều phải tồn tại lúc Save.

#### Ba nguồn env, thứ tự ưu tiên

Cả ba nguồn được trộn lên trên môi trường sẵn có của manager, theo thứ tự **sau
đè trước**:

```
môi trường của manager
  └─ Env file            (thấp nhất)
      └─ VAR=value file
          └─ Env (KEY=VALUE)   (cao nhất)
```

Nghĩa là: cùng một tên biến, giá trị gõ thẳng trong ô **Env** luôn thắng.

### Max retries

Số lần thử lại **liên tiếp** trước khi bỏ cuộc. Mặc định `5`.

`0` = thử mãi, không bao giờ vào `crash-looping`. Cân nhắc kỹ: một app hỏng vĩnh
viễn sẽ bị sinh lại mãi.

Bộ đếm được **xoá về 0** mỗi lần app được quan sát thấy khoẻ mạnh.

### HTTP check + Health URL

Bật ô **HTTP check** khi "tiến trình còn sống" chưa đủ để kết luận service còn
dùng được — điển hình là web server treo nhưng tiến trình chưa chết.

- **Health URL** — chỉ hỗ trợ `http://`. Probe **không làm TLS**, nên `https://`
  sẽ bị form từ chối ngay. Mặc định gợi ý `http://127.0.0.1:8080/health`.
- **Ô số bên phải** — số lần fail **liên tiếp** trước khi restart. Mặc định `2`,
  phải lớn hơn 0. Một lần nghẽn tạm thời không được phép giết một service đang
  phục vụ, nên đừng đặt `1` trừ khi service của bạn thực sự ổn định.

Hai tham số còn lại chỉ sửa được trong `config.toml`: `timeout_secs` (mặc định
`3`) và `expect_status` (mặc định `200`).

Nếu URL không hợp lệ, health check bị **bỏ qua** (ghi cảnh báo một lần vào log) và
app quay về chỉ kiểm tra tiến trình — chứ không bị coi là hỏng.

### Save

Bấm **Save**. Thứ tự thực hiện: ghi xuống `config.toml` **trước**, rồi mới nạp vào
supervisor. Nếu ghi hỏng (đĩa đầy, file bị khoá), form **không đóng** và mọi thứ
bạn vừa gõ còn nguyên.

---

## 5. Ví dụ đầy đủ

Ba service thật, lấy từ `cargo run --example seed_services`.

### 5.1 App Node chạy nền

```
Name:            9Router Background
Executable:      C:\Program Files\nodejs\node.exe
Arguments:       "C:\Users\Me\AppData\Roaming\npm\node_modules\9router\cli.js" -n -t --skip-update
Working folder:  C:\Users\Me\AppData\Roaming\npm\node_modules\9router
Check interval:  5 min
Launch on start: ✓
Max retries:     5
HTTP check:      ✗
```

Nguồn gốc là một task gọi `.vbs` gọi `node cli.js`. Ta bỏ qua cả hai lớp wrapper
và trỏ thẳng vào `node.exe`.

### 5.2 Service có health check và env

```
Name:            OpenCodex Service
Executable:      C:\Users\Me\Tools\opencodex\node_modules\bun\bin\bun.exe
Arguments:       "C:\Users\Me\Tools\opencodex\src\cli\index.ts" start --port 10100
Working folder:  C:\Users\Me\Tools\opencodex
Env:             OCX_SERVICE=1
                 OCX_API_TOKEN_FILE=C:\Users\Me\.opencodex\service-api-token
                 PATH=C:\Users\Me\bin;C:\Program Files\nodejs;...
HTTP check:      ✓
Health URL:      http://127.0.0.1:10100/health
Failures:        2
```

Wrapper gốc là `opencodex-service.cmd` có vòng `:loop` ngủ 5 giây rồi chạy lại —
đúng kiểu script không bao giờ thoát nói ở mục 4. Trỏ thẳng vào `bun.exe`.

`PATH` phải ghi đầy đủ: wrapper dùng `set "PATH=%PATH%;..."` và `cmd` tự bung
`%PATH%`, còn `CreateProcess` thì không.

### 5.3 Service Python trong venv

```
Name:            Hermes Gateway
Executable:      C:\Users\Me\AppData\Local\hermes\hermes-agent\venv\Scripts\python.exe
Arguments:       -m hermes_cli.main gateway run
Working folder:  C:\Users\Me\AppData\Local\hermes
Env:             HERMES_HOME=C:\Users\Me\AppData\Local\hermes
                 PYTHONIOENCODING=utf-8
                 HERMES_GATEWAY_DETACHED=1
                 VIRTUAL_ENV=C:\Users\Me\AppData\Local\hermes\hermes-agent\venv
                 PYTHONPATH=C:\Users\Me\AppData\Local\hermes\hermes-agent
```

Gọi thẳng `python.exe` trong venv thay vì `activate` rồi chạy — không cần shell
trung gian nào cả.

---

## 6. Thêm service bằng `config.toml`

Sửa tay được, và đôi khi nhanh hơn form. Đóng app trước khi sửa, vì app ghi đè
file mỗi lần bạn Save trong UI.

### Dạng file

```toml
next_app_id = 4

[settings]
default_check_interval_secs = 300

[[apps]]
id = 1
name = "9Router Background"
exe = 'C:\Program Files\nodejs\node.exe'
args = '"C:\Users\Me\AppData\Roaming\npm\node_modules\9router\cli.js" -n -t'
working_dir = 'C:\Users\Me\AppData\Roaming\npm\node_modules\9router'
enabled = true
launch_on_start = true
check_interval_secs = 300

[apps.restart]
max_retries = 5
backoff_base_secs = 5
backoff_max_secs = 300

[apps.env]
NODE_ENV = "production"

[apps.env_from_files]
API_TOKEN = 'C:\Users\Me\.secrets\token'

[apps.health]
url = "http://127.0.0.1:10100/health"
timeout_secs = 3
expect_status = 200
failures_before_restart = 2

[[apps]]
id = 2
# ... service tiếp theo
```

Lưu ý cú pháp TOML: bốn khối con `[apps.restart]`, `[apps.env]`,
`[apps.env_from_files]`, `[apps.health]` phải nằm **sau** khối `[[apps]]` của nó
và **trước** `[[apps]]` kế tiếp. Dùng nháy đơn `'...'` cho đường dẫn Windows để
khỏi phải escape dấu `\`.

### Bảng trường

| Trường | Kiểu | Mặc định | Ghi chú |
|--------|------|----------|---------|
| `id` | số | — | Duy nhất. Quyết định tên `app-<id>.log` |
| `name` | chuỗi | `""` | Tên hiển thị |
| `exe` | đường dẫn | `""` | File chạy hoặc lệnh theo `PATH` |
| `args` | chuỗi | `""` | Tham số thô, bọc nháy phần có khoảng trắng |
| `working_dir` | đường dẫn | không có | Bỏ trường đi = kế thừa của manager |
| `enabled` | bool | `true` | `false` = tạm dừng |
| `launch_on_start` | bool | `true` | Chạy ngay khi manager lên |
| `check_interval_secs` | số | `300` | Bị kẹp trong khoảng **10 … 86400** |
| `restart.max_retries` | số | `5` | `0` = vô hạn |
| `restart.backoff_base_secs` | số | `5` | Giãn cách lần thử đầu |
| `restart.backoff_max_secs` | số | `300` | Trần giãn cách |
| `env` | bảng | rỗng | Ưu tiên cao nhất |
| `env_file` | đường dẫn | không có | File `KEY=VALUE`, ưu tiên thấp nhất |
| `env_from_files` | bảng | rỗng | `VAR = đường-dẫn`, nhận cả nội dung file |
| `health.url` | chuỗi | `""` | Chỉ `http://` |
| `health.timeout_secs` | số | `3` | Chỉ sửa được ở đây |
| `health.expect_status` | số | `200` | Chỉ sửa được ở đây |
| `health.failures_before_restart` | số | `2` | Phải > 0 |

Mọi trường đều có mặc định, nên **thiếu trường không làm hỏng file** — file cũ vẫn
load được sau khi nâng cấp app.

### Vài cái bẫy khi sửa tay

- **`check_interval_secs` bị kẹp hai đầu.** Ghi `2` thì thực tế vẫn là 10 giây, và
  cột **Interval** hiện `10 sec` chứ không nói dối theo con số bạn gõ.
- **Nhân đôi một khối `[[apps]]` mà quên đổi `id`.** App phát hiện lúc nạp, cấp id
  mới cho khối trùng và ghi cảnh báo vào `manager.log`. Không cấp lại id đã xoá,
  để `app-<id>.log` của app cũ không lẫn với app mới.
- **File hỏng hoặc rỗng** không chặn app khởi động: manager ghi cảnh báo rồi dùng
  config rỗng. Nghĩa là bạn sẽ thấy bảng trống — hãy xem `manager.log` trước khi
  gõ lại từ đầu.

### Nạp lại config đã sửa

App chỉ đọc `config.toml` lúc khởi động. Sửa tay xong thì **Exit** rồi mở lại.

---

## 7. Điều gì khiến service bị khởi động lại

Khi bạn Save trong form, service **đang sống** sẽ bị dựng lại nếu bất kỳ thứ nào
sau đây thay đổi:

`exe` · `args` · `working_dir` · `env` · `env_file` · `env_from_files`

Đây là những tham số chỉ có hiệu lực lúc spawn, nên không sinh lại thì sửa cũng
vô nghĩa. Đổi `name`, `check_interval_secs` hay `max_retries` thì **không** đụng
tới tiến trình đang chạy.

Riêng service đang ở `crash-looping`, sửa `health` hoặc `restart` cũng đủ để nó
được thử lại — vì đó chính là cách bạn gỡ một app đã bỏ cuộc vì URL health sai.

Rút ngắn `check_interval_secs` có hiệu lực ngay: lịch hẹn đang chờ bị kéo về
không quá chu kỳ mới, chứ không phải đợi hết chu kỳ cũ.

---

## 8. Vòng lặp giám sát và backoff

Mỗi chu kỳ, với từng service, supervisor đếm số tiến trình còn sống trong Job
Object của nó, rồi (nếu bật) gọi health URL:

- **còn tiến trình + health OK** → `running`, xoá bộ đếm thất bại
- **hết tiến trình** hoặc **health fail đủ số lần liên tiếp** → tính là một lần
  thất bại, chuyển sang `backoff`
- **hỏi không được** (lỗi Win32 tạm thời) → **không kết luận gì**, giữ nguyên
  trạng thái. Coi lỗi truy vấn là "đã chết" sẽ giết nhầm service đang khoẻ.

Giãn cách trước lần thử thứ `n`:

```
backoff_base_secs × 2^(n-1),  chặn trên bởi backoff_max_secs
```

Với mặc định `base = 5`, `max = 300`:

| Lần thử | 1 | 2 | 3 | 4 | 5 | 6 |
|---------|---|---|---|---|---|---|
| Đợi | 5s | 10s | 20s | 40s | 80s | 160s |

Quá `max_retries` lần liên tiếp → `crash-looping`, dừng thử lại, và **báo bóng ở
khay một lần** kèm nguyên nhân. Muốn thử lại: sửa config cho đúng, hoặc bấm
**Restart** bằng tay.

---

## 9. Tự chạy cùng Windows

Đánh dấu ô **Start with Windows** ở góc dưới trái. App ghi vào:

```
HKCU\Software\Microsoft\Windows\CurrentVersion\Run
  StartupAppManager = "<đường dẫn đầy đủ tới .exe>" --tray
```

Nguồn sự thật duy nhất là registry, không lưu bản sao trong config.

**Đừng di chuyển file `.exe` sau khi bật.** Đường dẫn đã được ghi cứng; nếu lỡ di
chuyển, ô đánh dấu sẽ tự hiện về trạng thái tắt để bạn bật lại cho đúng chỗ mới.

---

## 10. Log và xử lý sự cố

```
%APPDATA%\StartupAppManager\logs\manager.log   quyết định của supervisor
%APPDATA%\StartupAppManager\logs\app-<id>.log  stdout + stderr của service
```

`<id>` là cột ẩn — lấy từ `config.toml`, hoặc đếm theo thứ tự khối `[[apps]]`.

### Log giữ bao lâu, có phình không

Cắt theo **dung lượng**, không theo thời gian. Không có dọn theo ngày, không nén,
không xoay lúc nửa đêm.

| File | Ngưỡng cắt | Kiểm tra khi nào |
|------|-----------|------------------|
| `manager.log` | 1 MB | mỗi lần ghi một dòng |
| `app-<id>.log` | 4 MB | chỉ lúc service được khởi chạy |

Vượt ngưỡng thì file hiện tại được đổi tên thành `.log.1` và bắt đầu file mới.
**Chỉ giữ đúng một bản lưu** — `.log.1` cũ bị xoá trước khi ghi đè.

Trần lý thuyết vì thế là ~2 MB cho `manager.log` và ~8 MB cho mỗi service. Bốn
service thì kịch trần khoảng 34 MB.

Log của service có ngưỡng cao hơn vì nó chứa toàn bộ `stdout` + `stderr` của tiến
trình con, nói nhiều hơn hẳn log quyết định của manager.

**Một lỗ hổng có thật:** `app-<id>.log` chỉ xoay được **lúc spawn**. Tiến trình
con giữ handle của chính file đó suốt vòng đời, nên đổi tên giữa chừng thì nó vẫn
ghi tiếp vào bản `.log.1` còn file thật nằm im — vừa không chặn được gì, vừa mất
dấu vết. Hệ quả: một service nói nhiều mà chạy hàng tuần không restart thì log
phình vượt 4 MB, và chỉ bị cắt ở lần khởi động lại kế tiếp.

Nếu gặp trường hợp đó, bấm **Restart** cho service ấy là file được cắt ngay.

### Service không lên

1. Mở `app-<id>.log` — service thường tự nói nó chết vì sao.
2. Không có gì trong đó → mở `manager.log`, tìm dòng có tên service. Các thông
   báo hay gặp:

| Log | Nghĩa |
|-----|-------|
| `cannot spawn: ...` | Sai đường dẫn `exe`, hoặc thiếu quyền |
| `cannot read env file ...` | `env_file` / `env_from_files` trỏ vào file không tồn tại |
| `cannot assign to job: ...` | Hiếm; tiến trình đã bị giết ngay để khỏi mồ côi |
| `old process tree is not fully dead, not spawning a new one` | Lần dừng trước chưa dọn xong — thường do service chạy ở quyền cao hơn manager |
| `health check failed N times in a row: ...` | Tiến trình sống nhưng URL không trả lời đúng |
| `health check skipped, invalid URL: ...` | URL sai dạng; app quay về chỉ kiểm tra tiến trình |

### Status luôn là `running` nhưng service thật đã chết

Gần như chắc chắn `exe` đang trỏ vào một wrapper có vòng lặp bên trong. Xem lại
mục 4 và trỏ thẳng vào tiến trình thật.

### `Processes` hiện `?`

Không đếm được tiến trình trong job ở nhịp đó. Một hai nhịp là bình thường; kéo
dài thì xem `manager.log`.

### App vào `crash-looping` ngay lập tức

Kiểm tra theo thứ tự: `exe` có tồn tại không → `working_dir` có tồn tại không →
mọi file trong `env_file` / `env_from_files` có tồn tại không. Thiếu file env làm
**mọi** lần spawn hỏng, nên số lần thử cạn rất nhanh.

---

## 11. Gỡ cài đặt

1. Bỏ đánh dấu **Start with Windows** (hoặc xoá value `StartupAppManager` trong
   `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`)
2. **Exit** từ menu khay
3. Xoá `%APPDATA%\StartupAppManager\` (config + log) và
   `%LOCALAPPDATA%\Programs\StartupAppManager\` (file chạy)

---

## 12. Giới hạn đã biết

- **Không phải Windows Service.** Mọi thứ chạy dưới phiên đăng nhập của bạn; đăng
  xuất là dừng hết.
- **Health check chỉ `http://`.** Không TLS, không header tuỳ biến, không body.
- **Menu khay cố định**, không liệt kê từng service — giới hạn của thư viện GUI
  đang dùng. Thao tác trên từng service nằm ở cửa sổ chính.
- **Cửa sổ không co giãn được**, bố trí theo toạ độ cố định.
- **`config.toml` chỉ đọc lúc khởi động.** Sửa tay xong phải mở lại app.
- **Log của service chỉ được cắt lúc spawn**, nên một service chạy hàng tuần
  không restart có thể vượt trần 4 MB. Chi tiết ở mục 10.
- **Không có trình cài đặt.** Cập nhật là chép đè file `.exe` bằng tay (mục 2).
- **Không bung `%VAR%`** trong giá trị env — phải ghi giá trị đầy đủ.
- **Backslash đôi trước dấu nháy trong ô Arguments bị hiểu sai.** Một tham số viết
  `--data "C:\data\\"` sẽ thành `C:\data"` thay vì `C:\data\`. Tránh kết thúc
  một đường dẫn bằng `\` ngay trước dấu nháy đóng: bỏ `\` cuối đi, hoặc đặt
  đường dẫn ở ô **Working folder** thay vì truyền qua tham số.

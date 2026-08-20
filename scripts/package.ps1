[CmdletBinding()]
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

$projectRoot = Split-Path -Parent $PSScriptRoot
$cargoToml = Join-Path $projectRoot 'Cargo.toml'
$releaseExe = Join-Path $projectRoot 'target\release\startup-app-manager.exe'
$distRoot = Join-Path $projectRoot 'dist'

if (-not (Test-Path -LiteralPath $cargoToml -PathType Leaf)) {
    throw "Khong tim thay Cargo.toml tai $projectRoot"
}

$cargoText = Get-Content -Raw -LiteralPath $cargoToml
$versionMatch = [regex]::Match($cargoText, '(?m)^\s*version\s*=\s*"([^"]+)"\s*$')
if (-not $versionMatch.Success) {
    throw 'Khong doc duoc version trong Cargo.toml'
}

$version = $versionMatch.Groups[1].Value
$packageName = "startup-app-manager-v$version-windows-x64"
$packageDir = Join-Path $distRoot $packageName
$zipPath = Join-Path $distRoot "$packageName.zip"

if (-not $SkipBuild) {
    $releaseFullPath = [System.IO.Path]::GetFullPath($releaseExe)
    $runningRelease = Get-Process -Name 'startup-app-manager' -ErrorAction SilentlyContinue |
        Where-Object {
            try {
                [System.IO.Path]::GetFullPath($_.Path) -eq $releaseFullPath
            }
            catch {
                $false
            }
        }
    if ($null -ne $runningRelease) {
        throw 'Hay thoat binary target\release\startup-app-manager.exe truoc khi build de tranh loi Access is denied.'
    }

    Push-Location $projectRoot
    try {
        & cargo build --release --locked
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build that bai voi ma $LASTEXITCODE"
        }
    }
    finally {
        Pop-Location
    }
}

if (-not (Test-Path -LiteralPath $releaseExe -PathType Leaf)) {
    throw "Khong tim thay binary release tai $releaseExe"
}

New-Item -ItemType Directory -Force -Path $distRoot | Out-Null
if (Test-Path -LiteralPath $packageDir) {
    # Chi xoa thu muc artifact do script tao, khong cham vao source hay target.
    Remove-Item -LiteralPath $packageDir -Recurse -Force
}
New-Item -ItemType Directory -Force -Path $packageDir | Out-Null

Copy-Item -LiteralPath $releaseExe -Destination (Join-Path $packageDir 'startup-app-manager.exe') -Force
Copy-Item -LiteralPath (Join-Path $projectRoot 'docs\huong-dan-su-dung.md') `
    -Destination (Join-Path $packageDir 'huong-dan-su-dung.md') -Force

$readme = @"
Startup App Manager - Windows x64 portable

1. Giai nen file ZIP nay vao mot thu muc co dinh, vi du:
   %LOCALAPPDATA%\Programs\StartupAppManager
2. Chay startup-app-manager.exe.
3. Trong cua so app, bam Add de them app/service can giam sat.
4. Neu muon tu chay cung Windows, bat tuy chon Start with Windows.

Package nay khong can cai .NET, Python, Node.js hay runtime phu. No chi chua
manager; cac app/service duoc manager theo doi va cac runtime cua chung khong
nam trong package, ban can cai rieng tren may dich.

Du lieu cua nguoi dung duoc tao sau lan chay dau tai:
  %APPDATA%\StartupAppManager\config.toml
  %APPDATA%\StartupAppManager\logs\

Khong chay truc tiep tu ben trong file ZIP. Khong di chuyen file EXE sau khi
bat Start with Windows, vi duong dan day du duoc luu vao registry.

Go cai dat: tat Start with Windows, chon Exit trong menu system tray, sau do
xoa thu muc chua startup-app-manager.exe va thu muc %APPDATA%\StartupAppManager
neu muon xoa ca cau hinh/log.

Xem huong-dan-su-dung.md de biet cach cau hinh exe, args, working folder, env
va health check.
"@
$readme | Set-Content -LiteralPath (Join-Path $packageDir 'readme.txt') -Encoding UTF8

$configExample = @'
# File mau tuy chon. App tu tao config.toml trong %APPDATA%\StartupAppManager
# khi chay lan dau; khong can chep file nay de chay manager.

[settings]
default_check_interval_secs = 300

# Vi du: bo comment va thay cac duong dan phu hop voi may dich.
# [[apps]]
# id = 1
# name = "My service"
# exe = 'C:\Program Files\MyApp\my-app.exe'
# args = ""
# working_dir = 'C:\Program Files\MyApp'
# enabled = true
# launch_on_start = true
# check_interval_secs = 300
#
# [apps.restart]
# max_retries = 5
# backoff_base_secs = 5
# backoff_max_secs = 300
'@
$configExample | Set-Content -LiteralPath (Join-Path $packageDir 'config.example.toml') -Encoding UTF8

$hash = (Get-FileHash -LiteralPath (Join-Path $packageDir 'startup-app-manager.exe') -Algorithm SHA256).Hash
"$hash  startup-app-manager.exe" | Set-Content -LiteralPath (Join-Path $packageDir 'sha256.txt') -Encoding ASCII

if (Test-Path -LiteralPath $zipPath) {
    Remove-Item -LiteralPath $zipPath -Force
}
Compress-Archive -Path $packageDir -DestinationPath $zipPath -CompressionLevel Optimal -Force

Write-Host "Da tao package: $packageDir"
Write-Host "Da tao ZIP:     $zipPath"
Write-Host "SHA256 EXE:     $hash"

# ============================================================
# Password Safer - 构建脚本
# 使用方式:
#   .\build.ps1                  - 桌面开发模式（热重载）
#   .\build.ps1 -Build           - 桌面生产构建（生成安装包）
#   .\build.ps1 -Check           - 仅检查环境依赖
#   .\build.ps1 -Android         - 安卓开发模式（需 Android SDK）
#   .\build.ps1 -Android -Build  - 安卓生产构建（生成 APK / AAB）
#   .\build.ps1 -Android -Check  - 仅检查 Android 环境
# ============================================================

param(
    [switch]$Build,
    [switch]$Check,
    [switch]$Android
)

$ErrorActionPreference = "Stop"
$ProjectRoot = Split-Path -Parent $MyInvocation.MyCommand.Path

Write-Host ""
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "  Password Safer - Build Script" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""

# ========== 1. 检查 Rust 工具链 ==========
Write-Host "[1/5] 检查 Rust 工具链..." -ForegroundColor Yellow

$rustupCmd = Get-Command rustup -ErrorAction SilentlyContinue
if (-not $rustupCmd) {
    $cargoPath = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (Test-Path $cargoPath) {
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
        $rustupCmd = Get-Command rustup -ErrorAction SilentlyContinue
    }
}

if (-not $rustupCmd) {
    Write-Host "  [ERROR] Rust 未安装" -ForegroundColor Red
    Write-Host ""
    Write-Host "  请安装 Rust 工具链:" -ForegroundColor White
    Write-Host "    1. 访问 https://rustup.rs" -ForegroundColor White
    Write-Host "    2. 下载并运行 rustup-init.exe" -ForegroundColor White
    Write-Host "    3. 选择默认安装 (stable MSVC 或 GNU 工具链)" -ForegroundColor White
    Write-Host ""
    Write-Host "  注意: 如果使用 MSVC 工具链, 需要先安装 Visual Studio C++ Build Tools" -ForegroundColor White
    Write-Host "        下载: https://visualstudio.microsoft.com/visual-cpp-build-tools/" -ForegroundColor White
    Write-Host "        安装时勾选 '使用 C++ 的桌面开发'" -ForegroundColor White
    Write-Host ""
    exit 1
}

# 检查工具链
$toolchain = rustup show active-toolchain 2>&1
Write-Host "  Active toolchain: $toolchain" -ForegroundColor Green

# 检查是 MSVC 还是 GNU
$isGnu = $toolchain -match "gnu"
$isMsvc = $toolchain -match "msvc"

if ($isMsvc) {
    # 检查 MSVC 构建工具
    $vsWhere = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\Installer\vswhere.exe"
    $hasVcTools = $false
    if (Test-Path $vsWhere) {
        $hasVcTools = & $vsWhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property displayName 2>$null
    }

    if (-not $hasVcTools) {
        Write-Host "  [WARNING] 未检测到 MSVC C++ 构建工具!" -ForegroundColor Red
        Write-Host ""
        Write-Host "  Rust 使用的是 MSVC 工具链, 但系统中缺少 C++ 构建工具。" -ForegroundColor White
        Write-Host "  编译将会失败。请选择以下方案之一:" -ForegroundColor White
        Write-Host ""
        Write-Host "  方案 A: 安装 Visual Studio C++ Build Tools" -ForegroundColor Cyan
        Write-Host "    1. 下载: https://visualstudio.microsoft.com/visual-cpp-build-tools/" -ForegroundColor White
        Write-Host "    2. 安装时勾选 '使用 C++ 的桌面开发'" -ForegroundColor White
        Write-Host "    3. 重新运行此脚本" -ForegroundColor White
        Write-Host ""
        Write-Host "  方案 B: 切换到 GNU 工具链 (无需 MSVC)" -ForegroundColor Cyan
        Write-Host "    rustup toolchain install stable-x86_64-pc-windows-gnu" -ForegroundColor White
        Write-Host "    rustup default stable-x86_64-pc-windows-gnu" -ForegroundColor White
        Write-Host "    重新运行此脚本" -ForegroundColor White
        Write-Host ""

        if (-not $Check) {
            $choice = Read-Host "是否继续尝试构建? (y/N)"
            if ($choice -ne "y" -and $choice -ne "Y") { exit 1 }
        }
    } else {
        Write-Host "  MSVC C++ 构建工具: OK" -ForegroundColor Green
    }
}

if ($isGnu) {
    Write-Host "  GNU 工具链: OK (无需 MSVC)" -ForegroundColor Green
}

# ========== 2. 检查 Node.js ==========
Write-Host ""
Write-Host "[2/5] 检查 Node.js..." -ForegroundColor Yellow

$node = Get-Command node -ErrorAction SilentlyContinue
$npm = Get-Command npm -ErrorAction SilentlyContinue

if (-not $node -or -not $npm) {
    Write-Host "  [ERROR] Node.js 未安装" -ForegroundColor Red
    Write-Host ""
    Write-Host "  请安装 Node.js (LTS 版本):" -ForegroundColor White
    Write-Host "    https://nodejs.org/" -ForegroundColor White
    Write-Host ""
    exit 1
}

$nodeVersion = node --version
$npmVersion = npm --version
Write-Host "  Node.js: $nodeVersion" -ForegroundColor Green
Write-Host "  npm: $npmVersion" -ForegroundColor Green

# ========== 3. 安装前端依赖 ==========
Write-Host ""
Write-Host "[3/5] 检查前端依赖..." -ForegroundColor Yellow

$nodeModules = Join-Path $ProjectRoot "node_modules"
if (-not (Test-Path $nodeModules)) {
    Write-Host "  安装 npm 依赖..." -ForegroundColor White
    Push-Location $ProjectRoot
    npm install
    $npmExit = $LASTEXITCODE
    Pop-Location
    if ($npmExit -ne 0) {
        Write-Host "  [ERROR] npm install 失败" -ForegroundColor Red
        exit 1
    }
    Write-Host "  依赖安装完成" -ForegroundColor Green
} else {
    Write-Host "  node_modules 已存在, 跳过安装" -ForegroundColor Green
}

# ========== 4. 检查 Tauri CLI ==========
Write-Host ""
Write-Host "[4/5] 检查 Tauri CLI..." -ForegroundColor Yellow

$tauriCli = Join-Path $ProjectRoot "node_modules\.bin\tauri.cmd"
if (-not (Test-Path $tauriCli)) {
    Write-Host "  [WARNING] Tauri CLI 未找到, 尝试重新安装..." -ForegroundColor Yellow
    Push-Location $ProjectRoot
    npm install @tauri-apps/cli@^2 --save-dev
    Pop-Location
} else {
    Write-Host "  Tauri CLI: OK" -ForegroundColor Green
}

# ========== 4.5 检查 Android SDK（仅 -Android 模式） ==========
if ($Android) {
    Write-Host ""
    Write-Host "检查 Android SDK..." -ForegroundColor Yellow

    if (-not $env:ANDROID_HOME) {
        Write-Host "  [ERROR] ANDROID_HOME 环境变量未设置" -ForegroundColor Red
        Write-Host ""
        Write-Host "  Android 构建需要 Android SDK + NDK 环境。请:" -ForegroundColor White
        Write-Host "    1. 安装 Android Studio: https://developer.android.com/studio" -ForegroundColor White
        Write-Host "    2. 通过 SDK Manager 安装 Android SDK Platform / Build-Tools" -ForegroundColor White
        Write-Host "    3. 安装 NDK (Side by side) 与 CMake" -ForegroundColor White
        Write-Host "    4. 设置环境变量 ANDROID_HOME 指向 SDK 根目录" -ForegroundColor White
        Write-Host "       (例如: C:\Users\<user>\AppData\Local\Android\Sdk)" -ForegroundColor White
        Write-Host "    5. 首次运行需执行: npx tauri android init" -ForegroundColor White
        Write-Host ""
        exit 1
    }

    if (-not (Test-Path $env:ANDROID_HOME)) {
        Write-Host "  [ERROR] ANDROID_HOME 路径不存在: $env:ANDROID_HOME" -ForegroundColor Red
        exit 1
    }

    Write-Host "  ANDROID_HOME: $env:ANDROID_HOME" -ForegroundColor Green

    if ($env:NDK_HOME -and (Test-Path $env:NDK_HOME)) {
        Write-Host "  NDK_HOME: $env:NDK_HOME" -ForegroundColor Green
    } else {
        Write-Host "  [WARNING] NDK_HOME 未设置, Tauri 将尝试自动检测 NDK" -ForegroundColor Yellow
    }

    $androidGen = Join-Path $ProjectRoot "src-tauri\gen\android"
    if (-not (Test-Path $androidGen)) {
        Write-Host "  [WARNING] src-tauri\gen\android\ 不存在" -ForegroundColor Yellow
        if (-not $Check) {
            $choice = Read-Host "  是否现在执行 tauri android init? (y/N)"
            if ($choice -eq "y" -or $choice -eq "Y") {
                Push-Location $ProjectRoot
                npx tauri android init
                $initExit = $LASTEXITCODE
                Pop-Location
                if ($initExit -ne 0) {
                    Write-Host "  [ERROR] tauri android init 失败" -ForegroundColor Red
                    exit 1
                }
                Write-Host "  Android 工程初始化完成" -ForegroundColor Green
            } else {
                Write-Host "  跳过初始化, 构建可能失败" -ForegroundColor Yellow
            }
        }
    } else {
        Write-Host "  Android 工程已初始化: OK" -ForegroundColor Green
    }
}

# ========== 5. 构建/运行 ==========
Write-Host ""
Write-Host "[5/5] 启动构建..." -ForegroundColor Yellow

if ($Check) {
    Write-Host ""
    Write-Host "==========================================" -ForegroundColor Green
    Write-Host "  环境检查完成!" -ForegroundColor Green
    Write-Host "==========================================" -ForegroundColor Green
    Write-Host ""
    exit 0
}

Push-Location $ProjectRoot

if ($Android) {
    if ($Build) {
        Write-Host "  执行 Android 生产构建 (npm run tauri:android:build)..." -ForegroundColor White
        Write-Host "  这将生成 Android APK / AAB" -ForegroundColor White
        Write-Host ""
        npm run tauri:android:build
        $buildExit = $LASTEXITCODE

        if ($buildExit -eq 0) {
            Write-Host ""
            Write-Host "==========================================" -ForegroundColor Green
            Write-Host "  Android 构建成功!" -ForegroundColor Green
            Write-Host "==========================================" -ForegroundColor Green
            Write-Host ""
            Write-Host "  产物位置:" -ForegroundColor Cyan
            $apkPath = Join-Path $ProjectRoot "src-tauri\gen\android\app\build\outputs"
            if (Test-Path $apkPath) {
                Get-ChildItem $apkPath -Recurse -Include *.apk,*.aab | ForEach-Object {
                    Write-Host "    $($_.FullName)" -ForegroundColor White
                }
            }
            Write-Host ""
        } else {
            Write-Host ""
            Write-Host "==========================================" -ForegroundColor Red
            Write-Host "  Android 构建失败! 退出码: $buildExit" -ForegroundColor Red
            Write-Host "==========================================" -ForegroundColor Red
            Write-Host ""
            Write-Host "  常见问题:" -ForegroundColor Yellow
            Write-Host "    1. ANDROID_HOME / NDK_HOME 未正确设置" -ForegroundColor White
            Write-Host "    2. 未执行 npx tauri android init" -ForegroundColor White
            Write-Host "    3. 未连接设备或未启动模拟器 (dev 模式)" -ForegroundColor White
            Write-Host "    4. Android SDK Build-Tools / Platform 版本不匹配" -ForegroundColor White
            Write-Host ""
        }
    } else {
        Write-Host "  启动 Android 开发模式 (npm run tauri:android:dev)..." -ForegroundColor White
        Write-Host "  需连接 Android 设备或启动模拟器" -ForegroundColor White
        Write-Host ""
        npm run tauri:android:dev
    }
} elseif ($Build) {
    Write-Host "  执行生产构建 (npm run tauri build)..." -ForegroundColor White
    Write-Host "  这将生成 Windows 安装包 (.msi / .exe)" -ForegroundColor White
    Write-Host ""
    npm run tauri build
    $buildExit = $LASTEXITCODE

    if ($buildExit -eq 0) {
        Write-Host ""
        Write-Host "==========================================" -ForegroundColor Green
        Write-Host "  构建成功!" -ForegroundColor Green
        Write-Host "==========================================" -ForegroundColor Green
        Write-Host ""
        Write-Host "  安装包位置:" -ForegroundColor Cyan
        $bundlePath = Join-Path $ProjectRoot "src-tauri\target\release\bundle"
        if (Test-Path $bundlePath) {
            Get-ChildItem $bundlePath -Recurse -Include *.msi,*.exe | ForEach-Object {
                Write-Host "    $($_.FullName)" -ForegroundColor White
            }
        }
        Write-Host ""
    } else {
        Write-Host ""
        Write-Host "==========================================" -ForegroundColor Red
        Write-Host "  构建失败! 退出码: $buildExit" -ForegroundColor Red
        Write-Host "==========================================" -ForegroundColor Red
        Write-Host ""
        Write-Host "  常见问题:" -ForegroundColor Yellow
        Write-Host "    1. MSVC 工具链缺少 C++ 构建工具 → 安装 VS Build Tools 或切换 GNU 工具链" -ForegroundColor White
        Write-Host "    2. 网络问题 → 配置 crates.io 镜像 (见 .cargo/config.toml)" -ForegroundColor White
        Write-Host "    3. 缺少 WebView2 → Windows 10/11 通常已预装" -ForegroundColor White
        Write-Host ""
    }
} else {
    Write-Host "  启动开发模式 (npm run tauri dev)..." -ForegroundColor White
    Write-Host "  应用窗口将自动打开, 支持热重载" -ForegroundColor White
    Write-Host ""
    npm run tauri dev
}

Pop-Location

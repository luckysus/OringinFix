# OringinFix

[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-green)](./LICENSE)
[![Release](https://img.shields.io/github/v/release/luckysus/OringinFix)](https://github.com/luckysus/OringinFix/releases)

轻量 Windows 工具：安全清理 **OriginLab Origin** 关闭后残留的后台进程（`Origin64.exe`）。

- GitHub：<https://github.com/luckysus/OringinFix>
- Gitee：<https://gitee.com/neusu/OringinFix>
- 下载：<https://github.com/luckysus/OringinFix/releases>

## 背景

Origin 有时在关闭主窗口后仍会留下无界面的后台进程，占用内存并影响再次启动。手动在任务管理器里结束进程既麻烦，也容易误杀正在使用或自动化脚本拉起的实例。

OringinFix 只针对真正「无窗口且非自动化」的残留进程，尽量做到可放心常开。

## 功能

- **一键清理**：识别并结束残留的 `Origin64.exe`
- **清理自动化**：可二次确认后清理 `-Embedding` 实例
- **自动巡检**：默认每 5 秒检查一次（可关，设置会写入 `%AppData%\OringinFix\config.toml`）
- **系统托盘**：关闭窗口隐藏到托盘；菜单支持显示 / 清理 / 自动清理 / 开机自启 / 退出
- **开机自启**：可选写入当前用户 Run 键
- **保护在用实例**：有可见窗口的进程不会被清理
- **保护 COM 自动化**：默认不自动清理 `-Embedding` 实例
- **启动缓冲**：进程存活不足 10 秒时暂不处理
- **轻量界面**：无边框小窗，Acrylic / Mica；中文优先用系统字体以减小体积

## 工作原理

对每个 `Origin64.exe` 进程按以下规则分类：

| 类型 | 判断条件 | 行为 |
|------|----------|------|
| 在用 | 存在可见且标题非空的窗口 | 保留 |
| 自动化 | 命令行包含 `-Embedding` | 默认保留（可手动确认清理） |
| 刚启动 | 无窗口，但存活时间 &lt; 10 秒 | 暂不处理 |
| 残留 | 无窗口、非自动化、已运行足够久 | 可清理 |

终止前会再次核对进程镜像文件名；失败时会区分「权限不足」等结果。

## 环境要求

- Windows 10 / 11（x64）
- 构建时需要 [Rust](https://rustup.rs/)（edition 2024）与 MSVC 工具链

## 安装

### 预编译（推荐安装包）

从 [Releases](https://github.com/luckysus/OringinFix/releases) 下载：

| 文件 | 说明 |
|------|------|
| `OringinFix-Setup-x.y.z.exe` | **安装包**：开始菜单、可选开机自启；卸载时可清除 `%AppData%\OringinFix` 与开机项 |
| `OringinFix.exe` | 绿色版，下载即用 |

安装无需管理员（默认装到 `%LocalAppData%\Programs\OringinFix`）。

> 代码签名证书可选。未签名时 Windows SmartScreen 可能提示「未知发布者」，选「仍要运行」即可。

### 从源码构建

```bash
git clone https://github.com/luckysus/OringinFix.git
cd OringinFix
cargo build --release
```

产物：`target/release/OringinFix.exe`

打安装包（需 [NSIS 3](https://nsis.sourceforge.io/)：`winget install NSIS.NSIS`）：

```powershell
.\installer\build.ps1
```

输出：`dist\OringinFix-Setup-<version>.exe`（当前用户安装、可选桌面快捷方式；卸载时可清除 `%AppData%\OringinFix`）

国内用户可改用 Gitee：

```bash
git clone https://gitee.com/neusu/OringinFix.git
```

## 使用说明

### 图形界面

运行 `OringinFix.exe`：

1. **清理后台进程**：清理无窗口残留
2. **清理自动化实例…**：二次确认后清理 `-Embedding`
3. **自动清理 / 开机自启**：状态会保存
4. 点关闭会**隐藏到托盘**；在托盘菜单选「退出」才真正结束

### 命令行

```bash
OringinFix.exe --check        # 只报告状态
OringinFix.exe --clean        # 清理残留一次后退出
OringinFix.exe --once         # 同 --clean
OringinFix.exe --clean-auto   # 清理自动化实例一次后退出
```

示例：

```text
check: idle
check: stale=[1234] age=[120s] windowed=[] auto=[5678]
已清理 1 个残留进程，1 个自动化未动
```

可配合计划任务定期执行 `--clean`。

### 调试：更换监控目标

```powershell
$env:ORINGINFIX_TARGET = "SomeOther.exe"
.\OringinFix.exe
```

### 配置文件

`%AppData%\OringinFix\config.toml`：

```toml
auto_clean = true
interval_secs = 5
start_with_windows = false
```

## 项目结构

```text
OringinFix/
├── src/
│   ├── main.rs      # CLI 入口
│   ├── origin.rs    # 进程扫描与清理
│   ├── app.rs       # GUI + 托盘
│   ├── config.rs    # 配置持久化
│   ├── startup.rs   # 开机自启
│   └── fonts.rs     # 字体加载
├── .github/workflows/release.yml
├── assets/
└── README.md
```

## 注意事项

- 请仅在确认 Origin 已关闭、且无需保留后台自动化时清理。
- 自动化实例无法区分「正在计算」与「崩溃残留」；自动模式不会动它们。
- 若 Origin 以更高权限运行，可能出现「权限不足」。
- 中文显示依赖系统字体（如微软雅黑）；也可把 `NotoSansSC-Regular.ttf` 放在 exe 同目录作为回退。

## 贡献

欢迎 Issue 与 Pull Request。提交前建议：

```bash
cargo build --release
OringinFix.exe --check
```

若改动了清理判定逻辑，请说明测试场景（有窗口 / 自动化 / 刚启动 / 纯残留）。

## 致谢

- UI：[egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui)
- 托盘：[tray-icon](https://github.com/tauri-apps/tray-icon)
- 字体：[Inter](https://github.com/rsms/inter)（SIL OFL）；系统 CJK 字体

## License

本项目采用 [MIT License](./LICENSE) 发布。

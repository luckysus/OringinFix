# OringinFix

[![Platform](https://img.shields.io/badge/platform-Windows-0078D4?logo=windows&logoColor=white)](https://www.microsoft.com/windows)
[![Rust](https://img.shields.io/badge/rust-2024-orange?logo=rust)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-green)](./LICENSE)

轻量 Windows 工具：安全清理 **OriginLab Origin** 关闭后残留的后台进程（`Origin64.exe`）。

- GitHub：<https://github.com/luckysus/OringinFix>
- Gitee：<https://gitee.com/neusu/OringinFix>

## 背景

Origin 有时在关闭主窗口后仍会留下无界面的后台进程，占用内存并影响再次启动。手动在任务管理器里结束进程既麻烦，也容易误杀正在使用或自动化脚本拉起的实例。

OringinFix 只针对真正「无窗口且非自动化」的残留进程，尽量做到可放心常开。

## 功能

- **一键清理**：识别并结束残留的 `Origin64.exe`
- **自动巡检**：默认每 5 秒检查一次（可关闭）
- **保护在用实例**：有可见窗口的进程不会被清理
- **保护 COM 自动化**：命令行带 `-Embedding` 的实例不会被清理
- **启动缓冲**：进程存活不足 10 秒时暂不处理，避免误杀刚打开的 Origin
- **轻量界面**：无边框小窗，支持 Acrylic / Mica 毛玻璃效果

## 工作原理

对每个 `Origin64.exe` 进程按以下规则分类：

| 类型 | 判断条件 | 行为 |
|------|----------|------|
| 在用 | 存在可见且标题非空的窗口 | 保留 |
| 自动化 | 命令行包含 `-Embedding` | 保留（仅提示） |
| 刚启动 | 无窗口，但存活时间 &lt; 10 秒 | 暂不处理 |
| 残留 | 无窗口、非自动化、已运行足够久 | 可清理 |

终止前会再次核对进程镜像文件名，降低 PID 复用导致误杀的风险。

## 环境要求

- Windows 10 / 11（x64）
- 构建时需要 [Rust](https://rustup.rs/)（edition 2024）与 MSVC 工具链

## 安装与构建

```bash
git clone https://github.com/luckysus/OringinFix.git
cd OringinFix
cargo build --release
```

产物路径：

```text
target/release/OringinFix.exe
```

也可从本仓库的 [Releases](https://github.com/luckysus/OringinFix/releases) 下载已编译版本（若已发布）。

国内用户可改用 Gitee：

```bash
git clone https://gitee.com/neusu/OringinFix.git
```

## 使用说明

### 图形界面

直接运行 `OringinFix.exe`：

1. 点击 **清理后台进程** 立即扫描并清理
2. 勾选 **自动清理（每 5 秒检查）** 后可最小化挂机（默认开启）
3. 状态栏会显示当前检测结果（未运行 / 在用 / 已清理数量等）

### 命令行检查

仅报告状态，不结束任何进程：

```bash
OringinFix.exe --check
```

示例输出：

```text
check: idle
check: in use, windowed=1 auto=0
check: stale pids=[1234] age=[120s] windowed=0 auto=0
```

### 调试：更换监控目标

可通过环境变量临时指定进程名（默认 `Origin64.exe`）：

```powershell
$env:ORINGINFIX_TARGET = "SomeOther.exe"
.\OringinFix.exe
```

## 项目结构

```text
OringinFix/
├── src/main.rs          # 进程扫描、清理逻辑与 GUI
├── build.rs             # Windows 图标与版本资源
├── assets/              # 图标与内嵌字体
├── Cargo.toml
└── README.md
```

## 注意事项

- 本工具通过 Win32 API 结束进程，请仅在确认 Origin 已关闭、且无需保留后台自动化时使用。
- 自动化实例（`-Embedding`）无法区分「正在计算」与「崩溃残留」，因此一律不自动清理；如需结束请自行在任务管理器中处理。
- 需要查询/终止目标进程的权限；若 Origin 以更高权限运行，可能清理失败。

## 贡献

欢迎 Issue 与 Pull Request。提交前建议：

```bash
cargo build --release
OringinFix.exe --check
```

若改动了清理判定逻辑，请说明测试场景（有窗口 / 自动化 / 刚启动 / 纯残留）。

## 致谢

- UI：[egui](https://github.com/emilk/egui) / [eframe](https://github.com/emilk/egui)
- 字体：[Inter](https://github.com/rsms/inter)、[Noto Sans SC](https://fonts.google.com/noto/specimen/Noto+Sans+SC)（SIL Open Font License）

## License

本项目采用 [MIT License](./LICENSE) 发布。

内嵌字体遵循各自的 SIL Open Font License，详见 `assets/`。

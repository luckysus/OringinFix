//! 极简中英文字符串表。

use crate::config::Language;

#[derive(Clone, Copy)]
pub struct Tr {
    pub lang: Language,
}

impl Tr {
    pub fn new(lang: Language) -> Self {
        Self { lang }
    }

    fn zh(self) -> bool {
        matches!(self.lang, Language::Zh)
    }

    pub fn home(self) -> &'static str {
        if self.zh() { "首页" } else { "Home" }
    }
    pub fn settings(self) -> &'static str {
        if self.zh() { "设置" } else { "Settings" }
    }
    pub fn about(self) -> &'static str {
        if self.zh() { "关于" } else { "About" }
    }
    pub fn about_tagline(self) -> &'static str {
        if self.zh() { "Origin修复" } else { "Origin Fix" }
    }
    pub fn about_version(self) -> &'static str {
        if self.zh() { "版本" } else { "Version" }
    }
    pub fn about_check_update(self) -> &'static str {
        if self.zh() { "检查更新" } else { "Check for updates" }
    }
    pub fn about_checking(self) -> &'static str {
        if self.zh() { "正在检查…" } else { "Checking…" }
    }
    pub fn about_up_to_date(self, latest: &str) -> String {
        if self.zh() {
            format!("已是最新（v{latest}）")
        } else {
            format!("Up to date (v{latest})")
        }
    }
    pub fn about_update_available(self, latest: &str) -> String {
        if self.zh() {
            format!("发现新版本 v{latest}")
        } else {
            format!("Update available: v{latest}")
        }
    }
    pub fn about_no_release(self) -> &'static str {
        if self.zh() {
            "尚未发布任何版本"
        } else {
            "No releases published yet"
        }
    }
    pub fn about_open_release(self) -> &'static str {
        if self.zh() { "打开下载页" } else { "Open download" }
    }
    pub fn about_download_update(self) -> &'static str {
        if self.zh() { "下载更新" } else { "Download update" }
    }
    pub fn about_install_now(self) -> &'static str {
        if self.zh() { "立即安装" } else { "Install now" }
    }
    pub fn about_downloading(self) -> &'static str {
        if self.zh() { "正在下载…" } else { "Downloading…" }
    }
    pub fn about_downloading_pct(self, pct: u8) -> String {
        if self.zh() {
            format!("正在下载… {pct}%")
        } else {
            format!("Downloading… {pct}%")
        }
    }
    pub fn about_update_ready(self, latest: &str) -> String {
        if self.zh() {
            format!("v{latest} 已就绪")
        } else {
            format!("v{latest} ready")
        }
    }
    pub fn about_installing(self) -> &'static str {
        if self.zh() { "正在安装…" } else { "Installing…" }
    }
    pub fn about_download_failed(self, err: &str) -> String {
        if self.zh() {
            format!("更新失败：{err}")
        } else {
            format!("Update failed: {err}")
        }
    }
    pub fn about_check_failed(self, err: &str) -> String {
        if self.zh() {
            format!("检查失败：{err}")
        } else {
            format!("Check failed: {err}")
        }
    }
    pub fn about_license(self) -> &'static str {
        if self.zh() { "许可证" } else { "License" }
    }
    pub fn about_author(self) -> &'static str {
        if self.zh() { "作者" } else { "Author" }
    }
    pub fn about_repo(self) -> &'static str {
        if self.zh() { "开源仓库" } else { "Repositories" }
    }
    pub fn clean_stale(self) -> &'static str {
        if self.zh() { "清理残留" } else { "Clean leftovers" }
    }
    pub fn refresh(self) -> &'static str {
        if self.zh() { "刷新" } else { "Refresh" }
    }
    pub fn clean_auto(self) -> &'static str {
        if self.zh() { "清理自动化" } else { "Clean automation" }
    }
    pub fn clean_selected(self) -> &'static str {
        if self.zh() { "清理所选" } else { "Clean selected" }
    }
    pub fn no_procs(self) -> &'static str {
        if self.zh() {
            "当前没有 Origin 进程"
        } else {
            "No Origin processes"
        }
    }
    pub fn recent(self) -> &'static str {
        if self.zh() { "最近清理" } else { "Recent cleanups" }
    }
    pub fn no_log(self) -> &'static str {
        if self.zh() { "暂无记录" } else { "No history yet" }
    }
    pub fn kind_stale(self) -> &'static str {
        if self.zh() { "残留" } else { "Leftover" }
    }
    pub fn kind_windowed(self) -> &'static str {
        if self.zh() { "在用" } else { "In use" }
    }
    pub fn kind_auto(self) -> &'static str {
        if self.zh() {
            "自动化（含 PPT 嵌入）"
        } else {
            "Automation (incl. PPT OLE)"
        }
    }
    pub fn kind_protected(self) -> &'static str {
        if self.zh() { "保护" } else { "Protected" }
    }
    pub fn oldest(self) -> &'static str {
        if self.zh() { "最老" } else { "Oldest" }
    }
    pub fn just_started(self) -> &'static str {
        if self.zh() { "刚启动" } else { "starting" }
    }
    pub fn stale_mem(self, mem: &str) -> String {
        if self.zh() {
            format!("残留合计约 {mem}（清理后可释放）")
        } else {
            format!("Leftovers ≈ {mem} (reclaimable)")
        }
    }
    pub fn updated(self) -> &'static str {
        if self.zh() { "已更新" } else { "Updated" }
    }
    pub fn auto_clean(self) -> &'static str {
        if self.zh() { "自动清理" } else { "Auto clean" }
    }
    pub fn interval(self) -> &'static str {
        if self.zh() { "检查间隔" } else { "Interval" }
    }
    pub fn seconds(self) -> &'static str {
        if self.zh() { " 秒" } else { " s" }
    }
    pub fn notify(self) -> &'static str {
        if self.zh() { "清理成功时通知" } else { "Notify on clean" }
    }
    pub fn silent(self) -> &'static str {
        if self.zh() {
            "静默启动（进托盘）"
        } else {
            "Silent start (tray)"
        }
    }
    pub fn startup(self) -> &'static str {
        if self.zh() { "开机自启" } else { "Start with Windows" }
    }
    pub fn close_when(self) -> &'static str {
        if self.zh() { "关闭窗口时" } else { "On close" }
    }
    pub fn to_tray(self) -> &'static str {
        if self.zh() { "隐藏到托盘" } else { "Hide to tray" }
    }
    pub fn exit_app(self) -> &'static str {
        if self.zh() { "退出程序" } else { "Exit" }
    }
    pub fn protect(self) -> &'static str {
        if self.zh() {
            "保护参数（逗号分隔，匹配命令行则不自动清理）"
        } else {
            "Protect args (comma-separated; matched cmdline skips auto-clean)"
        }
    }
    pub fn protect_hint(self) -> &'static str {
        if self.zh() {
            "例如：-r, myscript"
        } else {
            "e.g. -r, myscript"
        }
    }
    pub fn language(self) -> &'static str {
        if self.zh() { "语言" } else { "Language" }
    }
    pub fn appearance(self) -> &'static str {
        if self.zh() { "外观" } else { "Appearance" }
    }
    pub fn theme(self) -> &'static str {
        if self.zh() { "主题" } else { "Theme" }
    }
    pub fn theme_system(self) -> &'static str {
        if self.zh() { "跟随系统" } else { "System" }
    }
    pub fn theme_light(self) -> &'static str {
        if self.zh() { "浅色" } else { "Light" }
    }
    pub fn theme_dark(self) -> &'static str {
        if self.zh() { "深色" } else { "Dark" }
    }
    pub fn lang_zh(self) -> &'static str {
        "中文"
    }
    pub fn lang_en(self) -> &'static str {
        "English"
    }
    pub fn tray_show(self) -> &'static str {
        if self.zh() { "显示窗口" } else { "Show window" }
    }
    pub fn tray_clean(self) -> &'static str {
        if self.zh() { "清理残留" } else { "Clean leftovers" }
    }
    pub fn tray_refresh(self) -> &'static str {
        if self.zh() { "刷新状态" } else { "Refresh" }
    }
    pub fn tray_quit(self) -> &'static str {
        if self.zh() { "退出" } else { "Quit" }
    }
}

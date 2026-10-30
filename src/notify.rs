//! Windows 通知（失败则静默忽略）。

pub fn toast(title: &str, body: &str) {
    let title = title.to_owned();
    let body = body.to_owned();
    // 通知 API 偶发较慢，放到后台，避免卡住 UI / 托盘线程。
    std::thread::spawn(move || {
        let _ = notify_rust::Notification::new()
            .appname("OringinFix")
            .summary(&title)
            .body(&body)
            .timeout(notify_rust::Timeout::Milliseconds(4500))
            .show();
    });
}

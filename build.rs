fn main() {
    println!("cargo:rerun-if-changed=assets/app.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/app.ico");
    res.set("ProductName", "OringinFix");
    res.set("FileDescription", "OringinFix");
    // 缺 rc.exe 时只提示，不让构建失败。
    if let Err(e) = res.compile() {
        eprintln!("icon embedding skipped: {e}");
    }
}

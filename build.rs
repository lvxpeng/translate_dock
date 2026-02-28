fn main() {
    // 仅在 Windows 下嵌入应用图标到 .exe
    if std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default() == "windows" {
        let mut res = winres::WindowsResource::new();
        res.set_icon("src/assets/icons/ico.ico");
        if let Err(e) = res.compile() {
            eprintln!("Warning: failed to embed icon: {}", e);
        }
    }
}

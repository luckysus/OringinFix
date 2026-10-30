#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod fonts;
mod i18n;
mod log;
mod notify;
mod origin;
mod startup;
mod theme;
mod update;

fn attach_console() {
    unsafe {
        let _ = windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        );
    }
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let check = args.iter().any(|a| a == "--check");
    let clean = args.iter().any(|a| a == "--clean" || a == "--once");
    let clean_auto = args.iter().any(|a| a == "--clean-auto");
    let silent = args.iter().any(|a| a == "--silent");

    if check || clean || clean_auto {
        attach_console();
        let protect = config::Config::load().protect_args;
        let zh = matches!(config::Config::load().language, config::Language::Zh);
        if check {
            println!("{}", origin::report_check(&protect));
        }
        if clean {
            let (msg, _) = origin::release(&protect, zh);
            println!("{msg}");
        }
        if clean_auto {
            let (msg, _) = origin::release_auto(&protect, zh);
            println!("{msg}");
        }
        return Ok(());
    }

    app::run(silent)
}

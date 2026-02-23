// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("backsnap — System Backup & Recovery Manager\n");
        println!("Verwendung: backsnap [OPTIONEN]\n");
        println!("Optionen:");
        println!("  --sync              Sync headless ausführen (für systemd/cron)");
        println!("  --config <pfad>     Config-Pfad (Standard: ~/.config/backsnap/config.toml)");
        println!("  --help, -h          Diese Hilfe anzeigen");
        println!("\nOhne Optionen wird die GUI gestartet.");
        return;
    }

    if args.iter().any(|a| a == "--sync") {
        let config_path = args
            .iter()
            .position(|a| a == "--config")
            .and_then(|i| args.get(i + 1))
            .cloned();
        std::process::exit(app_lib::run_sync_cli(config_path));
    }

    app_lib::run();
}

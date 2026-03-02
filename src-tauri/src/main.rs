// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

/// CLI sub-commands parsed from `std::env::args`.
enum CliCommand {
    Help,
    SysfsWrite(String),
    FileOps(String),
    VerifyCollect(String),
    SyncElevated { config: Option<String> },
    RollbackElevated { snap_id: u32, config: Option<String> },
    RollbackRecover { config: Option<String> },
    Sync { config: Option<String> },
    Gui,
}

impl CliCommand {
    #[allow(clippy::print_stderr)]
    fn from_args(args: &[String]) -> Self {
        if args.iter().any(|a| a == "--help" || a == "-h") {
            return Self::Help;
        }

        let config = || -> Option<String> {
            args.iter()
                .position(|a| a == "--config")
                .and_then(|i| args.get(i + 1))
                .cloned()
        };

        if let Some(pos) = args.iter().position(|a| a == "--sysfs-write") {
            return Self::SysfsWrite(args.get(pos + 1).cloned().unwrap_or_default());
        }
        if let Some(pos) = args.iter().position(|a| a == "--file-ops") {
            return Self::FileOps(args.get(pos + 1).cloned().unwrap_or_default());
        }
        if let Some(pos) = args.iter().position(|a| a == "--verify-collect") {
            return Self::VerifyCollect(args.get(pos + 1).cloned().unwrap_or_default());
        }
        if args.iter().any(|a| a == "--sync-elevated") {
            return Self::SyncElevated { config: config() };
        }
        if let Some(pos) = args.iter().position(|a| a == "--rollback-elevated") {
            let snap_id: u32 = args
                .get(pos + 1)
                .and_then(|s| s.parse().ok())
                .unwrap_or_else(|| {
                    eprintln!("--rollback-elevated benötigt eine Snapshot-ID");
                    std::process::exit(1);
                });
            return Self::RollbackElevated { snap_id, config: config() };
        }
        if args.iter().any(|a| a == "--rollback-recover") {
            return Self::RollbackRecover { config: config() };
        }
        if args.iter().any(|a| a == "--sync") {
            return Self::Sync { config: config() };
        }
        Self::Gui
    }
}

#[allow(clippy::print_stdout, clippy::print_stderr)]
fn main() {
    let args: Vec<String> = std::env::args().collect();

    match CliCommand::from_args(&args) {
        CliCommand::Help => {
            println!("backsnap — System Backup & Recovery Manager\n");
            println!("Verwendung: backsnap [OPTIONEN]\n");
            println!("Optionen:");
            println!("  --sync              Sync headless ausführen (für systemd/cron)");
            println!("  --config <pfad>     Config-Pfad (Standard: ~/.config/backsnap/config.toml)");
            println!("  --sysfs-write <json> Sysfs-Werte schreiben (JSON-Array, nur als root)");
            println!("  --verify-collect <json> Backup-Daten sammeln (nur als root)");
            println!("  --sync-elevated     Sync als root ausführen (intern, von GUI gestartet)");
            println!(
                "  --rollback-elevated <id>  Rollback als root ausführen (intern, von GUI gestartet)"
            );
            println!("  --rollback-recover    Rollback-Recovery Wizard (Rescue-CLI, root, interaktiv)");
            println!("  --file-ops <json>   Dateioperationen als root (intern, von GUI gestartet)");
            println!("  --help, -h          Diese Hilfe anzeigen");
            println!("\nOhne Optionen wird die GUI gestartet.");
        }
        CliCommand::SysfsWrite(json) => {
            std::process::exit(app_lib::run_sysfs_write(&json));
        }
        CliCommand::FileOps(json) => {
            std::process::exit(app_lib::run_file_ops(&json));
        }
        CliCommand::VerifyCollect(json) => {
            std::process::exit(app_lib::run_verify_collect(&json));
        }
        CliCommand::SyncElevated { config } => {
            std::process::exit(app_lib::run_sync_elevated(config));
        }
        CliCommand::RollbackElevated { snap_id, config } => {
            std::process::exit(app_lib::run_rollback_elevated(snap_id, config));
        }
        CliCommand::RollbackRecover { config } => {
            std::process::exit(app_lib::run_rollback_recover(config));
        }
        CliCommand::Sync { config } => {
            std::process::exit(app_lib::run_sync_cli(config));
        }
        CliCommand::Gui => {
            app_lib::run();
        }
    }
}

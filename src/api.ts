import { invoke } from "@tauri-apps/api/core";

// ─── Types ────────────────────────────────────────────────────

export interface DiskInfo {
  name: string;
  uuid: string;
  size: string;
  mountpoint: string;
  fstype: string;
  used: string;
  avail: string;
  use_percent: string;
}

export interface Snapshot {
  id: number;
  snap_type: string;
  pre_id: number | null;
  date: string;
  user: string;
  cleanup: string;
  description: string;
}

export interface SyncStatus {
  last_sync: string | null;
  last_duration: string | null;
  timer_active: boolean;
  timer_next: string | null;
  timer_last_trigger: string | null;
  direction: string;
  log_tail: string[];
  sync_running: boolean;
}

export interface SystemStatus {
  hostname: string;
  kernel: string;
  uptime: string;
  boot_disk: string;
  boot_uuid: string;
  disks: DiskInfo[];
  snapper_configs: string[];
  snapshot_counts: { config: string; count: number }[];
  sync_status: SyncStatus;
}

export interface TimerConfig {
  enabled: boolean;
  calendar: string;
  randomized_delay: string;
  last_trigger: string | null;
  service_result: string | null;
}

export interface HealthCheck {
  primary_present: boolean;
  backup_present: boolean;
  snapper_installed: boolean;
  rsync_installed: boolean;
  btrfs_tools: boolean;
  boot_disk: string;
  issues: string[];
}

export interface SubvolumeInfo {
  id: string;
  gen: string;
  top_level: string;
  path: string;
}

// ─── Config Types ─────────────────────────────────────────────

export interface AppConfig {
  disks: DiskConfig;
  sync: SyncConfigData;
  boot: BootConfig;
  snapper: SnapperConfig;
  rollback: RollbackConfig;
}

export interface DiskConfig {
  primary_uuid: string;
  primary_label: string;
  backup_uuid: string;
  backup_label: string;
}

export interface SyncConfigData {
  timer_unit: string;
  service_unit: string;
  log_path: string;
  log_max_lines: number;
  mount_options: string;
  mount_base: string;
  subvolumes: SubvolSync[];
  system_excludes: string[];
  home_excludes: string[];
  home_extra_excludes: string[];
  extra_excludes_on_primary: boolean;
}

export interface SubvolSync {
  name: string;
  subvol: string;
  source: string;
  delete: boolean;
}

export interface BootConfig {
  sync_enabled: boolean;
  excludes: string[];
}

export interface SnapperConfig {
  expected_configs: string[];
}

export interface RollbackConfig {
  max_broken_backups: number;
  recovery_label: string;
  root_subvol: string;
}

export interface DetectedDisk {
  device: string;
  uuid: string;
  label: string;
  size: string;
  model: string;
  mountpoint: string | null;
  is_boot: boolean;
}

export interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exit_code: number;
}

// ─── API Calls ────────────────────────────────────────────────

export const api = {
  getSystemStatus: () => invoke<SystemStatus>("get_system_status"),
  getSnapshots: (config: string) =>
    invoke<Snapshot[]>("get_snapshots", { config }),
  createSnapshot: (config: string, description: string) =>
    invoke<CommandResult>("create_snapshot", { config, description }),
  deleteSnapshot: (config: string, id: number) =>
    invoke<CommandResult>("delete_snapshot", { config, id }),
  runSync: () => invoke<CommandResult>("run_sync"),
  getSyncStatus: () => invoke<SyncStatus>("get_sync_status"),
  getSyncLog: () => invoke<string[]>("get_sync_log"),
  getTimerConfig: () => invoke<TimerConfig>("get_timer_config"),
  setTimerEnabled: (enabled: boolean) =>
    invoke<CommandResult>("set_timer_enabled", { enabled }),
  rollbackSnapshot: (config: string, id: number) =>
    invoke<CommandResult>("rollback_snapshot", { config, id }),
  getSnapperDiff: (config: string, id: number) =>
    invoke<string>("get_snapper_diff", { config, id }),
  getBtrfsUsage: () => invoke<string>("get_btrfs_usage"),
  getHealth: () => invoke<HealthCheck>("get_health"),
  getSubvolumes: () => invoke<SubvolumeInfo[]>("get_subvolumes"),
  getConfig: () => invoke<AppConfig>("get_config"),
  saveConfig: (newConfig: AppConfig) =>
    invoke<void>("save_config_cmd", { newConfig }),
  detectDisks: () => invoke<DetectedDisk[]>("detect_disks"),
  resetConfig: () => invoke<AppConfig>("reset_config"),
  installTimer: (calendar: string, delay: string) =>
    invoke<CommandResult>("install_timer", { calendar, delay }),
  uninstallTimer: () => invoke<CommandResult>("uninstall_timer"),
};

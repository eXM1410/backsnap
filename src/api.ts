import { invoke } from "@tauri-apps/api/core";

// ─── Types ────────────────────────────────────────────────────

export interface DiskInfo {
  name: string;
  model: string;
  role: string;
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

export interface NestedMount {
  path: string;
  rel_path: string;
  device: string;
  fstype: string;
  excluded: boolean;
  reason: string;
}

export interface SyncScopeEntry {
  name: string;
  source: string;
  subvol: string;
  delete: boolean;
  excludes: string[];
  nested_mounts: NestedMount[];
}

export interface SyncScope {
  direction: string;
  boot_sync: boolean;
  subvolumes: SyncScopeEntry[];
}

export interface SystemStatus {
  hostname: string;
  kernel: string;
  uptime: string;
  boot_disk: string;
  backup_disk: string;
  boot_uuid: string;
  disks: DiskInfo[];
  snapper_configs: string[];
  snapshot_counts: { config: string; count: number }[];
  sync_status: SyncStatus;
  boot_info: BootInfo | null;
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
  boot_validation: BootValidation | null;
}

export interface BootValidation {
  backup_efi_accessible: boolean;
  bootloader_present: boolean;
  entries_valid: boolean;
  kernels_present: string[];
  kernels_missing: string[];
  entry_issues: string[];
}

export interface BootInfo {
  current_entry: string;
  bootloader_version: string;
  entries: BootEntryInfo[];
  backup_bootable: boolean;
  backup_bootloader_version: string | null;
  booted_from: string;
}

export interface BootEntryInfo {
  title: string;
  id: string;
  root_uuid: string;
  kernel: string;
  disk: string;
}

export interface SubvolumeInfo {
  id: string;
  gen: string;
  top_level: string;
  path: string;
}

export interface SnapperLimits {
  config: string;
  values: Record<string, string>;
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
  bootloader_type: string;
  excludes: string[];
}

export interface SnapperConfig {
  expected_configs: string[];
}

export interface RollbackConfig {
  max_broken_backups: number;
  recovery_label: string;
  root_subvol: string;
  root_config: string;
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

export interface ScannedExclude {
  path: string;
  category: string;
  reason: string;
  size_bytes: number;
  size_human: string;
  auto_exclude: boolean;
}

export interface ScanPhase {
  phase: number;
  label: string;
}

export interface ExcludeScanRuntimeStats {
  cpu_threads: number;
  io_workers_cap: number;
  rayon_threads: number;
  tokio_blocking_task: number;
}

export interface CommandResult {
  success: boolean;
  stdout: string;
  stderr: string;
  exit_code: number;
}

// ─── Cleanup Types ────────────────────────────────────────────

export interface CleanupItem {
  path: string;
  abs_path: string;
  category: string;
  reason: string;
  size_bytes: number;
  size_human: string;
  safe: boolean;
  ai_checked?: boolean;
  ai_confidence?: number | null;
  ai_note?: string | null;
}

export interface DeleteResult {
  path: string;
  success: boolean;
  error: string | null;
}

export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
  size_bytes: number;
}

// ─── System Integration Types ──────────────────────────

export interface IntegrationStatus {
  binary: boolean;
  desktop: boolean;
  polkit: boolean;
  pacman_hook: boolean;
  binary_path: string;
}

// ─── Tuning Types ──────────────────────────────────────

export interface TweakInfo {
  id: string;
  category: string;
  name: string;
  description: string;
  current: string;
  status: string;
  active: boolean;
  recommended: string;
  options: string[];
  /** "toggle" | "select" | "slider" | "info" */
  control: string;
  min?: number | null;
  max?: number | null;
}

export interface TuningStatus {
  tweaks: TweakInfo[];
}

export interface TuningApplyResult {
  success: boolean;
  message: string;
  new_value: string;
}

export interface GpuOcStatus {
  available: boolean;
  gpu_name: string;
  vram_mb: number;
  sclk_min: number;
  sclk_max: number;
  sclk_range_min: number;
  sclk_range_max: number;
  mclk_max: number;
  mclk_range_min: number;
  mclk_range_max: number;
  voltage_offset: number;
  voltage_min: number;
  voltage_max: number;
  power_cap_w: number;
  power_default_w: number;
  power_max_w: number;
  power_current_w: number;
  temp_edge: number;
  temp_junction: number;
  temp_mem: number;
  fan_rpm: number;
  fan_max_rpm: number;
  fan_pwm: number;
  fan_mode: string;
  current_sclk_mhz: number;
  current_mclk_mhz: number;
  gpu_busy_percent: number;
}

// ─── Backup Verification Types ──────────────────────────────

export interface BackupCheck {
  name: string;
  ok: boolean;
  detail: string;
}

export interface BackupVerifyResult {
  backup_dev: string;
  overall_ok: boolean;
  checks: BackupCheck[];
}

// ─── System Monitor Types ─────────────────────────────────────

export interface SystemMonitorData {
  cpu: CpuMonInfo;
  memory: MemoryMonInfo;
  swap: SwapMonInfo;
  cpu_sensor: CpuSensorInfo;
  gpu: GpuMonInfo;
  load: LoadAvgInfo;
  uptime: UptimeMonInfo;
  battery: BatteryMonInfo | null;
  extra_power: ExtraPowerInfo;
  nvme_temps: NvmeTempInfo[];
}

export interface CpuMonInfo {
  model: string;
  cores: number;
  threads: number;
  usage_percent: number;
  per_core_usage: number[];
  frequency_mhz: number | null;
  architecture: string;
}

export interface MemoryMonInfo {
  total_mib: number;
  used_mib: number;
  available_mib: number;
  percent: number;
}

export interface SwapMonInfo {
  total_mib: number;
  used_mib: number;
  percent: number;
}

export interface CpuSensorInfo {
  temp_celsius: number | null;
  power_watts: number | null;
  power_no_permission: boolean;
}

export interface GpuMonInfo {
  name: string;
  temp_celsius: number | null;
  power_watts: number | null;
  vram_total_mib: number | null;
  vram_used_mib: number | null;
  gpu_busy_percent: number | null;
}

export interface LoadAvgInfo {
  one: number;
  five: number;
  fifteen: number;
}

export interface UptimeMonInfo {
  seconds: number;
  formatted: string;
}

export interface BatteryMonInfo {
  power_watts: number;
}

export interface ExtraPowerInfo {
  dram_watts: number | null;
  platform_watts: number | null;
  total_system_watts: number | null;
}

export interface NvmeTempInfo {
  name: string;
  temp_celsius: number;
}

// ─── Pi Remote Types ──────────────────────────────────────────

export interface PiDevice {
  id: string;
  label: string;
  model: string;
  ip: string;
  user: string;
  ssh_key: string;
  mount_point: string;
  remote_protocol: string;
  remote_port: number;
  rdp_password: string;
  watch_services: string[];
}

export interface PiTestResult {
  reachable: boolean;
  ssh_ok: boolean;
  hostname: string | null;
  model: string | null;
  kernel: string | null;
  error: string | null;
}

export interface PiService {
  name: string;
  active: boolean;
  enabled: boolean;
}

export interface PiStatus {
  id: string;
  label: string;
  model: string;
  ip: string;
  online: boolean;
  uptime: string | null;
  cpu_temp: number | null;
  cpu_usage: number | null;
  cpu_freq_mhz: number | null;
  mem_total_mb: number | null;
  mem_used_mb: number | null;
  disk_total_gb: number | null;
  disk_used_gb: number | null;
  disk_percent: number | null;
  hostname: string | null;
  kernel: string | null;
  throttled: string | null;
  nfs_mounted: boolean;
  services: PiService[];
  remote_protocol: string;
  remote_port: number;
}

export interface PiActionResult {
  success: boolean;
  message: string;
}

// ─── Boot Guard ───────────────────────────────────────────────

export interface EntryHealth {
  filename: string;
  title: string;
  kernel_exists: boolean;
  initramfs_exists: boolean;
  custom_params_intact: boolean;
  missing_params: string[];
  options: string;
  changed_since_backup: boolean;
  diff: string[];
}

export interface BackupInfo {
  timestamp: number;
  label: string;
  entry_count: number;
}

export interface BootHealth {
  boot_mounted: boolean;
  boot_device: string | null;
  running_kernel: string;
  installed_modules: string[];
  kernel_module_match: boolean;
  entries: EntryHealth[];
  status: "healthy" | "warning" | "critical";
  issues: string[];
  backups: BackupInfo[];
}

export interface RestoreResult {
  success: boolean;
  restored: string[];
  errors: string[];
}

// ─── API Calls ────────────────────────────────────────────────
/** Normalise a caught value to a readable string (Tauri errors come back as strings). */
export function apiError(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
export const api = {
  getSystemStatus: () => invoke<SystemStatus>("get_system_status"),
  getSnapshots: (config: string) =>
    invoke<Snapshot[]>("get_snapshots", { config }),
  createSnapshot: (config: string, description: string) =>
    invoke<CommandResult>("create_snapshot", { config, description }),
  deleteSnapshot: (config: string, id: number) =>
    invoke<CommandResult>("delete_snapshot", { config, id }),
  getSnapperLimits: (config: string) =>
    invoke<SnapperLimits>("get_snapper_limits", { config }),
  runSnapperCleanup: (config: string) =>
    invoke<CommandResult>("run_snapper_cleanup", { config }),
  runSync: () => invoke<CommandResult>("run_sync"),
  getSyncStatus: () => invoke<SyncStatus>("get_sync_status"),
  getSyncLog: () => invoke<string[]>("get_sync_log"),
  getSyncScope: () => invoke<SyncScope>("get_sync_scope"),
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
  getActivityLog: () => invoke<string[]>("get_activity_log"),
  saveConfig: (newConfig: AppConfig) =>
    invoke<void>("save_config_cmd", { newConfig }),
  detectDisks: () => invoke<DetectedDisk[]>("detect_disks"),
  resetConfig: () => invoke<AppConfig>("reset_config"),
  scanExcludes: () => invoke<void>("scan_excludes"),
  installTimer: (calendar: string, delay: string) =>
    invoke<CommandResult>("install_timer", { calendar, delay }),
  uninstallTimer: () => invoke<CommandResult>("uninstall_timer"),
  getSystemMonitor: () => invoke<SystemMonitorData>("get_system_monitor"),
  getBootInfo: () => invoke<BootInfo>("get_boot_info"),
  scanCleanup: (aiAssist?: boolean) =>
    invoke<CleanupItem[]>("scan_cleanup", { aiAssist, ai_assist: aiAssist }),
  cancelScan: () => invoke<void>("cancel_scan"),
  deleteCleanupPaths: (paths: string[]) =>
    invoke<DeleteResult[]>("delete_cleanup_paths", { paths }),
  getCleanupDirContents: (relPath: string) =>
    invoke<DirEntry[]>("get_cleanup_dir_contents", { relPath }),
  verifyBackup: () => invoke<BackupVerifyResult>("verify_backup"),
  getIntegrationStatus: () => invoke<IntegrationStatus>("get_integration_status"),
  installSystemIntegration: () => invoke<string>("install_system_integration"),
  uninstallSystemIntegration: () => invoke<string>("uninstall_system_integration"),
  getTuningStatus: () => invoke<TuningStatus>("get_tuning_status"),
  applyTuning: (tweakId: string, value: string) =>
    invoke<TuningApplyResult>("apply_tuning", { tweak_id: tweakId, value }),
  getGpuOcStatus: () => invoke<GpuOcStatus>("get_gpu_oc_status"),
  applyGpuOc: (params: {
    sclk_max?: number;
    mclk_max?: number;
    voltage_offset?: number;
    power_cap_w?: number;
    fan_mode?: string;
    fan_pwm?: number;
  }) => invoke<TuningApplyResult>("apply_gpu_oc", params),
  resetGpuOc: () => invoke<TuningApplyResult>("reset_gpu_oc"),
  getGpuOcServiceStatus: () => invoke<boolean>("get_gpu_oc_service_status"),
  installGpuOcService: () => invoke<TuningApplyResult>("install_gpu_oc_service"),
  uninstallGpuOcService: () => invoke<TuningApplyResult>("uninstall_gpu_oc_service"),

  // Pi Remote
  getPiDevices: () => invoke<PiDevice[]>("get_pi_devices"),
  getPiStatusAll: () => invoke<PiStatus[]>("get_pi_status_all"),
  getPiStatus: (id: string) => invoke<PiStatus>("get_pi_status", { id }),
  piReboot: (id: string) => invoke<PiActionResult>("pi_reboot", { id }),
  piShutdown: (id: string) => invoke<PiActionResult>("pi_shutdown", { id }),
  piRunCommand: (id: string, command: string) =>
    invoke<PiActionResult>("pi_run_command", { id, command }),
  testPiConnection: (ip: string, user: string, sshKey: string) =>
    invoke<PiTestResult>("test_pi_connection", { ip, user, sshKey }),
  addPiDevice: (device: PiDevice) =>
    invoke<PiActionResult>("add_pi_device", { device }),
  removePiDevice: (id: string) =>
    invoke<PiActionResult>("remove_pi_device", { id }),
  openPiRemote: (id: string) =>
    invoke<PiActionResult>("open_pi_remote", { id }),

  // Boot Guard
  getBootHealth: () => invoke<BootHealth>("get_boot_health"),
  backupBootEntries: (label?: string) =>
    invoke<BackupInfo>("backup_boot_entries", { label }),
  restoreBootEntries: (timestamp: number) =>
    invoke<RestoreResult>("restore_boot_entries", { timestamp }),
  deleteBootBackup: (timestamp: number) =>
    invoke<string>("delete_boot_backup", { timestamp }),
};

import { useEffect, useState } from "react";
import {
  Camera,
  HardDrive,
  RefreshCw,
  Clock,
  CheckCircle2,
  AlertTriangle,
  XCircle,
  Server,
  Cpu,
  Activity,
  Shield,
  Zap,
  Monitor,
} from "lucide-react";
import { api, SystemStatus, BootEntryInfo } from "../api";
import { Card, StatCard, Badge, PageHeader, Loading } from "../components/ui";

export default function Dashboard() {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = async (isBackground = false) => {
    if (!isBackground) setLoading(true);
    try {
      const s = await api.getSystemStatus();
      setStatus(s);
      setError(null);
    } catch (e: any) {
      setError(e.toString());
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    const interval = setInterval(() => refresh(true), 30000);
    return () => clearInterval(interval);
  }, []);

  if (loading && !status) return <div className="p-8"><Loading /></div>;
  if (error && !status)
    return (
      <div className="p-8 text-red-400">
        <AlertTriangle className="w-6 h-6 mb-2" />
        Fehler: {error}
      </div>
    );
  if (!status) return null;

  const totalSnapshots = status.snapshot_counts.reduce(
    (a, b) => a + b.count,
    0
  );
  const rootDisk = status.disks.find(
    (d) => d.fstype === "btrfs" && d.mountpoint === "/"
  );

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <PageHeader
        title="Dashboard"
        description={`${status.hostname} — ${status.kernel}`}
      />

      {/* Status Bar */}
      <div className="grid grid-cols-4 gap-4 mb-6">
        <StatCard
          label="Boot-Disk"
          value={status.boot_disk.split(" (")[0]}
          icon={HardDrive}
          color="text-cyan-400"
          sub={
            status.boot_info?.booted_from === "Backup" ? "Backup-Modus!" : "Primary"
          }
        />
        <StatCard
          label="Snapshots"
          value={totalSnapshots}
          icon={Camera}
          color="text-emerald-400"
          sub={status.snapshot_counts
            .map((s) => `${s.config}: ${s.count}`)
            .join(", ")}
        />
        <StatCard
          label="NVMe Sync"
          value={status.sync_status.timer_active ? "Aktiv" : "Inaktiv"}
          icon={RefreshCw}
          color={
            status.sync_status.timer_active
              ? "text-emerald-400"
              : "text-red-400"
          }
          sub={status.sync_status.direction}
        />
        <StatCard
          label="Disk-Belegung"
          value={rootDisk?.use_percent || "–"}
          icon={Server}
          color="text-amber-400"
          sub={rootDisk ? `${rootDisk.used} / ${rootDisk.size}` : ""}
        />
      </div>

      {/* System Info + Sync */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        {/* System */}
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
            <Cpu className="w-4 h-4" /> System
          </h3>
          <div className="space-y-3">
            <InfoRow label="Hostname" value={status.hostname} />
            <InfoRow label="Kernel" value={status.kernel} />
            <InfoRow label="Uptime" value={status.uptime} />
            <InfoRow
              label="Boot UUID"
              value={status.boot_uuid.slice(0, 13) + "..."}
              mono
            />
          </div>
        </Card>

        {/* Last Sync */}
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
            <RefreshCw className="w-4 h-4" /> Letzter Sync
          </h3>
          <div className="space-y-3">
            <InfoRow
              label="Richtung"
              value={status.sync_status.direction}
            />
            <InfoRow
              label="Timer"
              value={
                status.sync_status.timer_active ? "Aktiv" : "Deaktiviert"
              }
              badge={status.sync_status.timer_active ? "green" : "red"}
            />
            <InfoRow
              label="Nächster Lauf"
              value={status.sync_status.timer_next || "—"}
            />
            <InfoRow
              label="Letzter Sync"
              value={
                status.sync_status.last_sync?.replace(/\[.*?\]\s*/, "") ||
                "Noch keiner"
              }
            />
          </div>
        </Card>
      </div>

      {/* Boot-Übersicht — Dual Disk Visual */}
      {status.boot_info && (() => {
        const primaryEntries = status.boot_info.entries.filter(e => e.disk === "Primary");
        const backupEntries = status.boot_info.entries.filter(e => e.disk === "Backup");
        const isPrimaryBoot = status.boot_info.booted_from === "Primary";

        return (
          <div className="grid grid-cols-2 gap-4 mb-6">
            {/* Primary Disk */}
            <BootDiskCard
              label={status.boot_disk.split(" (")[0]}
              subtitle="Primary"
              isActive={isPrimaryBoot}
              isBootable={true}
              bootloaderVersion={status.boot_info!.bootloader_version}
              entries={primaryEntries}
              currentEntry={status.boot_info!.current_entry}
              accentColor="cyan"
            />

            {/* Backup Disk */}
            <BootDiskCard
              label={status.backup_disk || "Backup"}
              subtitle="Backup"
              isActive={!isPrimaryBoot}
              isBootable={status.boot_info!.backup_bootable}
              bootloaderVersion={status.boot_info!.backup_bootloader_version || undefined}
              entries={backupEntries}
              currentEntry={status.boot_info!.current_entry}
              accentColor="emerald"
            />
          </div>
        );
      })()}

      {/* Disk Overview */}
      <Card>
        <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
          <HardDrive className="w-4 h-4" /> Partitionen
        </h3>
        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-zinc-500 border-b border-zinc-800">
                <th className="pb-2 font-medium">Mount</th>
                <th className="pb-2 font-medium">Device</th>
                <th className="pb-2 font-medium">Größe</th>
                <th className="pb-2 font-medium">Belegt</th>
                <th className="pb-2 font-medium">Frei</th>
                <th className="pb-2 font-medium">%</th>
              </tr>
            </thead>
            <tbody>
              {status.disks.map((disk, i) => (
                <tr key={i} className="border-b border-zinc-800/50">
                  <td className="py-2 font-mono text-xs text-cyan-400">
                    {disk.mountpoint.includes(', ') ? (
                      <div className="flex flex-wrap gap-1">
                        {disk.mountpoint.split(', ').map((m, j) => (
                          <span key={j} className="bg-zinc-800 px-1.5 py-0.5 rounded text-[11px]">{m}</span>
                        ))}
                      </div>
                    ) : disk.mountpoint}
                  </td>
                  <td className="py-2 font-mono text-xs text-zinc-500">
                    {disk.name}
                  </td>
                  <td className="py-2">{disk.size}</td>
                  <td className="py-2">{disk.used}</td>
                  <td className="py-2">{disk.avail}</td>
                  <td className="py-2">
                    <DiskBar percent={parseInt(disk.use_percent)} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </div>
  );
}

function BootDiskCard({
  label,
  subtitle,
  isActive,
  isBootable,
  bootloaderVersion,
  entries,
  currentEntry,
  accentColor,
}: {
  label: string;
  subtitle: string;
  isActive: boolean;
  isBootable: boolean;
  bootloaderVersion?: string;
  entries: BootEntryInfo[];
  currentEntry: string;
  accentColor: "cyan" | "emerald";
}) {
  const borderColor = isActive
    ? accentColor === "cyan" ? "border-cyan-500/40" : "border-emerald-500/40"
    : "border-zinc-800";
  const glowBg = isActive
    ? accentColor === "cyan" ? "bg-cyan-500/5" : "bg-emerald-500/5"
    : "";
  const dotColor = accentColor === "cyan" ? "bg-cyan-400" : "bg-emerald-400";
  const textAccent = accentColor === "cyan" ? "text-cyan-400" : "text-emerald-400";
  const ringColor = accentColor === "cyan" ? "ring-cyan-500/20" : "ring-emerald-500/20";

  return (
    <div className={`rounded-xl border ${borderColor} ${glowBg} p-5 transition-all`}>
      {/* Header */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-3">
          <div className={`w-10 h-10 rounded-lg flex items-center justify-center ring-2 ${ringColor} ${
            isActive ? (accentColor === "cyan" ? "bg-cyan-500/15" : "bg-emerald-500/15") : "bg-zinc-800/50"
          }`}>
            <HardDrive className={`w-5 h-5 ${isActive ? textAccent : "text-zinc-500"}`} />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <span className="font-semibold text-sm">{label}</span>
              {isActive && (
                <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-[10px] font-bold uppercase tracking-wider ${
                  accentColor === "cyan" ? "bg-cyan-500/15 text-cyan-400" : "bg-emerald-500/15 text-emerald-400"
                }`}>
                  <Zap className="w-2.5 h-2.5" /> Aktiv
                </span>
              )}
            </div>
            <span className="text-[11px] text-zinc-500">{subtitle}</span>
          </div>
        </div>
        {/* Status indicator */}
        <div className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium ${
          isBootable
            ? "bg-emerald-500/10 text-emerald-400"
            : "bg-red-500/10 text-red-400"
        }`}>
          {isBootable ? <CheckCircle2 className="w-3.5 h-3.5" /> : <XCircle className="w-3.5 h-3.5" />}
          {isBootable ? "Bootbar" : "Nicht bootbar"}
        </div>
      </div>

      {/* Bootloader Version */}
      {bootloaderVersion && (
        <div className="flex items-center gap-2 mb-3 px-3 py-1.5 rounded-lg bg-zinc-900/60 border border-zinc-800/50">
          <Monitor className="w-3 h-3 text-zinc-500" />
          <span className="text-[11px] text-zinc-500">Bootloader</span>
          <span className="text-[11px] text-zinc-300 ml-auto font-mono">{bootloaderVersion}</span>
        </div>
      )}

      {/* Boot Entries */}
      <div className="space-y-1.5">
        <span className="text-[10px] uppercase tracking-wider text-zinc-500 font-semibold">
          Einträge ({entries.length})
        </span>
        {entries.length === 0 ? (
          <div className="text-xs text-zinc-600 italic py-2">Keine Entries gefunden</div>
        ) : (
          entries.map((entry, i) => {
            const isCurrent = currentEntry.includes(entry.id);
            return (
              <div
                key={i}
                className={`flex items-center gap-3 px-3 py-2 rounded-lg transition ${
                  isCurrent
                    ? accentColor === "cyan" ? "bg-cyan-500/8 border border-cyan-500/20" : "bg-emerald-500/8 border border-emerald-500/20"
                    : "bg-zinc-900/40 border border-transparent hover:border-zinc-800"
                }`}
              >
                {/* Status dot */}
                <span className={`w-2 h-2 rounded-full flex-shrink-0 ${
                  isCurrent ? dotColor : "bg-zinc-600"
                } ${isCurrent ? "shadow-sm shadow-current" : ""}`} />

                {/* Entry info */}
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className={`text-xs font-medium truncate ${isCurrent ? "text-zinc-100" : "text-zinc-400"}`}>
                      {entry.title}
                    </span>
                  </div>
                  <span className="text-[10px] text-zinc-600 font-mono">{entry.kernel}</span>
                </div>

                {/* Kernel badge */}
                <span className={`text-[10px] px-1.5 py-0.5 rounded font-mono flex-shrink-0 ${
                  isCurrent
                    ? accentColor === "cyan" ? "bg-cyan-500/10 text-cyan-400" : "bg-emerald-500/10 text-emerald-400"
                    : "bg-zinc-800 text-zinc-500"
                }`}>
                  {entry.id.includes("lts") ? "LTS" : entry.id.includes("rescue") ? "Rescue" : "Default"}
                </span>
              </div>
            );
          })
        )}
      </div>
    </div>
  );
}

function InfoRow({
  label,
  value,
  mono = false,
  badge,
}: {
  label: string;
  value: string;
  mono?: boolean;
  badge?: "green" | "red" | "cyan";
}) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-xs text-zinc-500">{label}</span>
      {badge ? (
        <Badge color={badge}>{value}</Badge>
      ) : (
        <span className={`text-sm ${mono ? "font-mono text-xs" : ""}`}>
          {value}
        </span>
      )}
    </div>
  );
}

function DiskBar({ percent }: { percent: number }) {
  const safePercent = isNaN(percent) ? 0 : percent;
  const color =
    safePercent > 90
      ? "bg-red-500"
      : safePercent > 70
        ? "bg-amber-500"
        : "bg-cyan-500";
  return (
    <div className="flex items-center gap-2">
      <div className="w-16 h-1.5 bg-zinc-800 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full ${color}`}
          style={{ width: `${safePercent}%` }}
        />
      </div>
      <span className="text-xs text-zinc-500">{safePercent}%</span>
    </div>
  );
}

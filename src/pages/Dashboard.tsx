import { useEffect, useState } from "react";
import {
  Camera,
  HardDrive,
  RefreshCw,
  Clock,
  CheckCircle2,
  AlertTriangle,
  Server,
  Cpu,
  Activity,
  Shield,
} from "lucide-react";
import { api, SystemStatus } from "../api";
import { Card, StatCard, Badge, PageHeader, Loading } from "../components/ui";

export default function Dashboard() {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  const refresh = async () => {
    setLoading(true);
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
    const interval = setInterval(refresh, 30000);
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
  const rootDisk = status.disks.find((d) => d.fstype === "btrfs" && d.mountpoint.startsWith("/"));

  return (
    <div className="p-8">
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

      {/* Boot Info */}
      {status.boot_info && (
        <Card className="mb-6">
          <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
            <Shield className="w-4 h-4" /> Boot-Menü (systemd-boot)
          </h3>
          <div className="grid grid-cols-2 gap-6">
            <div className="space-y-3">
              <InfoRow label="Aktiver Entry" value={status.boot_info.current_entry} />
              <InfoRow label="Bootloader" value={status.boot_info.bootloader_version || "—"} />
              <InfoRow label="Gebootet von" value={status.boot_info.booted_from} badge={status.boot_info.booted_from === "Primary" ? "cyan" : "red"} />
              <InfoRow
                label="Backup bootbar"
                value={status.boot_info.backup_bootable ? "Ja" : "Nein"}
                badge={status.boot_info.backup_bootable ? "green" : "red"}
              />
              {status.boot_info.backup_bootloader_version && (
                <InfoRow label="Backup-Bootloader" value={status.boot_info.backup_bootloader_version} />
              )}
            </div>
            <div>
              <span className="text-xs text-zinc-500 block mb-2">Boot-Entries ({status.boot_info.entries.length})</span>
              <div className="space-y-1.5">
                {status.boot_info.entries.map((entry, i) => (
                  <div key={i} className="flex items-center justify-between text-sm">
                    <div className="flex items-center gap-2">
                      <span className={`w-1.5 h-1.5 rounded-full ${
                        status.boot_info!.current_entry.includes(entry.id) ? "bg-emerald-400" : "bg-zinc-600"
                      }`} />
                      <span className="text-xs">{entry.title}</span>
                    </div>
                    <Badge color={entry.disk === "Primary" ? "cyan" : entry.disk === "Backup" ? "green" : "red"}>
                      {entry.disk}
                    </Badge>
                  </div>
                ))}
              </div>
            </div>
          </div>
        </Card>
      )}

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
  const color =
    percent > 90
      ? "bg-red-500"
      : percent > 70
        ? "bg-amber-500"
        : "bg-cyan-500";
  return (
    <div className="flex items-center gap-2">
      <div className="w-16 h-1.5 bg-zinc-800 rounded-full overflow-hidden">
        <div
          className={`h-full rounded-full ${color}`}
          style={{ width: `${percent}%` }}
        />
      </div>
      <span className="text-xs text-zinc-500">{percent}%</span>
    </div>
  );
}

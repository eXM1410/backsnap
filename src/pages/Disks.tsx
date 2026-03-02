import { useEffect, useState } from "react";
import { HardDrive, Database } from "lucide-react";
import { api, DiskInfo } from "../api";
import { Card, Badge, PageHeader, Loading } from "../components/ui";

export default function Disks() {
  const [disks, setDisks] = useState<DiskInfo[]>([]);
  const [btrfsUsage, setBtrfsUsage] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");

  useEffect(() => {
    const load = async () => {
      try {
        const status = await api.getSystemStatus();
        setDisks(status.disks);
        try {
          const usage = await api.getBtrfsUsage();
          setBtrfsUsage(usage);
        } catch (e) {
          // May need pkexec
          setBtrfsUsage("Benötigt Root-Rechte (pkexec)");
        }
      } catch (e) {
        console.error(e);
        setError(String(e));
      }
      setLoading(false);
    };
    load();
  }, []);

  if (loading) return <div className="p-8"><Loading /></div>;
  if (error && disks.length === 0) return (
    <div className="p-8">
      <PageHeader title="Disks" description="NVMe-Laufwerke und Btrfs-Subvolumes" />
      <Card className="p-6 text-red-400 text-sm">{error}</Card>
    </div>
  );

  return (
    <div className="p-8">
      <PageHeader
        title="Disks"
        description="NVMe-Laufwerke und Btrfs-Subvolumes"
      />

      {/* Disk Cards */}
      <div className="grid grid-cols-1 gap-4 mb-6">
        {disks.map((disk, i) => {
          const percent = parseInt(disk.use_percent) || 0;
          const color =
            percent > 90
              ? "bg-red-500"
              : percent > 70
                ? "bg-amber-500"
                : "bg-cyan-500";
          return (
            <Card key={disk.uuid || disk.name || i}>
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-3">
                  <HardDrive className="w-5 h-5 text-zinc-500" />
                  <div>
                    <div className="flex items-center gap-2">
                      <h3 className="font-mono text-sm text-cyan-400">
                        {disk.mountpoint}
                      </h3>
                      {disk.role && (
                        <Badge color={disk.role === "System Disk" ? "cyan" : disk.role === "Backup Disk" ? "emerald" : "zinc"}>
                          {disk.role}
                        </Badge>
                      )}
                    </div>
                    <p className="text-xs text-zinc-500 mt-0.5">
                      {disk.model} <span className="text-zinc-700 mx-1">•</span> {disk.name}
                    </p>
                  </div>
                </div>
                <div className="flex items-center gap-3">
                  <Badge color="zinc">{disk.fstype}</Badge>
                  {disk.uuid && (
                    <span className="text-xs font-mono text-zinc-600" title={disk.uuid}>
                      {disk.uuid.slice(0, 8)}...
                    </span>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-4">
                <div className="flex-1">
                  {disk.use_percent !== "—" ? (
                    <div className="w-full h-2 bg-zinc-800 rounded-full overflow-hidden">
                      <div
                        className={`h-full rounded-full transition-all ${color}`}
                        style={{ width: `${percent}%` }}
                      />
                    </div>
                  ) : (
                    <div className="w-full h-2 bg-zinc-800/50 rounded-full" />
                  )}
                </div>
                <div className="text-sm text-zinc-400 whitespace-nowrap">
                  {disk.used !== "—" ? (
                    <>
                      {disk.used} / {disk.size}
                      <span className="text-zinc-600 ml-1">({disk.use_percent})</span>
                    </>
                  ) : (
                    <>
                      {disk.size}
                      <span className="text-zinc-600 ml-1">(nicht gemountet)</span>
                    </>
                  )}
                </div>
              </div>
            </Card>
          );
        })}
      </div>

      {/* Btrfs Usage */}
      <Card>
        <h3 className="text-sm font-semibold text-zinc-400 mb-3 flex items-center gap-2">
          <Database className="w-4 h-4" /> Btrfs Filesystem Usage
        </h3>
        <pre className="text-xs font-mono text-zinc-500 bg-zinc-950 rounded-lg p-4 overflow-x-auto whitespace-pre">
          {btrfsUsage || "Keine Daten verfügbar"}
        </pre>
      </Card>
    </div>
  );
}

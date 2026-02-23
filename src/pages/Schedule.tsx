import { useEffect, useState } from "react";
import { Clock, Power, Save } from "lucide-react";
import { api, TimerConfig } from "../api";
import { Card, Button, Badge, PageHeader, Loading } from "../components/ui";

export default function Schedule() {
  const [config, setConfig] = useState<TimerConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState(false);

  const refresh = async () => {
    try {
      const c = await api.getTimerConfig();
      setConfig(c);
    } catch (e) {
      console.error(e);
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();
  }, []);

  const toggleTimer = async () => {
    if (!config) return;
    setToggling(true);
    await api.setTimerEnabled(!config.enabled);
    await refresh();
    setToggling(false);
  };

  if (loading) return <div className="p-8"><Loading /></div>;

  return (
    <div className="p-8">
      <PageHeader
        title="Zeitplan"
        description="Automatische Sync-Intervalle konfigurieren"
      />

      {/* Timer Status */}
      <Card className="mb-6">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-4">
            <div
              className={`w-12 h-12 rounded-xl flex items-center justify-center ${
                config?.enabled ? "bg-emerald-500/10" : "bg-zinc-800"
              }`}
            >
              <Clock
                className={`w-6 h-6 ${
                  config?.enabled ? "text-emerald-400" : "text-zinc-600"
                }`}
              />
            </div>
            <div>
              <h3 className="font-semibold text-lg flex items-center gap-2">
                nvme-sync Timer
                <Badge color={config?.enabled ? "green" : "red"}>
                  {config?.enabled ? "Aktiv" : "Deaktiviert"}
                </Badge>
              </h3>
              <p className="text-sm text-zinc-500">
                Synchronisiert System automatisch auf die Backup-NVMe
              </p>
            </div>
          </div>
          <Button
            variant={config?.enabled ? "danger" : "primary"}
            onClick={toggleTimer}
            loading={toggling}
          >
            <Power className="w-4 h-4" />
            {config?.enabled ? "Deaktivieren" : "Aktivieren"}
          </Button>
        </div>
      </Card>

      {/* Timer Details */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4">
            Timer-Konfiguration
          </h3>
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <span className="text-sm text-zinc-500">Intervall</span>
              <span className="text-sm font-mono bg-zinc-800 px-3 py-1 rounded">
                {config?.calendar || "daily"}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-zinc-500">Zufällige Verzögerung</span>
              <span className="text-sm font-mono bg-zinc-800 px-3 py-1 rounded">
                {config?.randomized_delay || "1h"}
              </span>
            </div>
            <div className="flex items-center justify-between">
              <span className="text-sm text-zinc-500">Persistent</span>
              <Badge color="green">Ja</Badge>
            </div>
          </div>
        </Card>

        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4">
            Was wird synchronisiert?
          </h3>
          <div className="space-y-2">
            <SyncItem label="System (/)" included />
            <SyncItem label="Home (/home)" included />
            <SyncItem label="Boot-Kernel" included />
            <SyncItem label="Games" included={false} />
            <SyncItem label="Steam-Spieledaten" included={false} />
            <SyncItem label="Cache" included={false} />
          </div>
        </Card>
      </div>

      {/* Info */}
      <Card>
        <h3 className="text-sm font-semibold text-zinc-400 mb-3">Hinweise</h3>
        <ul className="space-y-2 text-sm text-zinc-500">
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Der Timer wird bei Systemstart automatisch gestartet (systemd Persistent=true)
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Verpasste Syncs werden nach dem nächsten Boot nachgeholt
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Sync läuft mit niedrigster IO-Priorität (ionice idle)
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Manuellen Sync über die „NVMe Sync" Seite auslösen
          </li>
        </ul>
      </Card>
    </div>
  );
}

function SyncItem({
  label,
  included,
}: {
  label: string;
  included: boolean;
}) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-sm">{label}</span>
      <Badge color={included ? "green" : "zinc"}>
        {included ? "✓" : "✗"}
      </Badge>
    </div>
  );
}

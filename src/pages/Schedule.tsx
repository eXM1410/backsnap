import { useEffect, useState } from "react";
import { Clock, Power, Download, Trash2, RefreshCw } from "lucide-react";
import { api, TimerConfig, AppConfig } from "../api";
import { Card, Button, Badge, PageHeader, Loading } from "../components/ui";

export default function Schedule() {
  const [config, setConfig] = useState<TimerConfig | null>(null);
  const [appConfig, setAppConfig] = useState<AppConfig | null>(null);
  const [loading, setLoading] = useState(true);
  const [toggling, setToggling] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);
  const [calendar, setCalendar] = useState("daily");
  const [delay, setDelay] = useState("1h");
  const [message, setMessage] = useState<{ text: string; ok: boolean } | null>(null);

  // Check if timer unit files exist
  const [timerInstalled, setTimerInstalled] = useState<boolean | null>(null);

  const refresh = async () => {
    try {
      const [c, ac] = await Promise.all([
        api.getTimerConfig(),
        api.getConfig(),
      ]);
      setConfig(c);
      setAppConfig(ac);
      // If we get a calendar value back, the unit exists
      setTimerInstalled(c.calendar !== "" && c.calendar !== "n/a");
      if (c.calendar && c.calendar !== "n/a") {
        setCalendar(c.calendar);
      }
      if (c.randomized_delay && c.randomized_delay !== "0") {
        setDelay(c.randomized_delay);
      }
    } catch (e) {
      console.error(e);
      setTimerInstalled(false);
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();
  }, []);

  const toggleTimer = async () => {
    if (!config) return;
    setToggling(true);
    try {
      await api.setTimerEnabled(!config.enabled);
      setMessage({ text: config.enabled ? "Timer deaktiviert" : "Timer aktiviert", ok: true });
    } catch (e: any) {
      setMessage({ text: `Fehler: ${e}`, ok: false });
    }
    await refresh();
    setToggling(false);
  };

  const handleInstall = async () => {
    setInstalling(true);
    setMessage(null);
    try {
      const result = await api.installTimer(calendar, delay);
      setMessage({ text: result.stdout, ok: result.success });
      await refresh();
    } catch (e: any) {
      setMessage({ text: `Fehler: ${e}`, ok: false });
    }
    setInstalling(false);
  };

  const handleUninstall = async () => {
    setUninstalling(true);
    setMessage(null);
    try {
      const result = await api.uninstallTimer();
      setMessage({ text: result.stdout, ok: result.success });
      await refresh();
    } catch (e: any) {
      setMessage({ text: `Fehler: ${e}`, ok: false });
    }
    setUninstalling(false);
  };

  if (loading) return <div className="p-8"><Loading /></div>;

  // Derive sync items from config
  const syncItems = appConfig
    ? [
        ...appConfig.sync.subvolumes.map((sv) => ({
          label: `${sv.name} (${sv.source})`,
          included: true,
        })),
        {
          label: "Boot/EFI",
          included: appConfig.boot.sync_enabled,
        },
        ...appConfig.sync.home_extra_excludes.map((exc) => ({
          label: exc.split("/").pop() || exc,
          included: false,
        })),
        { label: "Cache", included: false },
      ]
    : [];

  return (
    <div className="p-8">
      <PageHeader
        title="Zeitplan"
        description="Automatische Sync-Intervalle konfigurieren"
      />

      {/* Message */}
      {message && (
        <div
          className={`mb-4 p-3 rounded-lg text-sm ${
            message.ok
              ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20"
              : "bg-red-500/10 text-red-400 border border-red-500/20"
          }`}
        >
          <pre className="whitespace-pre-wrap font-mono text-xs">{message.text}</pre>
        </div>
      )}

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
                {appConfig?.sync.timer_unit || "backsnap-sync.timer"}
                <Badge color={config?.enabled ? "green" : timerInstalled ? "yellow" : "red"}>
                  {config?.enabled
                    ? "Aktiv"
                    : timerInstalled
                    ? "Deaktiviert"
                    : "Nicht installiert"}
                </Badge>
              </h3>
              <p className="text-sm text-zinc-500">
                Synchronisiert System automatisch auf die Backup-Disk
              </p>
            </div>
          </div>
          <div className="flex gap-2">
            {timerInstalled && (
              <Button
                variant={config?.enabled ? "danger" : "primary"}
                onClick={toggleTimer}
                loading={toggling}
              >
                <Power className="w-4 h-4" />
                {config?.enabled ? "Deaktivieren" : "Aktivieren"}
              </Button>
            )}
          </div>
        </div>
      </Card>

      {/* Install / Config Section */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4">
            Timer {timerInstalled ? "aktualisieren" : "installieren"}
          </h3>
          <div className="space-y-3">
            <div>
              <label className="text-xs text-zinc-500 mb-1 block">
                Intervall (OnCalendar)
              </label>
              <input
                type="text"
                value={calendar}
                onChange={(e) => setCalendar(e.target.value)}
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-cyan-500"
                placeholder="daily, weekly, *-*-* 01:00:00"
              />
            </div>
            <div>
              <label className="text-xs text-zinc-500 mb-1 block">
                Zufällige Verzögerung
              </label>
              <input
                type="text"
                value={delay}
                onChange={(e) => setDelay(e.target.value)}
                className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-2 text-sm font-mono focus:outline-none focus:border-cyan-500"
                placeholder="1h, 30min, 0"
              />
            </div>
            <div className="flex gap-2 pt-2">
              <Button variant="primary" onClick={handleInstall} loading={installing}>
                <Download className="w-4 h-4" />
                {timerInstalled ? "Aktualisieren" : "Installieren"}
              </Button>
              {timerInstalled && (
                <Button variant="danger" onClick={handleUninstall} loading={uninstalling}>
                  <Trash2 className="w-4 h-4" />
                  Deinstallieren
                </Button>
              )}
              <Button variant="ghost" onClick={refresh}>
                <RefreshCw className="w-4 h-4" />
              </Button>
            </div>
          </div>
          <p className="text-xs text-zinc-600 mt-3">
            Erstellt systemd Service + Timer unter /etc/systemd/system/.
            Führt <code className="text-cyan-500">backsnap --sync</code> als Root aus.
          </p>
        </Card>

        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4">
            Was wird synchronisiert?
          </h3>
          <div className="space-y-2">
            {syncItems.map((item, i) => (
              <SyncItem key={i} label={item.label} included={item.included} />
            ))}
          </div>
        </Card>
      </div>

      {/* Timer Details (only if installed) */}
      {timerInstalled && config && (
        <Card className="mb-6">
          <h3 className="text-sm font-semibold text-zinc-400 mb-4">
            Timer-Details
          </h3>
          <div className="grid grid-cols-2 gap-4">
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-500">Intervall</span>
                <span className="text-sm font-mono bg-zinc-800 px-3 py-1 rounded">
                  {config.calendar || "—"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-500">Verzögerung</span>
                <span className="text-sm font-mono bg-zinc-800 px-3 py-1 rounded">
                  {config.randomized_delay || "—"}
                </span>
              </div>
            </div>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-500">Letzter Trigger</span>
                <span className="text-sm font-mono bg-zinc-800 px-3 py-1 rounded">
                  {config.last_trigger || "—"}
                </span>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-sm text-zinc-500">Service-Ergebnis</span>
                <Badge
                  color={
                    config.service_result === "success"
                      ? "green"
                      : config.service_result
                      ? "red"
                      : "zinc"
                  }
                >
                  {config.service_result || "—"}
                </Badge>
              </div>
            </div>
          </div>
        </Card>
      )}

      {/* Info */}
      <Card>
        <h3 className="text-sm font-semibold text-zinc-400 mb-3">Hinweise</h3>
        <ul className="space-y-2 text-sm text-zinc-500">
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Der Timer erstellt einen systemd Service der <code className="text-cyan-500">backsnap --sync</code> als Root ausführt
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Persistent=true: Verpasste Syncs werden nach dem nächsten Boot nachgeholt
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            Der Sync läuft mit niedrigster IO-Priorität (ionice idle, nice 19)
          </li>
          <li className="flex items-start gap-2">
            <span className="text-cyan-400 mt-0.5">•</span>
            CLI-Modus: <code className="text-cyan-500">backsnap --sync [--config pfad]</code>
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

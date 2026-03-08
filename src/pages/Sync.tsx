import { useEffect, useState, useRef } from "react";
import {
  RefreshCw,
  Play,
  CheckCircle2,
  XCircle,
  ArrowRight,
  Loader2,
  ShieldCheck,
  ChevronDown,
  ChevronUp,
  Clock,
  Power,
  Download,
  Trash2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api, SyncStatus, SyncScope, CommandResult, BackupVerifyResult, TimerConfig } from "../api";
import { Card, Button, Badge, PageHeader, Loading } from "../components/ui";

interface ByteProgress {
  phase: string;
  bytes: number;
  pct: number;
  speed: string;
}

interface SyncProgress {
  step: string;
  detail: string;
  percent?: number;
}

const STEP_LABELS: Record<string, string> = {
  init: "Initialisierung",
  boot: "Boot",
  done: "Fertig",
};

export default function Sync() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<CommandResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const [byteProgress, setByteProgress] = useState<ByteProgress | null>(null);
  const [verifying, setVerifying] = useState(false);
  const [verifyResult, setVerifyResult] = useState<BackupVerifyResult | null>(null);
  const [verifyOpen, setVerifyOpen] = useState(false);
  const [scope, setScope] = useState<SyncScope | null>(null);
  const [scopeOpen, setScopeOpen] = useState(false);
  const logEndRef = useRef<HTMLDivElement>(null);

  // Timer management state
  const [timerConfig, setTimerConfig] = useState<TimerConfig | null>(null);
  const [timerInstalled, setTimerInstalled] = useState<boolean | null>(null);
  const [timerOpen, setTimerOpen] = useState(false);
  const [toggling, setToggling] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [uninstalling, setUninstalling] = useState(false);
  const [calendar, setCalendar] = useState("daily");
  const [delay, setDelay] = useState("1h");
  const [timerMsg, setTimerMsg] = useState<{ text: string; ok: boolean } | null>(null);

  const refresh = async () => {
    try {
      const [s, l, sc, tc] = await Promise.all([
        api.getSyncStatus(),
        api.getSyncLog(),
        api.getSyncScope(),
        api.getTimerConfig().catch(() => null),
      ]);
      setStatus(s);
      setLogs(l);
      setScope(sc);
      if (tc) {
        setTimerConfig(tc);
        const installed = tc.calendar !== "" && tc.calendar !== "n/a";
        setTimerInstalled(installed);
        if (tc.calendar && tc.calendar !== "n/a") setCalendar(tc.calendar);
        if (tc.randomized_delay && tc.randomized_delay !== "0") setDelay(tc.randomized_delay);
      }
      setError("");
    } catch (e) {
      console.error(e);
      setError(String(e));
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 10000);
    return () => clearInterval(interval);
  }, []);

  // Listen for live sync progress events from Rust backend
  useEffect(() => {
    const unlisten = listen<SyncProgress>("sync-progress", (event) => {
      setProgress(event.payload);
      if (event.payload.step === "done") {
        setByteProgress(null);
        refresh();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  // Listen for rsync byte-level progress
  useEffect(() => {
    const unlisten = listen<ByteProgress>("rsync-bytes-progress", (event) => {
      setByteProgress(event.payload);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (syncing) logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs, syncing]);

  const handleSync = async () => {
    setSyncing(true);
    setSyncResult(null);
    setProgress(null);
    setByteProgress(null);
    try {
      const result = await api.runSync();
      setSyncResult(result);
      refresh();
    } catch (e: any) {
      setSyncResult({
        success: false,
        stdout: "",
        stderr: e.toString(),
        exit_code: -1,
      });
    }
    setSyncing(false);
    setProgress(null);
    setByteProgress(null);
  };

  const handleVerify = async () => {
    setVerifying(true);
    setVerifyResult(null);
    setVerifyOpen(true);
    try {
      const result = await api.verifyBackup();
      setVerifyResult(result);
    } catch (e: any) {
      setVerifyResult({
        backup_dev: "",
        overall_ok: false,
        checks: [{ name: "Fehler", ok: false, detail: e.toString() }],
      });
    }
    setVerifying(false);
  };

  // Timer actions
  const toggleTimer = async () => {
    if (!timerConfig) return;
    setToggling(true);
    try {
      const result = await api.setTimerEnabled(!timerConfig.enabled);
      if (result.success) {
        setTimerMsg({ text: timerConfig.enabled ? "Timer deaktiviert" : "Timer aktiviert", ok: true });
      } else {
        setTimerMsg({ text: result.stderr || result.stdout || "Fehlgeschlagen", ok: false });
      }
    } catch (e: any) {
      setTimerMsg({ text: `Fehler: ${e}`, ok: false });
    }
    await refresh();
    setToggling(false);
  };

  const handleInstallTimer = async () => {
    setInstalling(true);
    setTimerMsg(null);
    try {
      const result = await api.installTimer(calendar, delay);
      setTimerMsg({ text: result.stdout, ok: result.success });
      await refresh();
    } catch (e: any) {
      setTimerMsg({ text: `Fehler: ${e}`, ok: false });
    }
    setInstalling(false);
  };

  const handleUninstallTimer = async () => {
    setUninstalling(true);
    setTimerMsg(null);
    try {
      const result = await api.uninstallTimer();
      setTimerMsg({ text: result.stdout, ok: result.success });
      await refresh();
    } catch (e: any) {
      setTimerMsg({ text: `Fehler: ${e}`, ok: false });
    }
    setUninstalling(false);
  };

  if (loading) return <div className="p-8"><Loading /></div>;

  return (
    <div className="p-8">
      <PageHeader
        title="NVMe Sync"
        description="Systemdaten zwischen Primary und Backup synchronisieren"
        actions={
          <div className="flex gap-2">
            <Button
              onClick={handleVerify}
              loading={verifying}
              disabled={verifying || syncing}
              variant="secondary"
            >
              <ShieldCheck className="w-4 h-4" />
              {verifying ? "Prüfe..." : "Backup verifizieren"}
            </Button>
            <Button onClick={handleSync} loading={syncing} disabled={syncing}>
              <Play className="w-4 h-4" />
              {syncing ? "Sync läuft..." : "Jetzt synchronisieren"}
            </Button>
          </div>
        }
      />

      {/* Progress Indicator */}
      {syncing && progress && (
        <Card className="mb-4 border border-cyan-500/30">
          <div className="flex items-center gap-3 mb-3">
            <Loader2 className="w-5 h-5 text-cyan-400 animate-spin" />
            <span className="font-semibold text-cyan-400">
              Sync läuft: {STEP_LABELS[progress.step] || progress.step}
            </span>
            {progress.percent != null && (
              <span className="ml-auto text-xs text-zinc-500">{progress.percent}%</span>
            )}
          </div>
          <p className="text-sm text-zinc-400">{progress.detail}</p>
          <div className="mt-3 h-1.5 w-full bg-zinc-700 rounded-full overflow-hidden">
            <div
              className="h-full bg-gradient-to-r from-cyan-500 to-emerald-500 rounded-full transition-all duration-500"
              style={{ width: `${progress.percent ?? 0}%` }}
            />
          </div>
          {byteProgress && (
            <div className="mt-3 flex items-center gap-4 text-xs text-zinc-400">
              <span className="font-mono">
                {(byteProgress.bytes / 1_048_576).toFixed(1)} MB
              </span>
              <div className="flex-1 h-1 bg-zinc-700 rounded-full overflow-hidden">
                <div
                  className="h-full bg-cyan-600 rounded-full transition-all duration-300"
                  style={{ width: `${byteProgress.pct}%` }}
                />
              </div>
              <span>{byteProgress.pct}%</span>
              <span className="text-zinc-500">{byteProgress.speed}</span>
            </div>
          )}
        </Card>
      )}

      {/* Status Cards */}
      <div className="grid grid-cols-3 gap-4 mb-6">
        <Card>
          <div className="flex items-center gap-2 mb-2">
            <RefreshCw className="w-4 h-4 text-zinc-500" />
            <span className="text-xs text-zinc-500 uppercase tracking-wider">
              Richtung
            </span>
          </div>
          <div className="flex items-center gap-2 text-lg font-semibold">
            <span className="text-cyan-400">
              {status?.direction.split("->")[0]?.trim() || "—"}
            </span>
            <ArrowRight className="w-4 h-4 text-zinc-600" />
            <span className="text-zinc-400">
              {status?.direction.split("->")[1]?.trim() || "—"}
            </span>
          </div>
        </Card>

        <Card>
          <div className="flex items-center gap-2 mb-2">
            {status?.timer_active ? (
              <CheckCircle2 className="w-4 h-4 text-emerald-400" />
            ) : (
              <XCircle className="w-4 h-4 text-red-400" />
            )}
            <span className="text-xs text-zinc-500 uppercase tracking-wider">
              Timer
            </span>
          </div>
          <div className="text-lg font-semibold">
            <Badge color={status?.timer_active ? "green" : "red"}>
              {status?.timer_active ? "Aktiv" : "Deaktiviert"}
            </Badge>
          </div>
          {status?.timer_next && (
            <p className="text-xs text-zinc-500 mt-2">
              Nächster Lauf: {status.timer_next}
            </p>
          )}
        </Card>

        <Card>
          <div className="flex items-center gap-2 mb-2">
            <CheckCircle2 className="w-4 h-4 text-zinc-500" />
            <span className="text-xs text-zinc-500 uppercase tracking-wider">
              Letzter Sync
            </span>
          </div>
          <p className="text-sm text-zinc-300">
            {status?.last_sync?.replace(/\[.*?\]\s*/, "") || "Noch keiner"}
          </p>
        </Card>
      </div>

      {/* Sync Scope */}
      {scope && (
        <Card className="mb-4 border border-zinc-700">
          <button
            className="w-full flex items-center gap-2 text-left"
            onClick={() => setScopeOpen((o) => !o)}
          >
            <RefreshCw className="w-5 h-5 text-cyan-400" />
            <span className="font-semibold flex-1">Sync-Umfang</span>
            <span className="text-xs text-zinc-500">
              {scope.subvolumes.length} Subvolume{scope.subvolumes.length !== 1 && "s"}
              {scope.boot_sync && " + Boot"}
            </span>
            {scopeOpen ? (
              <ChevronUp className="w-4 h-4 text-zinc-500" />
            ) : (
              <ChevronDown className="w-4 h-4 text-zinc-500" />
            )}
          </button>

          {scopeOpen && (
            <div className="mt-4 space-y-4">
              {scope.subvolumes.map((sv) => (
                <div key={sv.name} className="bg-zinc-900 rounded-lg p-3">
                  <div className="flex items-center gap-2 mb-2">
                    <span className="text-sm font-semibold text-cyan-400">{sv.name}</span>
                    <span className="text-xs text-zinc-500 font-mono">{sv.source}</span>
                    <span className="text-xs text-zinc-600">({sv.subvol})</span>
                    {sv.delete && (
                      <Badge color="yellow">--delete</Badge>
                    )}
                  </div>

                  <div className="text-xs text-zinc-500 mb-2">
                    {sv.excludes.length} Exclude-Regeln
                  </div>

                  {sv.nested_mounts.length > 0 && (
                    <div className="space-y-1.5">
                      <span className="text-xs text-zinc-400 font-semibold">
                        Verschachtelte Mounts:
                      </span>
                      {sv.nested_mounts.map((m) => (
                        <div
                          key={m.path}
                          className={`flex items-start gap-2 text-xs rounded p-1.5 ${
                            m.excluded
                              ? "bg-zinc-800/50"
                              : "bg-amber-900/20 border border-amber-500/30"
                          }`}
                        >
                          {m.excluded ? (
                            <XCircle className="w-3.5 h-3.5 text-zinc-500 mt-0.5 shrink-0" />
                          ) : (
                            <CheckCircle2 className="w-3.5 h-3.5 text-amber-400 mt-0.5 shrink-0" />
                          )}
                          <div className="flex-1 min-w-0">
                            <div className="flex items-center gap-2">
                              <span className="font-mono text-zinc-300 truncate">{m.path}</span>
                              <Badge color={m.excluded ? "zinc" : "yellow"}>
                                {m.fstype}
                              </Badge>
                            </div>
                            <div className="text-zinc-500 truncate">{m.device}</div>
                            <div className={m.excluded ? "text-zinc-600" : "text-amber-400"}>
                              {m.excluded ? m.reason : "⚠ Wird mitgesynced!"}
                            </div>
                          </div>
                        </div>
                      ))}
                    </div>
                  )}

                  {sv.nested_mounts.length === 0 && (
                    <div className="text-xs text-zinc-600">Keine verschachtelten Mounts</div>
                  )}
                </div>
              ))}

              {scope.boot_sync && (
                <div className="bg-zinc-900 rounded-lg p-3">
                  <div className="flex items-center gap-2">
                    <span className="text-sm font-semibold text-cyan-400">boot</span>
                    <span className="text-xs text-zinc-500 font-mono">/boot/</span>
                    <Badge color="blue">EFI</Badge>
                  </div>
                </div>
              )}
            </div>
          )}
        </Card>
      )}

      {/* Sync-Zeitplan (Timer Management) */}
      <Card className={`mb-4 border ${timerConfig?.enabled ? "border-emerald-500/20" : "border-zinc-700"}`}>
        <button
          className="w-full flex items-center gap-2 text-left"
          onClick={() => setTimerOpen((o) => !o)}
        >
          <Clock className={`w-5 h-5 ${timerConfig?.enabled ? "text-emerald-400" : "text-zinc-500"}`} />
          <span className="font-semibold flex-1">Sync-Zeitplan</span>
          <span className="text-xs text-zinc-500">
            {timerConfig?.enabled
              ? `${timerConfig.calendar || "daily"}`
              : timerInstalled
                ? "Deaktiviert"
                : "Nicht installiert"}
          </span>
          {timerConfig?.enabled && (
            <Badge color="green">Aktiv</Badge>
          )}
          {timerOpen ? (
            <ChevronUp className="w-4 h-4 text-zinc-500" />
          ) : (
            <ChevronDown className="w-4 h-4 text-zinc-500" />
          )}
        </button>

        {timerOpen && (
          <div className="mt-4 space-y-4">
            {/* Timer message */}
            {timerMsg && (
              <div className={`p-2.5 rounded-lg text-xs font-mono whitespace-pre-wrap ${
                timerMsg.ok
                  ? "bg-emerald-500/10 text-emerald-400 border border-emerald-500/20"
                  : "bg-red-500/10 text-red-400 border border-red-500/20"
              }`}>
                {timerMsg.text}
              </div>
            )}

            {/* Install / Config */}
            <div className="grid grid-cols-2 gap-4">
              <div className="space-y-3">
                <div>
                  <label className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1 block">Intervall (OnCalendar)</label>
                  <input
                    type="text"
                    value={calendar}
                    onChange={(e) => setCalendar(e.target.value)}
                    className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:border-cyan-500"
                    placeholder="daily, weekly, *-*-* 01:00:00"
                  />
                </div>
                <div>
                  <label className="text-[10px] text-zinc-500 uppercase tracking-wider mb-1 block">Zufällige Verzögerung</label>
                  <input
                    type="text"
                    value={delay}
                    onChange={(e) => setDelay(e.target.value)}
                    className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm font-mono focus:outline-none focus:border-cyan-500"
                    placeholder="1h, 30min, 0"
                  />
                </div>
                <div className="flex gap-2">
                  <Button size="sm" onClick={handleInstallTimer} loading={installing}>
                    <Download className="w-3.5 h-3.5" />
                    {timerInstalled ? "Aktualisieren" : "Installieren"}
                  </Button>
                  {timerInstalled && (
                    <>
                      <Button size="sm" variant={timerConfig?.enabled ? "danger" : "primary"} onClick={toggleTimer} loading={toggling}>
                        <Power className="w-3.5 h-3.5" />
                        {timerConfig?.enabled ? "Deaktivieren" : "Aktivieren"}
                      </Button>
                      <Button size="sm" variant="danger" onClick={handleUninstallTimer} loading={uninstalling}>
                        <Trash2 className="w-3.5 h-3.5" />
                      </Button>
                    </>
                  )}
                </div>
              </div>

              {/* Timer details */}
              {timerInstalled && timerConfig && (
                <div className="space-y-2.5">
                  <div className="flex items-center justify-between bg-zinc-900/60 rounded-lg px-3 py-2">
                    <span className="text-xs text-zinc-500">Intervall</span>
                    <span className="text-xs font-mono text-zinc-300">{timerConfig.calendar || "—"}</span>
                  </div>
                  <div className="flex items-center justify-between bg-zinc-900/60 rounded-lg px-3 py-2">
                    <span className="text-xs text-zinc-500">Verzögerung</span>
                    <span className="text-xs font-mono text-zinc-300">{timerConfig.randomized_delay || "—"}</span>
                  </div>
                  <div className="flex items-center justify-between bg-zinc-900/60 rounded-lg px-3 py-2">
                    <span className="text-xs text-zinc-500">Letzter Trigger</span>
                    <span className="text-xs font-mono text-zinc-300">{timerConfig.last_trigger || "—"}</span>
                  </div>
                  <div className="flex items-center justify-between bg-zinc-900/60 rounded-lg px-3 py-2">
                    <span className="text-xs text-zinc-500">Ergebnis</span>
                    <Badge color={timerConfig.service_result === "success" ? "green" : timerConfig.service_result ? "red" : "zinc"}>
                      {timerConfig.service_result || "—"}
                    </Badge>
                  </div>
                </div>
              )}
            </div>
          </div>
        )}
      </Card>

      {/* Verify Result */}
      {(verifyOpen || verifyResult) && (
        <Card
          className={`mb-4 border ${
            verifyResult
              ? verifyResult.overall_ok
                ? "border-emerald-500/30"
                : "border-amber-500/30"
              : "border-zinc-700"
          }`}
        >
          <button
            className="w-full flex items-center gap-2 text-left"
            onClick={() => setVerifyOpen((o) => !o)}
          >
            <ShieldCheck
              className={`w-5 h-5 ${
                verifyResult
                  ? verifyResult.overall_ok
                    ? "text-emerald-400"
                    : "text-amber-400"
                  : "text-zinc-500"
              }`}
            />
            <span className="font-semibold flex-1">
              {verifying
                ? "Backup wird geprüft..."
                : verifyResult
                ? verifyResult.overall_ok
                  ? "Backup verifiziert ✓"
                  : "Backup-Probleme gefunden"
                : "Backup-Verifikation"}
            </span>
            {verifyResult?.backup_dev && (
              <span className="text-xs text-zinc-500 font-mono">{verifyResult.backup_dev}</span>
            )}
            {verifyOpen ? (
              <ChevronUp className="w-4 h-4 text-zinc-500" />
            ) : (
              <ChevronDown className="w-4 h-4 text-zinc-500" />
            )}
          </button>

          {verifyOpen && verifyResult && (
            <div className="mt-4 space-y-2">
              {verifyResult.checks.map((check, i) => (
                <div key={i} className="flex items-start gap-3">
                  {check.ok ? (
                    <CheckCircle2 className="w-4 h-4 text-emerald-400 mt-0.5 shrink-0" />
                  ) : (
                    <XCircle className="w-4 h-4 text-red-400 mt-0.5 shrink-0" />
                  )}
                  <div>
                    <p className="text-sm font-medium text-zinc-300">{check.name}</p>
                    <p className="text-xs text-zinc-500">{check.detail}</p>
                  </div>
                </div>
              ))}
            </div>
          )}
        </Card>
      )}

      {/* Sync Result */}
      {syncResult && (
        <Card
          className={`mb-4 border ${
            syncResult.success ? "border-emerald-500/30" : "border-red-500/30"
          }`}
        >
          <div className="flex items-center gap-2 mb-2">
            {syncResult.success ? (
              <CheckCircle2 className="w-5 h-5 text-emerald-400" />
            ) : (
              <XCircle className="w-5 h-5 text-red-400" />
            )}
            <span className="font-semibold">
              {syncResult.success ? "Sync erfolgreich" : "Sync fehlgeschlagen"}
            </span>
          </div>
          {syncResult.stderr && (
            <pre className="text-xs font-mono text-red-400 mt-2 whitespace-pre-wrap">
              {syncResult.stderr}
            </pre>
          )}
        </Card>
      )}

      {/* Log View */}
      <Card>
        <h3 className="text-sm font-semibold text-zinc-400 mb-3">Sync Log</h3>
        <div className="bg-zinc-950 rounded-lg p-4 max-h-96 overflow-y-auto font-mono text-xs">
          {logs.length === 0 ? (
            <span className="text-zinc-600">Kein Log vorhanden</span>
          ) : (
            logs.map((line, i) => (
              <div
                key={i}
                className={`py-0.5 ${
                  line.includes("FEHLER")
                    ? "text-red-400"
                    : line.includes("===")
                      ? "text-cyan-400 font-semibold"
                      : line.includes("synchronisiert")
                        ? "text-emerald-400"
                        : "text-zinc-500"
                }`}
              >
                {line}
              </div>
            ))
          )}
          <div ref={logEndRef} />
        </div>
      </Card>
    </div>
  );
}

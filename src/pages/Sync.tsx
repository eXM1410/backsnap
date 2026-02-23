import { useEffect, useState, useRef } from "react";
import {
  RefreshCw,
  Play,
  CheckCircle2,
  XCircle,
  ArrowRight,
  Loader2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api, SyncStatus, CommandResult } from "../api";
import { Card, Button, Badge, PageHeader, Loading } from "../components/ui";

interface SyncProgress {
  step: string;
  detail: string;
  percent?: number;
}

const STEP_LABELS: Record<string, string> = {
  init: "Initialisierung",
  system: "System (/)",
  home: "Home",
  boot: "Boot",
  done: "Fertig",
};

export default function Sync() {
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [logs, setLogs] = useState<string[]>([]);
  const [syncing, setSyncing] = useState(false);
  const [syncResult, setSyncResult] = useState<CommandResult | null>(null);
  const [loading, setLoading] = useState(true);
  const [progress, setProgress] = useState<SyncProgress | null>(null);
  const logEndRef = useRef<HTMLDivElement>(null);

  const refresh = async () => {
    try {
      const s = await api.getSyncStatus();
      setStatus(s);
      const l = await api.getSyncLog();
      setLogs(l);
    } catch (e) {
      console.error(e);
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
        refresh();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [logs]);

  const handleSync = async () => {
    setSyncing(true);
    setSyncResult(null);
    setProgress(null);
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
  };

  if (loading) return <div className="p-8"><Loading /></div>;

  return (
    <div className="p-8">
      <PageHeader
        title="NVMe Sync"
        description="Systemdaten zwischen Samsung und XPG synchronisieren"
        actions={
          <Button onClick={handleSync} loading={syncing} disabled={syncing}>
            <Play className="w-4 h-4" />
            {syncing ? "Sync läuft..." : "Jetzt synchronisieren"}
          </Button>
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
          </div>
          <p className="text-sm text-zinc-400">{progress.detail}</p>
          <div className="flex gap-2 mt-3">
            {["init", "system", "home", "boot", "done"].map((step) => (
              <div
                key={step}
                className={`h-1.5 flex-1 rounded-full ${
                  step === progress.step
                    ? "bg-cyan-400 animate-pulse"
                    : ["init", "system", "home", "boot", "done"].indexOf(step) <
                        ["init", "system", "home", "boot", "done"].indexOf(progress.step)
                      ? "bg-emerald-500"
                      : "bg-zinc-700"
                }`}
              />
            ))}
          </div>
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
              {status?.direction.split("->")[0]?.trim()}
            </span>
            <ArrowRight className="w-4 h-4 text-zinc-600" />
            <span className="text-zinc-400">
              {status?.direction.split("->")[1]?.trim()}
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

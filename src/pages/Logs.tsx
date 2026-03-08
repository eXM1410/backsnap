import { useEffect, useState, useRef } from "react";
import { Terminal, RefreshCw, Download } from "lucide-react";
import { api } from "../api";
import { Card, Button, PageHeader, Loading } from "../components/ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export default function Logs() {
  const [logs, setLogs] = useState<string[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [autoScroll, setAutoScroll] = useState(true);
  const logEndRef = useRef<HTMLDivElement>(null);
  const unlistenRef = useRef<UnlistenFn | null>(null);
  const lastRefreshRef = useRef<string[]>([]);

  const refresh = async () => {
    try {
      const l = await api.getActivityLog();
      lastRefreshRef.current = l;
      setLogs(l);
      setError("");
    } catch (e) {
      console.error(e);
      setError(String(e));
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();

    const setupLive = async () => {
      const unlisten = await listen<string>("activity-log-line", (event) => {
        setLogs((prev) => {
          // Cap at 5000 lines to prevent unbounded growth
          const next = [...prev, event.payload];
          return next.length > 5000 ? next.slice(next.length - 5000) : next;
        });
      });
      unlistenRef.current = unlisten;
    };

    setupLive();

    // Live lines arrive via 'activity-log-line' events. Poll only as a fallback every 30s.
    const interval = setInterval(refresh, 30000);
    return () => {
      clearInterval(interval);
      if (unlistenRef.current) {
        unlistenRef.current();
      }
    };
  }, []);

  useEffect(() => {
    if (autoScroll) {
      logEndRef.current?.scrollIntoView({ behavior: "smooth" });
    }
  }, [logs, autoScroll]);

  const getLineColor = (line: string) => {
    if (line.includes("FEHLER") || line.includes("ERROR")) return "text-red-400";
    if (line.includes("===")) return "text-cyan-400 font-semibold";
    if (line.includes("synchronisiert") || line.includes("fertig"))
      return "text-emerald-400";
    if (line.includes("Mounted") || line.includes("Sync /"))
      return "text-amber-400";
    return "text-zinc-500";
  };

  if (loading) return <div className="p-8"><Loading /></div>;
  if (error && logs.length === 0) return (
    <div className="p-8">
      <PageHeader title="Logs" description="Live Aktivitäts-Log (Echtzeit + Datei)" />
      <Card className="p-6 text-red-400 text-sm">{error}</Card>
    </div>
  );

  return (
    <div className="p-8">
      <PageHeader
        title="Logs"
        description="Live Aktivitäts-Log (Echtzeit + Datei)"
        actions={
          <div className="flex items-center gap-2">
            <label className="flex items-center gap-2 text-sm text-zinc-500 cursor-pointer">
              <input
                type="checkbox"
                checked={autoScroll}
                onChange={(e) => setAutoScroll(e.target.checked)}
                className="accent-cyan-500"
              />
              Auto-Scroll
            </label>
            <Button variant="secondary" size="sm" onClick={refresh}>
              <RefreshCw className="w-3.5 h-3.5" /> Aktualisieren
            </Button>
          </div>
        }
      />

      <Card className="p-0 overflow-hidden">
        <div className="bg-zinc-950 p-4 max-h-[calc(100vh-200px)] overflow-y-auto font-mono text-xs leading-relaxed">
          {logs.length === 0 ? (
            <div className="text-zinc-600 py-8 text-center">
              <Terminal className="w-8 h-8 mx-auto mb-2 opacity-30" />
              Keine Log-Einträge vorhanden
            </div>
          ) : (
            <>
              {logs.map((line, i) => (
                <div
                  key={i}
                  className={`py-0.5 ${getLineColor(line)} hover:bg-zinc-900/50`}
                >
                  <span className="text-zinc-700 select-none mr-3">
                    {String(i + 1).padStart(4)}
                  </span>
                  {line}
                </div>
              ))}
              <div ref={logEndRef} />
            </>
          )}
        </div>
      </Card>

      <div className="mt-3 text-xs text-zinc-600 flex items-center justify-between">
        <span>{logs.length} Zeilen</span>
        <span>~/.local/share/arclight/activity.log</span>
      </div>
    </div>
  );
}

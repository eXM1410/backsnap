import React, { useEffect, useState } from "react";
import {
  Camera,
  Plus,
  Trash2,
  RotateCcw,
  FileText,
  AlertTriangle,
  ChevronDown,
  Loader2,
} from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api, Snapshot } from "../api";
import {
  Card,
  Button,
  Badge,
  PageHeader,
  Loading,
  EmptyState,
} from "../components/ui";

export default function Snapshots() {
  const [config, setConfig] = useState<string>("root");
  const [configs, setConfigs] = useState<string[]>([]);
  const [snapshots, setSnapshots] = useState<Snapshot[]>([]);
  const [loading, setLoading] = useState(true);
  const [creating, setCreating] = useState(false);
  const [newDesc, setNewDesc] = useState("");
  const [showCreate, setShowCreate] = useState(false);
  const [diffId, setDiffId] = useState<number | null>(null);
  const [diffContent, setDiffContent] = useState<string>("");
  const [confirmRollback, setConfirmRollback] = useState<number | null>(null);
  const [rollbackRunning, setRollbackRunning] = useState(false);
  const [rollbackProgress, setRollbackProgress] = useState<string>("");
  const [rollbackResult, setRollbackResult] = useState<string>("");

  const loadSnapshots = async () => {
    setLoading(true);
    try {
      const status = await api.getSystemStatus();
      setConfigs(status.snapper_configs);
      const snaps = await api.getSnapshots(config);
      setSnapshots(snaps);
    } catch (e) {
      console.error(e);
    }
    setLoading(false);
  };

  useEffect(() => {
    loadSnapshots();
  }, [config]);

  // Listen for rollback progress events
  useEffect(() => {
    const unlisten = listen<{ step: string; detail: string }>(
      "sync-progress",
      (event) => {
        setRollbackProgress(event.payload.detail);
      }
    );
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  const handleCreate = async () => {
    if (!newDesc.trim()) return;
    setCreating(true);
    await api.createSnapshot(config, newDesc.trim());
    setNewDesc("");
    setShowCreate(false);
    setCreating(false);
    loadSnapshots();
  };

  const handleDelete = async (id: number) => {
    await api.deleteSnapshot(config, id);
    loadSnapshots();
  };

  const handleDiff = async (id: number) => {
    if (diffId === id) {
      setDiffId(null);
      return;
    }
    try {
      const diff = await api.getSnapperDiff(config, id);
      setDiffContent(diff);
      setDiffId(id);
    } catch (e: any) {
      setDiffContent("Fehler: " + e.toString());
      setDiffId(id);
    }
  };

  const handleRollback = async (id: number) => {
    if (confirmRollback !== id) {
      setConfirmRollback(id);
      return;
    }
    setRollbackRunning(true);
    setRollbackProgress("");
    setRollbackResult("");
    try {
      const result = await api.rollbackSnapshot(config, id);
      setRollbackResult(result.success ? result.stdout : result.stderr);
    } catch (e: any) {
      setRollbackResult("Fehler: " + e.toString());
    }
    setRollbackRunning(false);
    setConfirmRollback(null);
    loadSnapshots();
  };

  return (
    <div className="p-8">
      <PageHeader
        title="Snapshots"
        description="Btrfs Snapshots verwalten — erstellen, vergleichen, zurückrollen"
        actions={
          <Button onClick={() => setShowCreate(!showCreate)}>
            <Plus className="w-4 h-4" /> Neuer Snapshot
          </Button>
        }
      />

      {/* Config Selector */}
      <div className="flex items-center gap-2 mb-4">
        {configs.map((c) => (
          <button
            key={c}
            onClick={() => setConfig(c)}
            className={`px-4 py-2 rounded-lg text-sm font-medium transition ${
              config === c
                ? "bg-cyan-500/15 text-cyan-400 border border-cyan-500/30"
                : "bg-zinc-900 text-zinc-400 border border-zinc-800 hover:bg-zinc-800"
            }`}
          >
            {c}
          </button>
        ))}
      </div>

      {/* Create Form */}
      {showCreate && (
        <Card className="mb-4">
          <div className="flex items-center gap-3">
            <input
              type="text"
              value={newDesc}
              onChange={(e) => setNewDesc(e.target.value)}
              placeholder="Beschreibung (z.B. 'vor System-Update')"
              className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-4 py-2 text-sm focus:outline-none focus:border-cyan-500 transition"
              onKeyDown={(e) => e.key === "Enter" && handleCreate()}
              autoFocus
            />
            <Button onClick={handleCreate} loading={creating}>
              Erstellen
            </Button>
            <Button
              variant="ghost"
              onClick={() => setShowCreate(false)}
            >
              Abbrechen
            </Button>
          </div>
        </Card>
      )}

      {/* Rollback Progress / Result */}
      {rollbackRunning && (
        <Card className="mb-4 border border-amber-500/30">
          <div className="flex items-center gap-3">
            <Loader2 className="w-5 h-5 text-amber-400 animate-spin" />
            <div>
              <span className="font-semibold text-amber-400">
                Rollback läuft...
              </span>
              {rollbackProgress && (
                <p className="text-xs text-zinc-400 mt-1">
                  {rollbackProgress}
                </p>
              )}
            </div>
          </div>
        </Card>
      )}
      {rollbackResult && !rollbackRunning && (
        <Card className="mb-4 border border-emerald-500/30">
          <div className="flex items-center justify-between mb-2">
            <span className="font-semibold text-emerald-400">
              Rollback Ergebnis
            </span>
            <button
              onClick={() => setRollbackResult("")}
              className="text-xs text-zinc-500 hover:text-zinc-300"
            >
              Schließen
            </button>
          </div>
          <pre className="text-xs font-mono text-zinc-300 whitespace-pre-wrap">
            {rollbackResult}
          </pre>
        </Card>
      )}

      {/* Snapshot List */}
      {loading ? (
        <Loading />
      ) : snapshots.length === 0 ? (
        <EmptyState
          icon={Camera}
          title="Keine Snapshots"
          description="Erstelle deinen ersten Snapshot"
        />
      ) : (
        <Card className="p-0 overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-left text-zinc-500 bg-zinc-900/80">
                <th className="px-4 py-3 font-medium">#</th>
                <th className="px-4 py-3 font-medium">Typ</th>
                <th className="px-4 py-3 font-medium">Datum</th>
                <th className="px-4 py-3 font-medium">User</th>
                <th className="px-4 py-3 font-medium">Beschreibung</th>
                <th className="px-4 py-3 font-medium text-right">Aktionen</th>
              </tr>
            </thead>
            <tbody>
              {snapshots.map((snap) => (
                <React.Fragment key={snap.id}>
                  <tr
                    key={snap.id}
                    className="border-t border-zinc-800/50 hover:bg-zinc-800/30 transition"
                  >
                    <td className="px-4 py-3 font-mono text-cyan-400">
                      {snap.id}
                    </td>
                    <td className="px-4 py-3">
                      <Badge
                        color={
                          snap.snap_type === "pre"
                            ? "yellow"
                            : snap.snap_type === "post"
                              ? "green"
                              : "zinc"
                        }
                      >
                        {snap.snap_type}
                      </Badge>
                    </td>
                    <td className="px-4 py-3 text-zinc-400">{snap.date}</td>
                    <td className="px-4 py-3 text-zinc-500">{snap.user}</td>
                    <td className="px-4 py-3">{snap.description}</td>
                    <td className="px-4 py-3 text-right">
                      <div className="flex items-center justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDiff(snap.id)}
                        >
                          <FileText className="w-3.5 h-3.5" />
                        </Button>
                        {config === "root" && (
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => handleRollback(snap.id)}
                          >
                            <RotateCcw className="w-3.5 h-3.5" />
                          </Button>
                        )}
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleDelete(snap.id)}
                        >
                          <Trash2 className="w-3.5 h-3.5 text-red-400" />
                        </Button>
                      </div>
                    </td>
                  </tr>
                  {/* Rollback Confirm */}
                  {confirmRollback === snap.id && (
                    <tr key={`rollback-${snap.id}`}>
                      <td colSpan={6} className="px-4 py-3 bg-red-500/5">
                        <div className="flex items-center gap-3">
                          <AlertTriangle className="w-4 h-4 text-red-400" />
                          <span className="text-red-400 text-sm">
                            Wirklich zu Snapshot #{snap.id} zurückrollen? Neustart
                            erforderlich!
                          </span>
                          <Button
                            variant="danger"
                            size="sm"
                            onClick={() => handleRollback(snap.id)}
                          >
                            Ja, Rollback
                          </Button>
                          <Button
                            variant="ghost"
                            size="sm"
                            onClick={() => setConfirmRollback(null)}
                          >
                            Abbrechen
                          </Button>
                        </div>
                      </td>
                    </tr>
                  )}
                  {/* Diff View */}
                  {diffId === snap.id && (
                    <tr key={`diff-${snap.id}`}>
                      <td colSpan={6} className="px-4 py-3 bg-zinc-900">
                        <pre className="text-xs font-mono text-zinc-400 max-h-60 overflow-y-auto whitespace-pre-wrap">
                          {diffContent || "Keine Änderungen"}
                        </pre>
                      </td>
                    </tr>
                  )}
                </React.Fragment>
              ))}
            </tbody>
          </table>
        </Card>
      )}
    </div>
  );
}

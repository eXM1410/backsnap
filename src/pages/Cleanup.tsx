import React, { useEffect, useState, useRef, useCallback, useMemo } from "react";
import {
  Trash2,
  Search,
  CheckCircle2,
  XCircle,
  Loader2,
  FolderOpen,
  Shield,
  AlertTriangle,
  RefreshCw,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  ArrowUpDown,
  File,
} from "lucide-react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, CleanupItem, DeleteResult, DirEntry } from "../api";
import { Card, Button, Badge, PageHeader, Loading, EmptyState } from "../components/ui";

// ─── Global State for Tab Persistence ─────────────────────────
let globalItems: CleanupItem[] = [];
let globalSelected: Set<string> = new Set();
let globalScanned = false;
let globalScanning = false;
let globalScanPhase = "";
let globalCurrentPath = "";
let globalAiAssist = true;

// ─── Helpers ──────────────────────────────────────────────────

function categoryLabel(cat: string): string {
  const map: Record<string, string> = {
    Cache: "Cache",
    BuildArtifact: "Build-Artefakt",
    Toolchain: "Toolchain",
    Gaming: "Gaming",
    Container: "Container",
    VirtualMachine: "VM",
    Runtime: "Runtime",
    Media: "Medien",
    Browser: "Browser",
    Communication: "Kommunikation",
    LargeUnknown: "Groß/Unbekannt",
  };
  return map[cat] || cat;
}

type BadgeColor = "cyan" | "green" | "red" | "yellow" | "zinc" | "amber" | "purple" | "blue" | "emerald";

function categoryColor(cat: string): BadgeColor {
  const map: Record<string, BadgeColor> = {
    Cache: "cyan",
    BuildArtifact: "amber",
    Toolchain: "purple",
    Gaming: "green",
    Container: "blue",
    VirtualMachine: "red",
    Runtime: "purple",
    Media: "emerald",
    Browser: "cyan",
    Communication: "blue",
    LargeUnknown: "zinc",
  };
  return map[cat] || "zinc";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1_099_511_627_776) return `${(bytes / 1_099_511_627_776).toFixed(1)} TB`;
  if (bytes >= 1_073_741_824) return `${(bytes / 1_073_741_824).toFixed(1)} GB`;
  if (bytes >= 1_048_576) return `${(bytes / 1_048_576).toFixed(1)} MB`;
  if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${bytes} B`;
}

// ─── SubPathViewer Component ──────────────────────────────────

function SubPathViewer({
  parentPath,
  selected,
  setSelected,
}: {
  parentPath: string;
  selected: Set<string>;
  setSelected: React.Dispatch<React.SetStateAction<Set<string>>>;
}) {
  const [entries, setEntries] = useState<DirEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const [isDragging, setIsDragging] = useState(false);
  const [dragMode, setDragMode] = useState<"select" | "deselect">("select");

  useEffect(() => {
    api
      .getCleanupDirContents(parentPath)
      .then((res) => {
        setEntries(res);
        setLoading(false);
      })
      .catch((e) => {
        setError(String(e));
        setLoading(false);
      });
  }, [parentPath]);

  const handleMouseDown = (path: string, currentlySelected: boolean) => {
    setIsDragging(true);
    const newMode = currentlySelected ? "deselect" : "select";
    setDragMode(newMode);
    toggleItem(path, newMode);
  };

  const handleMouseEnter = (path: string) => {
    if (isDragging) {
      toggleItem(path, dragMode);
    }
  };

  const handleMouseUp = () => {
    setIsDragging(false);
  };

  useEffect(() => {
    window.addEventListener("mouseup", handleMouseUp);
    return () => window.removeEventListener("mouseup", handleMouseUp);
  }, []);

  const toggleItem = (path: string, mode: "select" | "deselect") => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (mode === "select") next.add(path);
      else next.delete(path);
      return next;
    });
  };

  if (loading) {
    return (
      <div className="p-4 flex items-center justify-center text-zinc-500">
        <Loader2 className="w-4 h-4 animate-spin mr-2" /> Lade Inhalte...
      </div>
    );
  }

  if (error) {
    return <div className="p-4 text-red-400 text-xs">Fehler: {error}</div>;
  }

  if (entries.length === 0) {
    return <div className="p-4 text-zinc-500 text-xs text-center">Ordner ist leer</div>;
  }

  return (
    <div className="p-4 bg-zinc-900/50 border-t border-zinc-800/50 select-none">
      <div className="text-xs text-zinc-500 mb-3 flex items-center justify-between">
        <span>Inhalte von {parentPath} (Klicken & Ziehen zum Markieren)</span>
        <button
          onClick={() => {
            setSelected((prev) => {
              const next = new Set(prev);
              const allSelected = entries.every((e) => next.has(e.path));
              entries.forEach((e) => {
                if (allSelected) next.delete(e.path);
                else next.add(e.path);
              });
              return next;
            });
          }}
          className="text-cyan-400 hover:text-cyan-300"
        >
          Alle umschalten
        </button>
      </div>
      <div className="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 gap-2">
        {entries.map((entry) => {
          const isSelected = selected.has(entry.path);
          return (
            <div
              key={entry.path}
              onMouseDown={() => handleMouseDown(entry.path, isSelected)}
              onMouseEnter={() => handleMouseEnter(entry.path)}
              className={`flex flex-col items-center justify-center p-3 rounded-lg border transition-colors cursor-pointer ${
                isSelected
                  ? "bg-cyan-500/20 border-cyan-500/50 text-cyan-100"
                  : "bg-zinc-800/50 border-zinc-700/50 text-zinc-400 hover:bg-zinc-700/50 hover:border-zinc-600"
              }`}
            >
              {entry.is_dir ? (
                <FolderOpen className={`w-6 h-6 mb-2 ${isSelected ? "text-cyan-400" : "text-zinc-500"}`} />
              ) : (
                <File className={`w-6 h-6 mb-2 ${isSelected ? "text-cyan-400" : "text-zinc-500"}`} />
              )}
              <span className="text-xs font-medium truncate w-full text-center" title={entry.name}>
                {entry.name}
              </span>
              <span className="text-[10px] opacity-70 mt-1">{formatBytes(entry.size_bytes)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ─── Component ────────────────────────────────────────────────

export default function Cleanup() {
  const [items, setItems] = useState<CleanupItem[]>(globalItems);
  const [scanning, setScanning] = useState(globalScanning);
  const [deleting, setDeleting] = useState(false);
  const [selected, setSelected] = useState<Set<string>>(globalSelected);
  const [deleteResults, setDeleteResults] = useState<DeleteResult[]>([]);
  const [filter, setFilter] = useState<string>("all");
  const [error, setError] = useState("");
  const [scanned, setScanned] = useState(globalScanned);
  const [scanPhase, setScanPhase] = useState<string>(globalScanPhase);
  const [currentPath, setCurrentPath] = useState<string>(globalCurrentPath);
  const [aiAssist, setAiAssist] = useState<boolean>(globalAiAssist);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);
  const [sizeSort, setSizeSort] = useState<"none" | "asc" | "desc">("none");
  const unlistenRef = useRef<UnlistenFn[]>([]);
  const scanPromiseRef = useRef<Promise<CleanupItem[]> | null>(null);

  // Persist state to globals
  useEffect(() => {
    globalItems = items;
    globalSelected = selected;
    globalScanned = scanned;
    globalScanning = scanning;
    globalScanPhase = scanPhase;
    globalCurrentPath = currentPath;
    globalAiAssist = aiAssist;
  }, [items, selected, scanned, scanning, scanPhase, currentPath, aiAssist]);

  const cleanup = useCallback(() => {
    unlistenRef.current.forEach((fn) => fn());
    unlistenRef.current = [];
  }, []);

  // We don't want to unlisten when the component unmounts if a scan is running,
  // so we only cleanup on unmount if NOT scanning.
  useEffect(() => {
    return () => {
      if (!scanning) {
        cleanup();
      }
    };
  }, [cleanup, scanning]);

  // Re-attach streaming listeners if we mount while a scan is running.
  // Completion is handled by scanPromiseRef — no done-event needed.
  useEffect(() => {
    let isMounted = true;
    let unBatch: UnlistenFn | undefined;
    let unPhase: UnlistenFn | undefined;
    let unProgress: UnlistenFn | undefined;

    if (scanning && unlistenRef.current.length === 0) {
      const attach = async () => {
        unBatch = await listen<CleanupItem[]>("cleanup-item-batch", (event) => {
          if (!isMounted) return;
          setItems((prev) => {
            const newItems = event.payload.filter(
              (newItem) => !prev.some((p) => p.path === newItem.path)
            );
            return [...prev, ...newItems];
          });
        });
        unPhase = await listen<{ phase: number; label: string }>("cleanup-phase", (event) => {
          if (!isMounted) return;
          setScanPhase(event.payload.label);
        });
        unProgress = await listen<string>("cleanup-progress", (event) => {
          if (!isMounted) return;
          setCurrentPath(event.payload);
        });
        if (isMounted) {
          unlistenRef.current = [unBatch, unPhase, unProgress];
        } else {
          if (unBatch) unBatch();
          if (unPhase) unPhase();
          if (unProgress) unProgress();
        }
        // Wait for the invoke promise (survives remounts)
        if (scanPromiseRef.current && isMounted) {
          try {
            const finalItems = await scanPromiseRef.current;
            if (!isMounted) return;
            setItems(finalItems);
            setScanning(false);
            setScanned(true);
            setScanPhase("");
            setCurrentPath("");
            const safeSet = new Set(finalItems.filter((r) => r.safe).map((r) => r.path));
            setSelected(safeSet);
            // Sync globals directly — survives unmount
            globalItems = finalItems;
            globalSelected = safeSet;
            globalScanning = false;
            globalScanned = true;
            globalScanPhase = "";
            globalCurrentPath = "";
          } catch (e) {
            if (!isMounted) return;
            setError(String(e));
            setScanning(false);
            globalScanning = false;
          } finally {
            cleanup();
            scanPromiseRef.current = null;
          }
        }
      };
      attach();
    }
    return () => {
      isMounted = false;
    };
  }, [scanning]);

  const handleCancel = useCallback(async () => {
    try {
      await api.cancelScan();
      setScanning(false);
      setScanPhase("Abgebrochen");
      setCurrentPath("");
    } catch (e) {
      console.error("Failed to cancel scan:", e);
    }
  }, []);

  const handleScan = useCallback(async () => {
    setScanning(true);
    setItems([]);
    setSelected(new Set());
    setDeleteResults([]);
    setError("");
    setScanned(false);
    setScanPhase("");
    setCurrentPath("");
    setExpandedPath(null);
    cleanup();

    // Register streaming listeners for live progress
    const unBatch = await listen<CleanupItem[]>("cleanup-item-batch", (event) => {
      setItems((prev) => {
        const newItems = event.payload.filter(
          (newItem) => !prev.some((p) => p.path === newItem.path)
        );
        return [...prev, ...newItems];
      });
    });
    const unPhase = await listen<{ phase: number; label: string }>("cleanup-phase", (event) => {
      setScanPhase(event.payload.label);
    });
    const unProgress = await listen<string>("cleanup-progress", (event) => {
      setCurrentPath(event.payload);
    });
    unlistenRef.current = [unBatch, unPhase, unProgress];

    // The invoke Promise IS the completion signal — not an event.
    // Its return value is the final, sorted, complete item list.
    const promise = api.scanCleanup(aiAssist);
    scanPromiseRef.current = promise;

    try {
      const finalItems = await promise;
      // Replace streamed items with the authoritative sorted list
      setItems(finalItems);
      setScanning(false);
      setScanned(true);
      setScanPhase("");
      setCurrentPath("");
      const safeSet = new Set(finalItems.filter((r) => r.safe).map((r) => r.path));
      setSelected(safeSet);
      // Sync globals directly — survives unmount during tab switch
      globalItems = finalItems;
      globalSelected = safeSet;
      globalScanning = false;
      globalScanned = true;
      globalScanPhase = "";
      globalCurrentPath = "";
    } catch (e) {
      setError(String(e));
      setScanning(false);
      globalScanning = false;
    } finally {
      cleanup();
      scanPromiseRef.current = null;
    }
  }, [cleanup, aiAssist]);

  const handleDelete = useCallback(async () => {
    if (selected.size === 0) return;

    // Calculate total size of selection for safety confirmation
    const selectedItems = items.filter((i) => selected.has(i.path));
    const totalBytes = selectedItems.reduce((sum, i) => sum + i.size_bytes, 0);
    const unsafeCount = selectedItems.filter((i) => !i.safe).length;
    const totalGB = (totalBytes / (1024 * 1024 * 1024)).toFixed(1);

    // Confirm large deletions (>10 GB) or any unsafe items
    if (totalBytes > 10 * 1024 * 1024 * 1024 || unsafeCount > 0) {
      const msg = unsafeCount > 0
        ? `${selected.size} Einträge (${totalGB} GB) löschen? ${unsafeCount} davon sind NICHT als sicher markiert!`
        : `${selected.size} Einträge (${totalGB} GB) wirklich löschen?`;
      if (!window.confirm(msg)) return;
    }

    setDeleting(true);
    setDeleteResults([]);
    setError("");
    try {
      const results = await api.deleteCleanupPaths(Array.from(selected));
      setDeleteResults(results);
      // Remove successfully deleted items from the list
      const deleted = new Set(results.filter((r) => r.success).map((r) => r.path));
      setItems((prev) => {
        const next = prev.filter((item) => !deleted.has(item.path));
        globalItems = next; // Survive unmount
        return next;
      });
      setSelected((prev) => {
        const next = new Set(prev);
        deleted.forEach((p) => next.delete(p));
        globalSelected = next; // Survive unmount
        return next;
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setDeleting(false);
    }
  }, [selected, items]);

  const toggleSelect = (path: string) => {
    setSelected((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  };

  const selectAllSafe = () => {
    setSelected(new Set(items.filter((i) => i.safe).map((i) => i.path)));
  };

  const selectAll = () => {
    setSelected(new Set(items.map((i) => i.path)));
  };

  const deselectAll = () => {
    setSelected(new Set());
  };

  // ── Filter logic ──
  const categories = useMemo(() => [...new Set(items.map((i) => i.category))].sort(), [items]);
  
  const filtered = useMemo(() => {
    let result = items;
    if (filter === "safe") result = items.filter((i) => i.safe);
    else if (filter !== "all") result = items.filter((i) => i.category === filter);
    if (sizeSort === "asc") result = [...result].sort((a, b) => a.size_bytes - b.size_bytes);
    else if (sizeSort === "desc") result = [...result].sort((a, b) => b.size_bytes - a.size_bytes);
    return result;
  }, [items, filter, sizeSort]);

  const selectedSize = useMemo(() => {
    return items
      .filter((i) => selected.has(i.path))
      .reduce((sum, i) => sum + i.size_bytes, 0);
  }, [items, selected]);

  const totalSize = useMemo(() => {
    return items.reduce((sum, i) => sum + i.size_bytes, 0);
  }, [items]);

  // ── Render ──
  return (
    <div className="p-8">
      <PageHeader
        title="Aufräumen"
        description="Caches, Build-Artefakte und unnötige Dateien finden und löschen"
        actions={
          <div className="flex items-center gap-2">
            {scanned && items.length > 0 && (
              <Button
                variant="danger"
                onClick={handleDelete}
                loading={deleting}
                disabled={selected.size === 0}
              >
                <Trash2 className="w-4 h-4" />
                {selected.size} löschen ({formatBytes(selectedSize)})
              </Button>
            )}
            {scanning ? (
              <Button variant="danger" onClick={handleCancel}>
                <XCircle className="w-4 h-4" />
                Abbrechen
              </Button>
            ) : (
              <Button onClick={handleScan} loading={scanning}>
                <Search className="w-4 h-4" />
                {scanned ? "Erneut scannen" : "Scan starten"}
              </Button>
            )}
            {!scanning && (
              <label className="ml-2 inline-flex items-center gap-2 text-xs text-zinc-400 select-none">
                <input
                  type="checkbox"
                  checked={aiAssist}
                  onChange={(e) => setAiAssist(e.target.checked)}
                  className="accent-cyan-500"
                />
                KI-Assistent
              </label>
            )}
          </div>
        }
      />

      {/* Error */}
      {error && (
        <Card className="mb-4 border border-red-500/30 bg-red-500/5">
          <div className="flex items-center justify-between">
            <span className="text-sm text-red-400">{error}</span>
            <button onClick={() => setError("")} className="text-xs text-zinc-500 hover:text-zinc-300">
              Schließen
            </button>
          </div>
        </Card>
      )}

      {/* Delete Results */}
      {deleteResults.length > 0 && (
        <Card className="mb-4 border border-emerald-500/20 bg-emerald-500/5">
          <div className="flex items-center gap-2 mb-2">
            <CheckCircle2 className="w-4 h-4 text-emerald-400" />
            <span className="text-sm font-semibold text-emerald-400">
              {deleteResults.filter((r) => r.success).length} gelöscht
              {deleteResults.some((r) => !r.success) && (
                <span className="text-red-400 ml-2">
                  · {deleteResults.filter((r) => !r.success).length} fehlgeschlagen
                </span>
              )}
            </span>
            <button onClick={() => setDeleteResults([])} className="ml-auto text-xs text-zinc-500 hover:text-zinc-300">
              Schließen
            </button>
          </div>
          {deleteResults.filter((r) => !r.success).map((r) => (
            <div key={r.path} className="text-xs text-red-400/80 ml-6">
              {r.path}: {r.error}
            </div>
          ))}
        </Card>
      )}

      {/* Not scanned yet */}
      {!scanned && !scanning && (
        <EmptyState
          icon={FolderOpen}
          title="Noch nicht gescannt"
          description="Klicke auf 'Scan starten' um löschbare Dateien zu finden"
        />
      )}

      {/* Scanning */}
      {scanning && (
        <Card className="mb-4">
          <div className="flex items-start gap-3">
            <Loader2 className="w-5 h-5 text-cyan-400 animate-spin mt-0.5" />
            <div className="flex-1 min-w-0">
              <div className="flex items-center justify-between">
                <span className="text-sm font-medium">
                  Scanne Dateisystem...
                  {scanPhase && <span className="text-cyan-400 ml-2 font-normal">({scanPhase})</span>}
                </span>
                <span className="text-xs text-zinc-400">
                  {items.length} Müll-Einträge gefunden ({formatBytes(totalSize)})
                </span>
              </div>
              {currentPath && (
                <div className="text-[10px] text-zinc-500 mt-1 truncate font-mono bg-zinc-900/50 px-2 py-1 rounded border border-zinc-800/50" title={currentPath}>
                  {currentPath}
                </div>
              )}
            </div>
          </div>
        </Card>
      )}

      {/* Results */}
      {scanned && items.length > 0 && (
        <>
          {/* Stats + Filters */}
          <div className="flex items-center justify-between mb-4">
            <div className="flex items-center gap-3 flex-wrap">
              <button
                onClick={() => setFilter("all")}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${
                  filter === "all"
                    ? "bg-cyan-500/15 text-cyan-400 border border-cyan-500/30"
                    : "bg-zinc-900 text-zinc-400 border border-zinc-800 hover:bg-zinc-800"
                }`}
              >
                Alle ({items.length})
              </button>
              <button
                onClick={() => setFilter("safe")}
                className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${
                  filter === "safe"
                    ? "bg-emerald-500/15 text-emerald-400 border border-emerald-500/30"
                    : "bg-zinc-900 text-zinc-400 border border-zinc-800 hover:bg-zinc-800"
                }`}
              >
                <span className="inline-flex items-center gap-1">
                  <Shield className="w-3 h-3" /> Sicher ({items.filter((i) => i.safe).length})
                </span>
              </button>
              {categories.map((cat) => (
                <button
                  key={cat}
                  onClick={() => setFilter(cat)}
                  className={`px-3 py-1.5 rounded-lg text-xs font-medium transition ${
                    filter === cat
                      ? "bg-zinc-700 text-zinc-200 border border-zinc-600"
                      : "bg-zinc-900 text-zinc-400 border border-zinc-800 hover:bg-zinc-800"
                  }`}
                >
                  {categoryLabel(cat)} ({items.filter((i) => i.category === cat).length})
                </button>
              ))}
            </div>

            <div className="flex items-center gap-2 shrink-0 ml-4">
              <button onClick={selectAllSafe} className="text-xs text-emerald-400 hover:text-emerald-300">
                Sichere wählen
              </button>
              <span className="text-zinc-700">·</span>
              <button onClick={selectAll} className="text-xs text-zinc-400 hover:text-zinc-300">
                Alle
              </button>
              <span className="text-zinc-700">·</span>
              <button onClick={deselectAll} className="text-xs text-zinc-400 hover:text-zinc-300">
                Keine
              </button>
            </div>
          </div>

          {/* Summary bar */}
          <Card className="mb-4 py-3">
            <div className="flex items-center justify-between text-sm">
              <span className="text-zinc-400">
                {selected.size} von {items.length} ausgewählt
              </span>
              <div className="flex items-center gap-4">
                <span className="text-zinc-500">
                  Gesamt: <span className="text-zinc-300 font-mono">{formatBytes(totalSize)}</span>
                </span>
                <span className="text-zinc-500">
                  Auswahl: <span className="text-cyan-400 font-mono font-bold">{formatBytes(selectedSize)}</span>
                </span>
              </div>
            </div>
          </Card>

          {/* Item list */}
          <Card className="p-0 overflow-hidden">
            <table className="w-full text-sm">
              <thead>
                <tr className="text-left text-zinc-500 bg-zinc-900/80">
                  <th className="px-4 py-3 w-10">
                    <input
                      type="checkbox"
                      checked={filtered.length > 0 && filtered.every((i) => selected.has(i.path))}
                      onChange={(e) => {
                        if (e.target.checked) {
                          setSelected((prev) => {
                            const next = new Set(prev);
                            filtered.forEach((i) => next.add(i.path));
                            return next;
                          });
                        } else {
                          setSelected((prev) => {
                            const next = new Set(prev);
                            filtered.forEach((i) => next.delete(i.path));
                            return next;
                          });
                        }
                      }}
                      className="accent-cyan-500"
                    />
                  </th>
                  <th className="px-4 py-3 font-medium">Pfad</th>
                  <th className="px-4 py-3 font-medium">Kategorie</th>
                  <th
                    className="px-4 py-3 font-medium cursor-pointer select-none hover:text-zinc-300 transition-colors"
                    onClick={() => setSizeSort((prev) => prev === "none" ? "desc" : prev === "desc" ? "asc" : "none")}
                  >
                    <span className="inline-flex items-center gap-1">
                      Größe
                      {sizeSort === "desc" ? (
                        <ChevronDown className="w-3.5 h-3.5 text-cyan-400" />
                      ) : sizeSort === "asc" ? (
                        <ChevronUp className="w-3.5 h-3.5 text-cyan-400" />
                      ) : (
                        <ArrowUpDown className="w-3 h-3 text-zinc-600" />
                      )}
                    </span>
                  </th>
                  <th className="px-4 py-3 font-medium">Sicher</th>
                </tr>
              </thead>
              <tbody>
                {filtered.map((item) => (
                  <React.Fragment key={item.path}>
                    <tr
                      onClick={() => setExpandedPath(expandedPath === item.path ? null : item.path)}
                      className={`border-t border-zinc-800/50 cursor-pointer transition ${
                        selected.has(item.path)
                          ? "bg-cyan-500/5 hover:bg-cyan-500/10"
                          : "hover:bg-zinc-800/30"
                      }`}
                    >
                      <td className="px-4 py-3">
                        <input
                          type="checkbox"
                          checked={selected.has(item.path)}
                          onChange={() => toggleSelect(item.path)}
                          onClick={(e) => e.stopPropagation()}
                          className="accent-cyan-500"
                        />
                      </td>
                      <td className="px-4 py-3">
                        <div className="font-mono text-xs text-zinc-300 flex items-center gap-2">
                          {expandedPath === item.path ? (
                            <ChevronDown className="w-3 h-3 text-zinc-500" />
                          ) : (
                            <ChevronRight className="w-3 h-3 text-zinc-500" />
                          )}
                          {item.path}
                        </div>
                        <div className="text-[10px] text-zinc-600 mt-0.5 ml-5">{item.reason}</div>
                        {item.ai_checked && (
                          <div className="ml-5 mt-1 flex items-center gap-2 text-[10px]">
                            <span className="px-1.5 py-0.5 rounded border border-fuchsia-500/30 bg-fuchsia-500/10 text-fuchsia-300">
                              KI {item.ai_confidence != null ? `${Math.round(item.ai_confidence * 100)}%` : "geprüft"}
                            </span>
                            {item.ai_note && (
                              <span className="text-zinc-500 truncate" title={item.ai_note}>{item.ai_note}</span>
                            )}
                          </div>
                        )}
                      </td>
                      <td className="px-4 py-3">
                        <Badge color={categoryColor(item.category)}>
                          {categoryLabel(item.category)}
                        </Badge>
                      </td>
                      <td className="px-4 py-3 font-mono text-right whitespace-nowrap">
                        <span className={item.size_bytes >= 1_073_741_824 ? "text-red-400 font-bold" : item.size_bytes >= 104_857_600 ? "text-amber-400" : "text-zinc-400"}>
                          {item.size_human || formatBytes(item.size_bytes)}
                        </span>
                      </td>
                      <td className="px-4 py-3 text-center">
                        {item.safe ? (
                          <Shield className="w-4 h-4 text-emerald-400 mx-auto" />
                        ) : (
                          <AlertTriangle className="w-4 h-4 text-amber-400/50 mx-auto" />
                        )}
                      </td>
                    </tr>
                    {expandedPath === item.path && (
                      <tr className="bg-zinc-900/50 border-t border-zinc-800/30">
                        <td colSpan={5} className="p-0">
                          <SubPathViewer parentPath={item.path} selected={selected} setSelected={setSelected} />
                        </td>
                      </tr>
                    )}
                  </React.Fragment>
                ))}
              </tbody>
            </table>
          </Card>

          {/* Safety hint */}
          <div className="mt-3 flex items-center gap-2 text-xs text-zinc-600">
            <Shield className="w-3.5 h-3.5 text-emerald-500/50" />
            <span>
              <span className="text-emerald-400/60">Sicher</span> = Caches & Build-Artefakte, die automatisch neu erstellt werden
            </span>
            <span className="mx-2 text-zinc-700">·</span>
            <AlertTriangle className="w-3.5 h-3.5 text-amber-500/50" />
            <span>
              <span className="text-amber-400/60">Vorsicht</span> = Prüfe vor dem Löschen ob du diese Daten brauchst
            </span>
          </div>
        </>
      )}

      {/* No results after scan */}
      {scanned && !scanning && items.length === 0 && (
        <EmptyState
          icon={CheckCircle2}
          title="Alles sauber!"
          description="Keine unnötigen Dateien gefunden"
        />
      )}
    </div>
  );
}

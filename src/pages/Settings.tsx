import { useEffect, useState, useCallback, useRef } from "react";
import {
  Settings as SettingsIcon,
  HardDrive,
  Save,
  RotateCcw,
  RefreshCw,
  Plus,
  Trash2,
  CheckCircle2,
  AlertTriangle,
  Search,
  ChevronDown,
  ChevronRight,
  Shield,
  FolderSync,
  Eye,
  EyeOff,
  Loader2,
  XCircle,
  Download,
  PackageCheck,
} from "lucide-react";
import {
  api,
  AppConfig,
  DetectedDisk,
  ScannedExclude,
  ScanPhase,
  ExcludeScanRuntimeStats,
  IntegrationStatus,
} from "../api";
import { Card, Button, PageHeader, Loading } from "../components/ui";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

type ExcludeSection = "home_excludes" | "home_extra_excludes";

function mapScanCategoryToSection(category: string): ExcludeSection {
  if (
    category === "Gaming" ||
    category === "Container" ||
    category === "VirtualMachine" ||
    category === "LargeUnknown"
  ) {
    return "home_extra_excludes";
  }
  return "home_excludes";
}

function addUniqueExcludes(existing: string[], incoming: string[]): { next: string[]; added: number } {
  const trimmedExisting = existing.map((e) => e.trim()).filter(Boolean);
  const seen = new Set(trimmedExisting);
  const next = [...trimmedExisting];
  let added = 0;

  for (const raw of incoming) {
    const cleaned = raw.trim();
    if (!cleaned || cleaned.startsWith("#")) continue;
    if (!seen.has(cleaned)) {
      seen.add(cleaned);
      next.push(cleaned);
      added += 1;
    }
  }

  return { next, added };
}

export default function Settings() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [detectedDisks, setDetectedDisks] = useState<DetectedDisk[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [detecting, setDetecting] = useState(false);
  const [saved, setSaved] = useState(false);
  const [detectResult, setDetectResult] = useState("");
  const [scanResults, setScanResults] = useState<ScannedExclude[]>([]);
  const [scanApplyResult, setScanApplyResult] = useState("");
  const [scanLogPath, setScanLogPath] = useState("");
  const [scanRuntimeStats, setScanRuntimeStats] = useState<ExcludeScanRuntimeStats | null>(null);
  const [error, setError] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);
  const [integration, setIntegration] = useState<IntegrationStatus | null>(null);
  const [integrating, setIntegrating] = useState(false);
  const [integrationLog, setIntegrationLog] = useState("");
  const savedTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const detectTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Cleanup timeouts on unmount
  useEffect(() => {
    return () => {
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
      if (detectTimerRef.current) clearTimeout(detectTimerRef.current);
    };
  }, []);

  const load = async () => {
    try {
      const [cfg, disks, intStatus] = await Promise.all([
        api.getConfig(),
        api.detectDisks(),
        api.getIntegrationStatus(),
      ]);
      setConfig(cfg);
      setDetectedDisks(disks);
      setIntegration(intStatus);
    } catch (e: any) {
      setError(e.toString());
    }
    setLoading(false);
  };

  useEffect(() => {
    load();
  }, []);

  const handleSave = async () => {
    if (!config) return;
    if (config.disks.primary_uuid !== "" && config.disks.primary_uuid === config.disks.backup_uuid) {
      setError("Primary und Backup dürfen nicht dieselbe Disk sein!");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await api.saveConfig(config);
      setSaved(true);
      if (savedTimerRef.current) clearTimeout(savedTimerRef.current);
      savedTimerRef.current = setTimeout(() => setSaved(false), 3000);
    } catch (e: any) {
      setError(e.toString());
    }
    setSaving(false);
  };

  const handleReset = async () => {
    setDetecting(true);
    setDetectResult("");
    setError("");
    const oldConfig = config;
    const start = Date.now();
    try {
      const [cfg, disks] = await Promise.all([
        api.resetConfig(),
        api.detectDisks(),
      ]);
      const elapsed = Date.now() - start;
      if (elapsed < 600) await new Promise(r => setTimeout(r, 600 - elapsed));
      setConfig(cfg);
      setDetectedDisks(disks);

      const changes: string[] = [];
      if (oldConfig) {
        if (oldConfig.boot.bootloader_type !== cfg.boot.bootloader_type)
          changes.push(`Bootloader: ${oldConfig.boot.bootloader_type} → ${cfg.boot.bootloader_type}`);
        if (oldConfig.disks.primary_uuid !== cfg.disks.primary_uuid)
          changes.push(`Boot-Disk: ${cfg.disks.primary_label}`);
        if (oldConfig.disks.backup_uuid !== cfg.disks.backup_uuid)
          changes.push(`Backup-Disk: ${cfg.disks.backup_label}`);
      }
      const summary = [
        `${disks.length} Disks`,
        `${cfg.sync.subvolumes.length} Subvols`,
        `${cfg.boot.bootloader_type}`,
        ...changes,
      ].filter(Boolean).join(" · ");
      setDetectResult(changes.length > 0
        ? `Scan abgeschlossen — ${summary}`
        : `Scan abgeschlossen — ${summary} — Keine Änderungen nötig ✓`);
      if (detectTimerRef.current) clearTimeout(detectTimerRef.current);
      detectTimerRef.current = setTimeout(() => setDetectResult(""), 6000);
    } catch (e: any) {
      setError(e.toString());
    }
    setDetecting(false);
  };

  const handleScan = async () => {
    setScanning(true);
    try {
      const disks = await api.detectDisks();
      setDetectedDisks(disks);
    } catch (e: any) {
      setError(e.toString());
    }
    setScanning(false);
  };

  // ── Exclude Deep-Scan (streaming via Tauri events) ──
  const [excludeScanning, setExcludeScanning] = useState(false);
  const [scanPhase, setScanPhase] = useState<ScanPhase | null>(null);
  const [scanProgress, setScanProgress] = useState<string>("");
  const unlistenRefs = useRef<UnlistenFn[]>([]);

  const handleExcludeScan = useCallback(async () => {
    setExcludeScanning(true);
    setScanResults([]);
    setScanPhase(null);
    setScanProgress("");
    setScanApplyResult("");
    setScanLogPath("");
    setScanRuntimeStats(null);
    setError("");

    // Subscribe to streaming events
    const unlistenFound = await listen<ScannedExclude>("exclude-found", (event) => {
      setScanResults((prev) => {
        if (prev.some((p) => p.path === event.payload.path)) return prev;
        return [...prev, event.payload];
      });
    });
    const unlistenLogPath = await listen<string>("exclude-scan-log-path", (event) => {
      setScanLogPath(event.payload);
    });
    const unlistenRuntimeStats = await listen<ExcludeScanRuntimeStats>(
      "exclude-scan-runtime-stats",
      (event) => {
        setScanRuntimeStats(event.payload);
      }
    );
    const unlistenPhase = await listen<ScanPhase>("exclude-phase", (event) => {
      setScanPhase(event.payload);
    });
    const unlistenProgress = await listen<string>("exclude-progress", (event) => {
      setScanProgress(event.payload);
    });
    const unlistenDone = await listen<void>("exclude-scan-done", () => {
      setExcludeScanning(false);
      setScanPhase(null);
      setScanProgress("");
      unlistenFound();
      unlistenLogPath();
      unlistenRuntimeStats();
      unlistenPhase();
      unlistenProgress();
      unlistenDone();
    });
    unlistenRefs.current = [
      unlistenFound,
      unlistenLogPath,
      unlistenRuntimeStats,
      unlistenPhase,
      unlistenProgress,
      unlistenDone,
    ];

    try {
      await api.scanExcludes(); // fire-and-forget style — events drive UI
    } catch (e: any) {
      setError(e.toString());
      setExcludeScanning(false);
      setScanPhase(null);
      setScanProgress("");
      unlistenFound();
      unlistenLogPath();
      unlistenRuntimeStats();
      unlistenPhase();
      unlistenProgress();
      unlistenDone();
    }
  }, []);

  const applyScannedExcludes = useCallback((onlyAuto: boolean) => {
    if (!config) return;

    const picked = scanResults.filter((s) => (onlyAuto ? s.auto_exclude : true));
    if (picked.length === 0) {
      setScanApplyResult(onlyAuto
        ? "Keine sicheren (auto) Scan-Ergebnisse zum Übernehmen gefunden."
        : "Keine Scan-Ergebnisse zum Übernehmen gefunden.");
      return;
    }

    const toHome: string[] = [];
    const toExtra: string[] = [];

    for (const item of picked) {
      const target = mapScanCategoryToSection(item.category);
      if (target === "home_extra_excludes") {
        toExtra.push(item.path);
      } else {
        toHome.push(item.path);
      }
    }

    const homeMerged = addUniqueExcludes(config.sync.home_excludes, toHome);
    const extraMerged = addUniqueExcludes(config.sync.home_extra_excludes, toExtra);

    setConfig({
      ...config,
      sync: {
        ...config.sync,
        home_excludes: homeMerged.next,
        home_extra_excludes: extraMerged.next,
      },
    });

    setScanApplyResult(
      `Übernommen: ${homeMerged.added + extraMerged.added} neue Excludes ` +
      `(Home: +${homeMerged.added}, Extra: +${extraMerged.added})`
    );
  }, [config, scanResults]);

  // Cleanup listeners on unmount
  useEffect(() => {
    return () => {
      unlistenRefs.current.forEach((fn) => fn());
    };
  }, []);

  const updateDisk = (
    field: "primary_uuid" | "backup_uuid",
    labelField: "primary_label" | "backup_label",
    disk: DetectedDisk
  ) => {
    if (!config) return;
    setConfig({
      ...config,
      disks: {
        ...config.disks,
        [field]: disk.uuid,
        [labelField]: disk.model || disk.label,
      },
    });
  };

  const addExclude = (
    section: "system_excludes" | "home_excludes" | "home_extra_excludes"
  ) => {
    if (!config) return;
    setConfig({
      ...config,
      sync: { ...config.sync, [section]: [...config.sync[section], ""] },
    });
  };

  const updateExclude = (
    section: "system_excludes" | "home_excludes" | "home_extra_excludes",
    index: number,
    value: string
  ) => {
    if (!config) return;
    const arr = [...config.sync[section]];
    arr[index] = value;
    setConfig({ ...config, sync: { ...config.sync, [section]: arr } });
  };

  const removeExclude = (
    section: "system_excludes" | "home_excludes" | "home_extra_excludes",
    index: number
  ) => {
    if (!config) return;
    const arr = config.sync[section].filter((_, i) => i !== index);
    setConfig({ ...config, sync: { ...config.sync, [section]: arr } });
  };

  const handleCancelScan = async () => {
    try {
      await api.cancelScan();
      setScanning(false);
      setScanPhase({ phase: 0, label: "Abgebrochen" });
    } catch (e) {
      console.error("Failed to cancel scan:", e);
    }
  };

  if (loading) return <div className="p-8"><Loading /></div>;
  if (!config) return <div className="p-8 text-red-400">Config konnte nicht geladen werden: {error}</div>;

  const primaryDisk = detectedDisks.find(d => d.uuid === config.disks.primary_uuid);
  const backupDisk = detectedDisks.find(d => d.uuid === config.disks.backup_uuid);
  const sameDiskSelected = config.disks.primary_uuid !== "" && config.disks.primary_uuid === config.disks.backup_uuid;
  const subvolCount = config.sync.subvolumes.length;
  const excludeCount = config.sync.system_excludes.length + config.sync.home_excludes.length + config.sync.home_extra_excludes.length;

  return (
    <div className="p-8 space-y-6">
      <PageHeader
        title="Einstellungen"
        description="Wähle deine Disks — der Rest wird automatisch konfiguriert"
        actions={
          <div className="flex items-center gap-2">
            {saved && (
              <span className="text-emerald-400 text-sm flex items-center gap-1">
                <CheckCircle2 className="w-4 h-4" /> Gespeichert
              </span>
            )}
            <Button variant="secondary" size="sm" onClick={handleReset} loading={detecting}>
              <RotateCcw className="w-3.5 h-3.5" /> Auto-Detect
            </Button>
            <Button onClick={handleSave} loading={saving}>
              <Save className="w-4 h-4" /> Speichern
            </Button>
          </div>
        }
      />

      {error && (
        <div className="bg-red-500/10 border border-red-500/30 rounded-lg p-3 text-sm text-red-400 flex items-center gap-2">
          <AlertTriangle className="w-4 h-4 shrink-0" /> {error}
        </div>
      )}

      {detectResult && (
        <div className="bg-cyan-500/10 border border-cyan-500/30 rounded-lg p-3 text-sm text-cyan-300 flex items-center gap-2">
          <CheckCircle2 className="w-4 h-4 shrink-0" /> {detectResult}
        </div>
      )}

      {scanResults.length > 0 && (
        <Card>
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-sm font-semibold flex items-center gap-2">
              <Search className="w-4 h-4 text-cyan-400" />
              Scan-Ergebnisse: {scanResults.length} excludierbare Pfade gefunden
            </h3>
            <div className="flex items-center gap-2">
              <Button variant="secondary" size="sm" onClick={() => applyScannedExcludes(true)}>
                Sichere übernehmen
              </Button>
              <Button size="sm" onClick={() => applyScannedExcludes(false)}>
                Alle übernehmen
              </Button>
              <button
                onClick={() => setScanResults([])}
                className="text-xs text-zinc-500 hover:text-zinc-300"
              >Schließen</button>
            </div>
          </div>

          {scanApplyResult && (
            <div className="mb-3 bg-emerald-500/10 border border-emerald-500/30 rounded-lg p-2 text-xs text-emerald-300">
              {scanApplyResult}
            </div>
          )}

          <div className="max-h-64 overflow-y-auto space-y-1">
            {scanResults.slice(0, 30).map((s, i) => (
              <div key={i} className="flex items-center justify-between text-xs py-1.5 px-2 rounded bg-zinc-900/50 hover:bg-zinc-800/50">
                <div className="flex items-center gap-2 min-w-0">
                  <span className={`shrink-0 px-1.5 py-0.5 rounded text-[10px] font-medium ${
                    s.category === "Cache" ? "bg-blue-500/20 text-blue-300" :
                    s.category === "BuildArtifact" ? "bg-amber-500/20 text-amber-300" :
                    s.category === "Gaming" ? "bg-purple-500/20 text-purple-300" :
                    s.category === "Toolchain" ? "bg-emerald-500/20 text-emerald-300" :
                    s.category === "Browser" ? "bg-orange-500/20 text-orange-300" :
                    s.category === "Container" || s.category === "VirtualMachine" ? "bg-red-500/20 text-red-300" :
                    "bg-zinc-500/20 text-zinc-300"
                  }`}>{s.category}</span>
                  <span className="font-mono text-zinc-300 truncate">{s.path}</span>
                  <span className="text-[10px] px-1 py-0.5 rounded bg-zinc-800 text-zinc-400">
                    → {mapScanCategoryToSection(s.category) === "home_excludes" ? "Home" : "Extra"}
                  </span>
                  {s.auto_exclude && (
                    <span className="text-[10px] px-1 py-0.5 rounded bg-emerald-500/20 text-emerald-300">auto</span>
                  )}
                </div>
                <span className="shrink-0 ml-2 text-zinc-500 font-mono">{s.size_human}</span>
              </div>
            ))}
            {scanResults.length > 30 && (
              <div className="text-xs text-zinc-600 text-center py-1">
                … und {scanResults.length - 30} weitere
              </div>
            )}
          </div>
        </Card>
      )}

      {/* ═══════════════ SIMPLE VIEW ═══════════════ */}

      {/* ── Disk Selection ── */}
      <Card>
        <div className="flex items-center justify-between mb-5">
          <h3 className="text-lg font-semibold flex items-center gap-2">
            <HardDrive className="w-5 h-5 text-cyan-400" /> Festplatten
          </h3>
          <Button variant="secondary" size="sm" onClick={handleScan} loading={scanning}>
            <Search className="w-3.5 h-3.5" /> Scannen
          </Button>
        </div>

        <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <DiskPicker
            label="Quelle (Boot-Disk)"
            description="Die Disk von der du gerade bootest"
            uuid={config.disks.primary_uuid}
            diskLabel={config.disks.primary_label}
            disks={detectedDisks}
            onSelect={(d) => updateDisk("primary_uuid", "primary_label", d)}
          />
          <DiskPicker
            label="Backup (Ziel-Disk)"
            description="Hierhin wird alles synchronisiert"
            uuid={config.disks.backup_uuid}
            diskLabel={config.disks.backup_label}
            disks={detectedDisks}
            onSelect={(d) => updateDisk("backup_uuid", "backup_label", d)}
          />
        </div>
        {sameDiskSelected && (
          <div className="mt-3 bg-red-500/10 border border-red-500/30 rounded-lg p-3 text-sm text-red-400 flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 shrink-0" />
            Primary und Backup sind dieselbe Disk! Bitte zwei verschiedene Disks wählen.
          </div>
        )}
      </Card>

      {/* ── Quick Summary ── */}
      <Card>
        <h3 className="text-lg font-semibold mb-4 flex items-center gap-2">
          <FolderSync className="w-5 h-5 text-cyan-400" /> Sync-Übersicht
        </h3>

        <div className="grid grid-cols-2 md:grid-cols-4 gap-4">
          <SummaryItem
            label="Subvolumes"
            value={subvolCount.toString()}
            detail={config.sync.subvolumes.map(s => s.name || s.subvol).join(", ")}
          />
          <SummaryItem
            label="Excludes"
            value={excludeCount.toString()}
            detail={`${config.sync.system_excludes.length} System, ${config.sync.home_excludes.length} Home, ${config.sync.home_extra_excludes.length} Extra`}
          />
          <SummaryItem
            label="Boot-Sync"
            value={config.boot.sync_enabled ? "Aktiv" : "Aus"}
            detail={config.boot.sync_enabled ? `${config.boot.bootloader_type} · ${config.boot.excludes.length} Excludes` : "Deaktiviert"}
            color={config.boot.sync_enabled ? "emerald" : "zinc"}
          />
          <SummaryItem
            label="Sync-Zeitplan"
            value={config.sync.timer_unit}
            detail="Automatischer Sync"
          />
        </div>

        {/* Quick Toggles */}
        <div className="mt-5 pt-4 border-t border-zinc-800 space-y-3">
          <Toggle
            checked={config.boot.sync_enabled}
            onChange={(v) => setConfig({ ...config, boot: { ...config.boot, sync_enabled: v } })}
            label="Boot-Partition synchronisieren"
            description="EFI/Boot-Dateien werden auf die Backup-Disk kopiert"
          />
          <Toggle
            checked={config.sync.extra_excludes_on_primary}
            onChange={(v) => setConfig({ ...config, sync: { ...config.sync, extra_excludes_on_primary: v } })}
            label="Extra-Excludes nur beim Booten von Primary"
            description="Games, Steam etc. nur ausschließen wenn von der Hauptdisk gebootet"
          />
        </div>
      </Card>

      {/* ═══════════════ ADVANCED TOGGLE ═══════════════ */}

      <button
        onClick={() => setShowAdvanced(!showAdvanced)}
        className="w-full flex items-center justify-center gap-2 py-3 text-sm text-zinc-500 hover:text-zinc-300 transition-colors border border-zinc-800 rounded-lg hover:border-zinc-700"
      >
        {showAdvanced ? <EyeOff className="w-4 h-4" /> : <Eye className="w-4 h-4" />}
        {showAdvanced ? "Erweiterte Einstellungen ausblenden" : "Erweiterte Einstellungen anzeigen"}
        {showAdvanced ? <ChevronDown className="w-4 h-4" /> : <ChevronRight className="w-4 h-4" />}
      </button>

      {showAdvanced && (
        <div className="space-y-6 animate-in fade-in duration-200">
          {/* ── Subvolumes & Sync ── */}
          <Card>
            <h3 className="text-lg font-semibold mb-4">Subvolumes</h3>
            <div className="mt-6">
              <div className="flex items-center justify-between mb-2">
                <h4 className="text-sm font-medium text-zinc-400">Subvolumes</h4>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() =>
                    setConfig({
                      ...config,
                      sync: {
                        ...config.sync,
                        subvolumes: [
                          ...config.sync.subvolumes,
                          { name: "", subvol: "@", source: "/", delete: true },
                        ],
                      },
                    })
                  }
                >
                  <Plus className="w-3 h-3" /> Hinzufügen
                </Button>
              </div>
              {config.sync.subvolumes.map((sv, i) => (
                <div
                  key={i}
                  className="flex items-center gap-2 mb-2 bg-zinc-900/50 rounded-lg p-2"
                >
                  <input
                    className="bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-sm w-24"
                    value={sv.name}
                    placeholder="Name"
                    onChange={(e) => {
                      const arr = [...config.sync.subvolumes];
                      arr[i] = { ...arr[i], name: e.target.value };
                      setConfig({
                        ...config,
                        sync: { ...config.sync, subvolumes: arr },
                      });
                    }}
                  />
                  <input
                    className="bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-sm w-20 font-mono"
                    value={sv.subvol}
                    placeholder="@subvol"
                    onChange={(e) => {
                      const arr = [...config.sync.subvolumes];
                      arr[i] = { ...arr[i], subvol: e.target.value };
                      setConfig({
                        ...config,
                        sync: { ...config.sync, subvolumes: arr },
                      });
                    }}
                  />
                  <input
                    className="bg-zinc-800 border border-zinc-700 rounded px-2 py-1 text-sm flex-1 font-mono"
                    value={sv.source}
                    placeholder="/mount/path/"
                    onChange={(e) => {
                      const arr = [...config.sync.subvolumes];
                      arr[i] = { ...arr[i], source: e.target.value };
                      setConfig({
                        ...config,
                        sync: { ...config.sync, subvolumes: arr },
                      });
                    }}
                  />
                  <label className="flex items-center gap-1 text-xs text-zinc-500">
                    <input
                      type="checkbox"
                      checked={sv.delete}
                      className="accent-cyan-500"
                      onChange={(e) => {
                        const arr = [...config.sync.subvolumes];
                        arr[i] = { ...arr[i], delete: e.target.checked };
                        setConfig({
                          ...config,
                          sync: { ...config.sync, subvolumes: arr },
                        });
                      }}
                    />
                    --delete
                  </label>
                  <button
                    className="text-red-400 hover:text-red-300 p-1"
                    onClick={() => {
                      const arr = config.sync.subvolumes.filter((_, j) => j !== i);
                      setConfig({
                        ...config,
                        sync: { ...config.sync, subvolumes: arr },
                      });
                    }}
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              ))}
            </div>
          </Card>

          {/* ── Excludes ── */}
          <Card>
            <div className="flex items-center justify-between mb-4">
              <h3 className="text-lg font-semibold">Excludes</h3>
              {excludeScanning ? (
                <Button variant="danger" size="sm" onClick={handleCancelScan}>
                  <XCircle className="w-3.5 h-3.5" /> Abbrechen
                </Button>
              ) : (
                <Button
                  variant="secondary"
                  size="sm"
                  onClick={handleExcludeScan}
                  loading={excludeScanning}
                >
                  <Search className="w-3.5 h-3.5" /> Home scannen
                </Button>
              )}
            </div>

            {excludeScanning && (
              <div className="mb-4 bg-cyan-500/10 border border-cyan-500/30 rounded-lg p-3 text-sm text-cyan-300">
                <div className="flex items-center gap-2 mb-2">
                  <Loader2 className="w-4 h-4 animate-spin shrink-0" />
                  Scanne Home-Verzeichnis… {scanResults.length} Pfade gefunden
                </div>
                {scanPhase && (
                  <div className="flex items-center gap-2">
                    <div className="flex gap-1">
                      {[1, 2, 3].map((p) => (
                        <div key={p} className={`h-1.5 w-8 rounded-full transition-colors ${
                          p < scanPhase.phase ? "bg-cyan-400" :
                          p === scanPhase.phase ? "bg-cyan-400 animate-pulse" :
                          "bg-zinc-700"
                        }`} />
                      ))}
                    </div>
                    <span className="text-xs text-zinc-500">
                      Phase {scanPhase.phase}/3: {scanPhase.label}
                    </span>
                  </div>
                )}
                {scanProgress && (
                  <div className="mt-2 text-[10px] text-zinc-500 truncate font-mono bg-zinc-900/50 px-2 py-1 rounded border border-zinc-800/50" title={scanProgress}>
                    {scanProgress}
                  </div>
                )}
                {scanRuntimeStats && (
                  <div className="mt-2 grid grid-cols-2 md:grid-cols-4 gap-1.5">
                    <span className="text-[10px] px-2 py-1 rounded bg-zinc-900/60 border border-zinc-800 text-zinc-400">
                      CPU Threads: {scanRuntimeStats.cpu_threads}
                    </span>
                    <span className="text-[10px] px-2 py-1 rounded bg-zinc-900/60 border border-zinc-800 text-zinc-400">
                      I/O Worker Cap: {scanRuntimeStats.io_workers_cap}
                    </span>
                    <span className="text-[10px] px-2 py-1 rounded bg-zinc-900/60 border border-zinc-800 text-zinc-400">
                      Rayon Threads: {scanRuntimeStats.rayon_threads}
                    </span>
                    <span className="text-[10px] px-2 py-1 rounded bg-zinc-900/60 border border-zinc-800 text-zinc-400">
                      Tokio Blocking: {scanRuntimeStats.tokio_blocking_task}
                    </span>
                  </div>
                )}
                {scanLogPath && (
                  <div className="mt-2 text-xs text-zinc-400 font-mono break-all">
                    Log: {scanLogPath}
                  </div>
                )}
              </div>
            )}

            <ExcludeList
              title="System-Excludes"
              description="Pfade die beim System-Sync (/) ausgelassen werden"
              items={config.sync.system_excludes}
              onAdd={() => addExclude("system_excludes")}
              onUpdate={(i, v) => updateExclude("system_excludes", i, v)}
              onRemove={(i) => removeExclude("system_excludes", i)}
            />

            <ExcludeList
              title="Home-Excludes"
              description="Pfade die beim Home-Sync immer ausgelassen werden"
              items={config.sync.home_excludes}
              onAdd={() => addExclude("home_excludes")}
              onUpdate={(i, v) => updateExclude("home_excludes", i, v)}
              onRemove={(i) => removeExclude("home_excludes", i)}
            />

            <ExcludeList
              title="Home Extra-Excludes"
              description="Zusätzliche Excludes (z.B. Games, Steam) — nur wenn von Primary gebootet"
              items={config.sync.home_extra_excludes}
              onAdd={() => addExclude("home_extra_excludes")}
              onUpdate={(i, v) => updateExclude("home_extra_excludes", i, v)}
              onRemove={(i) => removeExclude("home_extra_excludes", i)}
            />
          </Card>

          {/* ── Boot ── */}
          <Card>
            <h3 className="text-lg font-semibold mb-4">Boot-Konfiguration</h3>
            <div className="mb-4 p-3 rounded-lg bg-zinc-900/50 border border-zinc-800">
              <div className="flex items-center justify-between mb-1">
                <span className="text-xs text-zinc-500">Erkannter Bootloader</span>
                <span className={`text-xs font-mono font-bold ${
                  config.boot.bootloader_type === "systemd-boot" ? "text-cyan-400" :
                  config.boot.bootloader_type === "grub" ? "text-amber-400" : "text-zinc-400"
                }`}>{config.boot.bootloader_type}</span>
              </div>
              <p className="text-[10px] text-zinc-600">
                {config.boot.bootloader_type === "systemd-boot" 
                  ? "Boot-Sync nutzt bootctl update für den Backup-Bootloader"
                  : config.boot.bootloader_type === "grub"
                  ? "Boot-Sync nutzt grub-install für den Backup-Bootloader"
                  : "Unbekannter Bootloader — Boot-Sync kopiert nur Dateien"}
              </p>
            </div>
            <ExcludeList
              title="Boot-Excludes"
              description="Dateien die beim Boot-Sync ausgelassen werden"
              items={config.boot.excludes}
              onAdd={() =>
                setConfig({
                  ...config,
                  boot: { ...config.boot, excludes: [...config.boot.excludes, ""] },
                })
              }
              onUpdate={(i, v) => {
                const arr = [...config.boot.excludes];
                arr[i] = v;
                setConfig({ ...config, boot: { ...config.boot, excludes: arr } });
              }}
              onRemove={(i) => {
                const arr = config.boot.excludes.filter((_, j) => j !== i);
                setConfig({ ...config, boot: { ...config.boot, excludes: arr } });
              }}
            />
          </Card>

          {/* ── Snapper + Rollback ── */}
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-6">
            <Card>
              <h3 className="text-lg font-semibold mb-4">Snapper</h3>
              <ExcludeList
                title="Erwartete Configs"
                description="Health-Check prüft ob diese Configs existieren"
                items={config.snapper.expected_configs}
                onAdd={() =>
                  setConfig({
                    ...config,
                    snapper: {
                      ...config.snapper,
                      expected_configs: [...config.snapper.expected_configs, ""],
                    },
                  })
                }
                onUpdate={(i, v) => {
                  const arr = [...config.snapper.expected_configs];
                  arr[i] = v;
                  setConfig({
                    ...config,
                    snapper: { ...config.snapper, expected_configs: arr },
                  });
                }}
                onRemove={(i) => {
                  const arr = config.snapper.expected_configs.filter(
                    (_, j) => j !== i
                  );
                  setConfig({
                    ...config,
                    snapper: { ...config.snapper, expected_configs: arr },
                  });
                }}
              />
            </Card>

            <Card>
              <h3 className="text-lg font-semibold mb-4">Rollback</h3>
              <div className="space-y-3">
                <Field
                  label="Root-Subvolume"
                  value={config.rollback.root_subvol}
                  onChange={(v) =>
                    setConfig({
                      ...config,
                      rollback: { ...config.rollback, root_subvol: v },
                    })
                  }
                />
                <Field
                  label="Snapper Root-Config"
                  value={config.rollback.root_config}
                  onChange={(v) =>
                    setConfig({
                      ...config,
                      rollback: { ...config.rollback, root_config: v },
                    })
                  }
                />
                <Field
                  label="Recovery-Eintrag"
                  value={config.rollback.recovery_label}
                  onChange={(v) =>
                    setConfig({
                      ...config,
                      rollback: { ...config.rollback, recovery_label: v },
                    })
                  }
                />
                <Field
                  label="Max. Broken-Backups"
                  value={String(config.rollback.max_broken_backups)}
                  onChange={(v) =>
                    setConfig({
                      ...config,
                      rollback: {
                        ...config.rollback,
                        max_broken_backups: parseInt(v) || 2,
                      },
                    })
                  }
                />
              </div>
            </Card>
          </div>
        </div>
      )}

      {/* System-Integration */}
      <Card className="mb-4">
        <div className="flex items-center gap-3 mb-4">
          <PackageCheck className="w-5 h-5 text-cyan-400" />
          <h3 className="text-lg font-semibold">System-Integration (Arch Linux)</h3>
        </div>

        {/* Status grid */}
        {integration && (
          <div className="grid grid-cols-2 gap-2 mb-4">
            {([
              { key: "binary",      label: "Binary",          desc: "/usr/local/bin/backsnap" },
              { key: "desktop",     label: "App-Launcher",    desc: "/usr/share/applications" },
              { key: "polkit",      label: "Polkit-Regel",    desc: "Kein Passwort-Prompt" },
              { key: "pacman_hook", label: "Pacman-Hook",     desc: "Pre-Update Snapshot" },
            ] as const).map(({ key, label, desc }) => (
              <div key={key} className="flex items-center gap-2 bg-zinc-900/50 rounded-lg p-3">
                {integration[key] ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
                ) : (
                  <XCircle className="w-4 h-4 text-zinc-600 shrink-0" />
                )}
                <div>
                  <p className={`text-sm font-medium ${
                    integration[key] ? "text-zinc-200" : "text-zinc-500"
                  }`}>{label}</p>
                  <p className="text-xs text-zinc-600">{desc}</p>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Action buttons */}
        <div className="flex gap-2">
          <Button
            onClick={async () => {
              setIntegrating(true);
              setIntegrationLog("");
              try {
                const log = await api.installSystemIntegration();
                setIntegrationLog(log);
                setIntegration(await api.getIntegrationStatus());
              } catch (e: any) {
                setIntegrationLog("Fehler: " + e.toString());
              }
              setIntegrating(false);
            }}
            loading={integrating}
            disabled={integrating}
          >
            <Download className="w-4 h-4" />
            {integration?.binary ? "Neu installieren" : "Installieren"}
          </Button>
          {integration?.binary && (
            <Button
              variant="danger"
              onClick={async () => {
                setIntegrating(true);
                setIntegrationLog("");
                try {
                  const log = await api.uninstallSystemIntegration();
                  setIntegrationLog(log);
                  setIntegration(await api.getIntegrationStatus());
                } catch (e: any) {
                  setIntegrationLog("Fehler: " + e.toString());
                }
                setIntegrating(false);
              }}
              loading={integrating}
              disabled={integrating}
            >
              <Trash2 className="w-4 h-4" />
              Entfernen
            </Button>
          )}
        </div>

        {/* Result log */}
        {integrationLog && (
          <pre className="mt-3 text-xs font-mono bg-zinc-950 rounded-lg p-3 text-zinc-400 whitespace-pre-wrap">
            {integrationLog}
          </pre>
        )}

        <p className="mt-3 text-xs text-zinc-600">
          Installiert Binary, App-Starter, Polkit-Regel (kein Passwort-Prompt) und
          Pacman-Hook (automatischer Snapshot vor jedem System-Update).
        </p>
      </Card>

      {/* Config Path Info */}
      <div className="text-xs text-zinc-600 text-center">
        Config: ~/.config/backsnap/config.toml
      </div>
    </div>
  );
}

// ── Reusable Components ──

function DiskPicker({
  label,
  description,
  uuid,
  diskLabel,
  disks,
  onSelect,
}: {
  label: string;
  description: string;
  uuid: string;
  diskLabel: string;
  disks: DetectedDisk[];
  onSelect: (d: DetectedDisk) => void;
}) {
  const selected = disks.find(d => d.uuid === uuid);
  return (
    <div className="space-y-2">
      <div>
        <label className="text-sm font-medium text-zinc-300">{label}</label>
        <p className="text-xs text-zinc-600">{description}</p>
      </div>
      <select
        className="w-full bg-zinc-800 text-zinc-200 border border-zinc-700 rounded-lg px-3 py-2.5 text-sm focus:outline-none focus:border-cyan-500 transition-colors [&>option]:bg-zinc-800 [&>option]:text-zinc-200"
        value={uuid}
        onChange={(e) => {
          const d = disks.find((d) => d.uuid === e.target.value);
          if (d) onSelect(d);
        }}
      >
        <option value="" className="bg-zinc-800 text-zinc-400">— Nicht konfiguriert —</option>
        {disks.map((d) => (
          <option key={d.uuid} value={d.uuid} className="bg-zinc-800 text-zinc-200">
            {d.model || d.label} — {d.size} ({d.device})
            {d.is_boot ? " ★ Boot" : ""}
          </option>
        ))}
      </select>
      {selected && (
        <div className="flex items-center gap-2 text-xs text-zinc-500">
          <HardDrive className="w-3 h-3" />
          <span className="font-mono">{selected.device}</span>
          <span>•</span>
          <span>{selected.size}</span>
          {selected.is_boot && (
            <>
              <span>•</span>
              <span className="text-emerald-500">★ Boot</span>
            </>
          )}
        </div>
      )}
    </div>
  );
}

function SummaryItem({
  label,
  value,
  detail,
  color = "cyan",
}: {
  label: string;
  value: string;
  detail: string;
  color?: string;
}) {
  const colors: Record<string, string> = {
    cyan: "text-cyan-400",
    emerald: "text-emerald-400",
    zinc: "text-zinc-500",
  };
  return (
    <div className="bg-zinc-900/50 rounded-lg p-3">
      <div className="text-xs text-zinc-500 mb-1">{label}</div>
      <div className={`text-xl font-bold ${colors[color]}`}>{value}</div>
      <div className="text-xs text-zinc-600 mt-0.5 truncate" title={detail}>
        {detail}
      </div>
    </div>
  );
}

function Toggle({
  checked,
  onChange,
  label,
  description,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  description: string;
}) {
  return (
    <label className="flex items-start gap-3 cursor-pointer group">
      <div className="pt-0.5">
        <div
          className={`w-9 h-5 rounded-full transition-colors relative ${
            checked ? "bg-cyan-500" : "bg-zinc-700"
          }`}
          onClick={() => onChange(!checked)}
        >
          <div
            className={`absolute top-0.5 w-4 h-4 rounded-full bg-white shadow transition-transform ${
              checked ? "translate-x-4.5" : "translate-x-0.5"
            }`}
          />
        </div>
      </div>
      <div onClick={() => onChange(!checked)}>
        <div className="text-sm font-medium text-zinc-300 group-hover:text-white transition-colors">
          {label}
        </div>
        <div className="text-xs text-zinc-600">{description}</div>
      </div>
    </label>
  );
}

function Field({
  label,
  value,
  onChange,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
}) {
  return (
    <div>
      <label className="text-xs text-zinc-500 block mb-1">{label}</label>
      <input
        className="w-full bg-zinc-800 border border-zinc-700 rounded px-3 py-1.5 text-sm font-mono focus:outline-none focus:border-cyan-500"
        value={value}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

function ExcludeList({
  title,
  description,
  items,
  onAdd,
  onUpdate,
  onRemove,
}: {
  title: string;
  description: string;
  items: string[];
  onAdd: () => void;
  onUpdate: (i: number, v: string) => void;
  onRemove: (i: number) => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <div className="mb-4">
      <button
        onClick={() => setOpen(!open)}
        className="w-full flex items-center justify-between py-1.5 group"
      >
        <div className="flex items-center gap-2">
          {open
            ? <ChevronDown className="w-3.5 h-3.5 text-zinc-500" />
            : <ChevronRight className="w-3.5 h-3.5 text-zinc-500" />
          }
          <span className="text-sm font-medium">{title}</span>
          <span className="text-xs text-zinc-600">({items.length})</span>
        </div>
        <span className="text-xs text-zinc-600 group-hover:text-zinc-400 transition-colors">
          {open ? "Zuklappen" : "Bearbeiten"}
        </span>
      </button>
      {!open && (
        <p className="text-xs text-zinc-600 ml-6 truncate">
          {items.length > 0 ? items.slice(0, 4).join(", ") + (items.length > 4 ? ` … +${items.length - 4}` : "") : "Keine Einträge"}
        </p>
      )}
      {open && (
        <div className="mt-2 ml-6">
          <p className="text-xs text-zinc-600 mb-2">{description}</p>
          <div className="space-y-1">
            {items.map((item, i) => (
              <div key={i} className="flex items-center gap-1">
                <input
                  className="flex-1 bg-zinc-900 border border-zinc-800 rounded px-2 py-1 text-xs font-mono focus:outline-none focus:border-cyan-500/50"
                  value={item}
                  onChange={(e) => onUpdate(i, e.target.value)}
                  placeholder="/path/to/exclude"
                />
                <button
                  onClick={() => onRemove(i)}
                  className="text-red-400/50 hover:text-red-400 p-0.5"
                >
                  <Trash2 className="w-3 h-3" />
                </button>
              </div>
            ))}
          </div>
          <button
            onClick={onAdd}
            className="mt-2 text-xs text-cyan-400 hover:text-cyan-300 flex items-center gap-1"
          >
            <Plus className="w-3 h-3" /> Hinzufügen
          </button>
        </div>
      )}
    </div>
  );
}

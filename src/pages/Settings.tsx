import { useEffect, useState } from "react";
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
} from "lucide-react";
import { api, AppConfig, DetectedDisk } from "../api";
import { Card, Button, PageHeader, Loading } from "../components/ui";

export default function Settings() {
  const [config, setConfig] = useState<AppConfig | null>(null);
  const [detectedDisks, setDetectedDisks] = useState<DetectedDisk[]>([]);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState("");
  const [showAdvanced, setShowAdvanced] = useState(false);

  const load = async () => {
    try {
      const [cfg, disks] = await Promise.all([
        api.getConfig(),
        api.detectDisks(),
      ]);
      setConfig(cfg);
      setDetectedDisks(disks);
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
    setSaving(true);
    setError("");
    try {
      await api.saveConfig(config);
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } catch (e: any) {
      setError(e.toString());
    }
    setSaving(false);
  };

  const handleReset = async () => {
    setSaving(true);
    setError("");
    try {
      const cfg = await api.resetConfig();
      setConfig(cfg);
      setSaved(true);
      setTimeout(() => setSaved(false), 3000);
    } catch (e: any) {
      setError(e.toString());
    }
    setSaving(false);
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

  if (loading) return <div className="p-8"><Loading /></div>;
  if (!config) return <div className="p-8 text-red-400">Config konnte nicht geladen werden: {error}</div>;

  const primaryDisk = detectedDisks.find(d => d.uuid === config.disks.primary_uuid);
  const backupDisk = detectedDisks.find(d => d.uuid === config.disks.backup_uuid);
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
            <Button variant="secondary" size="sm" onClick={handleReset}>
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
            detail={config.boot.sync_enabled ? `${config.boot.excludes.length} Excludes` : "Deaktiviert"}
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
            <h3 className="text-lg font-semibold mb-4">Excludes</h3>

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
            <h3 className="text-lg font-semibold mb-4">Boot-Excludes</h3>
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

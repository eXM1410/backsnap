import { useEffect, useState } from "react";
import {
  Shield,
  ShieldCheck,
  ShieldAlert,
  ShieldX,
  RefreshCw,
  Download,
  Upload,
  Trash2,
  HardDrive,
  Cpu,
  FileText,
  ChevronDown,
  ChevronRight,
  AlertTriangle,
  CheckCircle2,
  XCircle,
  Clock,
} from "lucide-react";
import {
  api,
  apiError,
  BootHealth,
  EntryHealth,
  BackupInfo,
} from "../api";
import { Card, Button, Badge, PageHeader, Loading } from "../components/ui";

// ─── Status helpers ───────────────────────────────────────────

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case "healthy":
      return <ShieldCheck className="w-6 h-6 text-emerald-400" />;
    case "warning":
      return <ShieldAlert className="w-6 h-6 text-amber-400" />;
    case "critical":
      return <ShieldX className="w-6 h-6 text-red-400" />;
    default:
      return <Shield className="w-6 h-6 text-zinc-400" />;
  }
}

function statusColor(status: string): "green" | "yellow" | "red" | "zinc" {
  switch (status) {
    case "healthy":
      return "green";
    case "warning":
      return "yellow";
    case "critical":
      return "red";
    default:
      return "zinc";
  }
}

function statusLabel(status: string): string {
  switch (status) {
    case "healthy":
      return "Gesund";
    case "warning":
      return "Warnung";
    case "critical":
      return "Kritisch";
    default:
      return "Unbekannt";
  }
}

// ─── Main Component ───────────────────────────────────────────

export default function BootGuard() {
  const [health, setHealth] = useState<BootHealth | null>(null);
  const [loading, setLoading] = useState(true);
  const [backing, setBacking] = useState(false);
  const [restoring, setRestoring] = useState<number | null>(null);
  const [deleting, setDeleting] = useState<number | null>(null);
  const [message, setMessage] = useState<{
    text: string;
    ok: boolean;
  } | null>(null);
  const [expandedEntry, setExpandedEntry] = useState<string | null>(null);
  const [expandedBackup, setExpandedBackup] = useState<number | null>(null);

  const refresh = async () => {
    setLoading(true);
    try {
      const h = await api.getBootHealth();
      setHealth(h);
    } catch (e) {
      setMessage({ text: apiError(e), ok: false });
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();
  }, []);

  const handleBackup = async () => {
    setBacking(true);
    try {
      const info = await api.backupBootEntries();
      setMessage({
        text: `Backup erstellt: ${info.entry_count} Entries gesichert`,
        ok: true,
      });
      await refresh();
    } catch (e) {
      setMessage({ text: apiError(e), ok: false });
    }
    setBacking(false);
  };

  const handleRestore = async (timestamp: number) => {
    setRestoring(timestamp);
    try {
      const result = await api.restoreBootEntries(timestamp);
      if (result.success) {
        setMessage({
          text: `Wiederhergestellt: ${result.restored.join(", ")}`,
          ok: true,
        });
      } else {
        setMessage({
          text: `Fehler: ${result.errors.join("; ")}`,
          ok: false,
        });
      }
      await refresh();
    } catch (e) {
      setMessage({ text: apiError(e), ok: false });
    }
    setRestoring(null);
  };

  const handleDelete = async (timestamp: number) => {
    setDeleting(timestamp);
    try {
      await api.deleteBootBackup(timestamp);
      setMessage({ text: "Backup gelöscht", ok: true });
      await refresh();
    } catch (e) {
      setMessage({ text: apiError(e), ok: false });
    }
    setDeleting(null);
  };

  if (loading && !health) {
    return (
      <div className="p-6">
        <Loading text="Boot-Konfiguration wird geprüft..." />
      </div>
    );
  }

  return (
    <div className="p-6 space-y-6 max-w-5xl">
      <PageHeader
        title="Boot Guard"
        description="Überwachung und Schutz der Boot-Konfiguration"
        actions={
          <div className="flex gap-2">
            <Button onClick={handleBackup} disabled={backing}>
              <Download className="w-4 h-4 mr-1.5" />
              {backing ? "Sichere..." : "Backup erstellen"}
            </Button>
            <Button variant="ghost" onClick={refresh}>
              <RefreshCw className="w-4 h-4" />
            </Button>
          </div>
        }
      />

      {/* Message Toast */}
      {message && (
        <div
          className={`rounded-lg px-4 py-3 text-sm border ${
            message.ok
              ? "bg-emerald-500/10 border-emerald-500/20 text-emerald-400"
              : "bg-red-500/10 border-red-500/20 text-red-400"
          }`}
        >
          {message.text}
          <button
            className="float-right text-xs opacity-60 hover:opacity-100"
            onClick={() => setMessage(null)}
          >
            ✕
          </button>
        </div>
      )}

      {health && (
        <>
          {/* ─── Overall Status ─────────────────────────────── */}
          <Card>
            <div className="flex items-center gap-4">
              <StatusIcon status={health.status} />
              <div className="flex-1">
                <div className="flex items-center gap-3">
                  <h2 className="text-lg font-semibold">Boot-Status</h2>
                  <Badge color={statusColor(health.status)}>
                    {statusLabel(health.status)}
                  </Badge>
                </div>
                <p className="text-sm text-zinc-500 mt-1">
                  {health.entries.length} Boot-Entries · {health.backups.length}{" "}
                  Backups vorhanden
                </p>
              </div>
            </div>
          </Card>

          {/* ─── Issues ────────────────────────────────────── */}
          {health.issues.length > 0 && (
            <Card>
              <h3 className="text-sm font-semibold text-red-400 mb-3 flex items-center gap-2">
                <AlertTriangle className="w-4 h-4" />
                Probleme erkannt ({health.issues.length})
              </h3>
              <div className="space-y-2">
                {health.issues.map((issue, i) => (
                  <div
                    key={i}
                    className="flex items-start gap-2 text-sm text-zinc-300 bg-red-500/5 rounded-lg px-3 py-2 border border-red-500/10"
                  >
                    <XCircle className="w-4 h-4 text-red-400 mt-0.5 shrink-0" />
                    {issue}
                  </div>
                ))}
              </div>
            </Card>
          )}

          {/* ─── System Info ───────────────────────────────── */}
          <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
            <Card>
              <div className="flex items-center gap-3 mb-2">
                <HardDrive className="w-4 h-4 text-cyan-400" />
                <span className="text-sm font-medium">/boot Mount</span>
              </div>
              <div className="flex items-center gap-2">
                {health.boot_mounted ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                ) : (
                  <XCircle className="w-4 h-4 text-red-400" />
                )}
                <span className="text-sm text-zinc-300">
                  {health.boot_mounted
                    ? health.boot_device || "Gemountet"
                    : "Nicht gemountet!"}
                </span>
              </div>
            </Card>

            <Card>
              <div className="flex items-center gap-3 mb-2">
                <Cpu className="w-4 h-4 text-cyan-400" />
                <span className="text-sm font-medium">Laufender Kernel</span>
              </div>
              <p className="text-sm text-zinc-300 font-mono truncate">
                {health.running_kernel}
              </p>
            </Card>

            <Card>
              <div className="flex items-center gap-3 mb-2">
                <Cpu className="w-4 h-4 text-cyan-400" />
                <span className="text-sm font-medium">Kernel/Module</span>
              </div>
              <div className="flex items-center gap-2">
                {health.kernel_module_match ? (
                  <CheckCircle2 className="w-4 h-4 text-emerald-400" />
                ) : (
                  <XCircle className="w-4 h-4 text-red-400" />
                )}
                <span className="text-sm text-zinc-300">
                  {health.kernel_module_match
                    ? "Module passen zum Kernel"
                    : "MISMATCH!"}
                </span>
              </div>
              {!health.kernel_module_match && (
                <p className="text-xs text-zinc-500 mt-1">
                  Module: {health.installed_modules.join(", ")}
                </p>
              )}
            </Card>
          </div>

          {/* ─── Boot Entries ──────────────────────────────── */}
          <Card>
            <h3 className="text-sm font-semibold mb-3 flex items-center gap-2">
              <FileText className="w-4 h-4 text-cyan-400" />
              Boot-Entries
            </h3>
            <div className="space-y-2">
              {health.entries.map((entry) => (
                <EntryRow
                  key={entry.filename}
                  entry={entry}
                  expanded={expandedEntry === entry.filename}
                  onToggle={() =>
                    setExpandedEntry(
                      expandedEntry === entry.filename
                        ? null
                        : entry.filename
                    )
                  }
                />
              ))}
              {health.entries.length === 0 && (
                <p className="text-sm text-zinc-500">
                  {health.boot_mounted
                    ? "Keine Boot-Entries gefunden."
                    : "/boot nicht gemountet — Entries können nicht gelesen werden."}
                </p>
              )}
            </div>
          </Card>

          {/* ─── Backups ───────────────────────────────────── */}
          <Card>
            <h3 className="text-sm font-semibold mb-3 flex items-center gap-2">
              <Clock className="w-4 h-4 text-cyan-400" />
              Backups ({health.backups.length})
            </h3>
            {health.backups.length === 0 ? (
              <div className="text-sm text-zinc-500 bg-zinc-800/30 rounded-lg p-4 text-center">
                <Shield className="w-8 h-8 mx-auto mb-2 text-zinc-600" />
                <p>Noch kein Backup vorhanden.</p>
                <p className="text-xs mt-1">
                  Erstelle jetzt ein Backup deiner Boot-Entries als Referenz.
                </p>
              </div>
            ) : (
              <div className="space-y-2">
                {health.backups.map((backup) => (
                  <BackupRow
                    key={backup.timestamp}
                    backup={backup}
                    expanded={expandedBackup === backup.timestamp}
                    onToggle={() =>
                      setExpandedBackup(
                        expandedBackup === backup.timestamp
                          ? null
                          : backup.timestamp
                      )
                    }
                    onRestore={() => handleRestore(backup.timestamp)}
                    onDelete={() => handleDelete(backup.timestamp)}
                    restoring={restoring === backup.timestamp}
                    deleting={deleting === backup.timestamp}
                  />
                ))}
              </div>
            )}
          </Card>

          {/* ─── How it works ──────────────────────────────── */}
          <Card>
            <h3 className="text-sm font-semibold mb-2">Wie Boot Guard funktioniert</h3>
            <div className="text-xs text-zinc-500 space-y-1.5">
              <p>
                <strong className="text-zinc-400">Automatisch:</strong> Vor jedem
                Kernel-Update (pacman-Hook) werden alle Boot-Entries gesichert.
              </p>
              <p>
                <strong className="text-zinc-400">Health-Check:</strong> Prüft ob
                /boot gemountet ist, Kernel und Module übereinstimmen, und
                wichtige Kernel-Parameter (amdgpu, mitigations, etc.) nicht
                verschwunden sind.
              </p>
              <p>
                <strong className="text-zinc-400">1-Click Restore:</strong> Stellt
                Boot-Entries aus einem Backup wieder her — ohne chroot oder
                Live-USB.
              </p>
            </div>
          </Card>
        </>
      )}
    </div>
  );
}

// ─── Entry Row ────────────────────────────────────────────────

function EntryRow({
  entry,
  expanded,
  onToggle,
}: {
  entry: EntryHealth;
  expanded: boolean;
  onToggle: () => void;
}) {
  const hasIssue =
    !entry.kernel_exists ||
    !entry.initramfs_exists ||
    !entry.custom_params_intact;
  const changed = entry.changed_since_backup;

  return (
    <div
      className={`rounded-lg border transition-colors ${
        hasIssue
          ? "border-red-500/20 bg-red-500/5"
          : changed
          ? "border-amber-500/20 bg-amber-500/5"
          : "border-zinc-800 bg-zinc-800/20"
      }`}
    >
      <button
        onClick={onToggle}
        className="w-full flex items-center gap-3 px-4 py-3 text-left"
      >
        {expanded ? (
          <ChevronDown className="w-4 h-4 text-zinc-500 shrink-0" />
        ) : (
          <ChevronRight className="w-4 h-4 text-zinc-500 shrink-0" />
        )}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-medium truncate">
              {entry.title || entry.filename}
            </span>
            <span className="text-xs text-zinc-600 font-mono">
              {entry.filename}
            </span>
          </div>
        </div>
        <div className="flex gap-1.5">
          {entry.kernel_exists ? (
            <Badge color="green">Kernel ✓</Badge>
          ) : (
            <Badge color="red">Kernel fehlt</Badge>
          )}
          {entry.initramfs_exists ? (
            <Badge color="green">Initramfs ✓</Badge>
          ) : (
            <Badge color="red">Initramfs fehlt</Badge>
          )}
          {!entry.custom_params_intact && (
            <Badge color="red">Params fehlen</Badge>
          )}
          {changed && entry.custom_params_intact && (
            <Badge color="yellow">Geändert</Badge>
          )}
        </div>
      </button>

      {expanded && (
        <div className="px-4 pb-3 space-y-2 border-t border-zinc-800/50">
          {/* Options line */}
          <div className="mt-2">
            <p className="text-xs text-zinc-500 mb-1">Kernel-Parameter:</p>
            <code className="text-xs text-zinc-300 bg-zinc-900 rounded px-2 py-1.5 block overflow-x-auto whitespace-pre-wrap break-all">
              {entry.options || "(leer)"}
            </code>
          </div>

          {/* Missing params */}
          {entry.missing_params.length > 0 && (
            <div>
              <p className="text-xs text-red-400 mb-1">
                Fehlende Parameter (waren im Backup):
              </p>
              <div className="flex flex-wrap gap-1">
                {entry.missing_params.map((p) => (
                  <Badge key={p} color="red">
                    {p}
                  </Badge>
                ))}
              </div>
            </div>
          )}

          {/* Diff */}
          {entry.diff.length > 0 && (
            <div>
              <p className="text-xs text-zinc-500 mb-1">
                Änderungen seit letztem Backup:
              </p>
              <div className="font-mono text-xs bg-zinc-900 rounded p-2 space-y-0.5 overflow-x-auto">
                {entry.diff.map((line, i) => (
                  <div
                    key={i}
                    className={
                      line.startsWith("- ")
                        ? "text-red-400"
                        : line.startsWith("+ ")
                        ? "text-emerald-400"
                        : "text-zinc-500"
                    }
                  >
                    {line}
                  </div>
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ─── Backup Row ───────────────────────────────────────────────

function BackupRow({
  backup,
  expanded,
  onToggle,
  onRestore,
  onDelete,
  restoring,
  deleting,
}: {
  backup: BackupInfo;
  expanded: boolean;
  onToggle: () => void;
  onRestore: () => void;
  onDelete: () => void;
  restoring: boolean;
  deleting: boolean;
}) {
  return (
    <div className="rounded-lg border border-zinc-800 bg-zinc-800/20">
      <div className="flex items-center gap-3 px-4 py-3">
        <button onClick={onToggle} className="shrink-0">
          {expanded ? (
            <ChevronDown className="w-4 h-4 text-zinc-500" />
          ) : (
            <ChevronRight className="w-4 h-4 text-zinc-500" />
          )}
        </button>
        <div className="flex-1 min-w-0">
          <p className="text-sm font-medium truncate">{backup.label}</p>
          <p className="text-xs text-zinc-500">
            {backup.entry_count} Entries
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="ghost"
            size="sm"
            onClick={onRestore}
            disabled={restoring}
          >
            <Upload className="w-3.5 h-3.5 mr-1" />
            {restoring ? "..." : "Restore"}
          </Button>
          <Button
            variant="ghost"
            size="sm"
            onClick={onDelete}
            disabled={deleting}
          >
            <Trash2 className="w-3.5 h-3.5 text-red-400" />
          </Button>
        </div>
      </div>

      {expanded && (
        <div className="px-4 pb-3 border-t border-zinc-800/50">
          <p className="text-xs text-zinc-500 mt-2">
            Timestamp: {backup.timestamp} ·{" "}
            {new Date(backup.timestamp * 1000).toLocaleString("de-DE")}
          </p>
        </div>
      )}
    </div>
  );
}

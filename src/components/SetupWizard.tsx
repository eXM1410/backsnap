import { useEffect, useState } from "react";
import {
  Shield,
  HardDrive,
  CheckCircle2,
  ArrowRight,
  Loader2,
  AlertTriangle,
  RefreshCw,
} from "lucide-react";
import { api, AppConfig, DetectedDisk } from "../api";

interface Props {
  onComplete: () => void;
}

type Step = "welcome" | "primary" | "backup" | "saving" | "done";

export default function SetupWizard({ onComplete }: Props) {
  const [step, setStep] = useState<Step>("welcome");
  const [disks, setDisks] = useState<DetectedDisk[]>([]);
  const [loadingDisks, setLoadingDisks] = useState(false);
  const [primaryUuid, setPrimaryUuid] = useState("");
  const [backupUuid, setBackupUuid] = useState("");
  const [primaryLabel, setPrimaryLabel] = useState("Primary");
  const [backupLabel, setBackupLabel] = useState("Backup");
  const [error, setError] = useState("");
  const [config, setConfig] = useState<AppConfig | null>(null);

  const loadDisks = async () => {
    setLoadingDisks(true);
    setError("");
    try {
      const [detected, cfg] = await Promise.all([api.detectDisks(), api.getConfig()]);
      setDisks(detected);
      setConfig(cfg);
      // Auto-select boot disk as primary
      const boot = detected.find((d) => d.is_boot);
      if (boot) {
        setPrimaryUuid(boot.uuid);
        setPrimaryLabel(boot.model || "Primary");
      }
    } catch (e) {
      setError(String(e));
    }
    setLoadingDisks(false);
  };

  const handleSave = async () => {
    if (!config || !primaryUuid || !backupUuid) return;
    setStep("saving");
    try {
      const newConfig: AppConfig = {
        ...config,
        disks: {
          primary_uuid: primaryUuid,
          primary_label: primaryLabel,
          backup_uuid: backupUuid,
          backup_label: backupLabel,
        },
      };
      await api.saveConfig(newConfig);
      setStep("done");
    } catch (e) {
      setError(String(e));
      setStep("backup");
    }
  };

  const diskCard = (
    disk: DetectedDisk,
    selected: boolean,
    onSelect: () => void,
    disabled?: boolean
  ) => (
    <button
      key={disk.uuid}
      onClick={onSelect}
      disabled={disabled}
      className={`w-full text-left rounded-xl border p-4 transition-all ${
        selected
          ? "border-cyan-500 bg-cyan-500/10"
          : disabled
          ? "border-zinc-800 bg-zinc-900/30 opacity-40 cursor-not-allowed"
          : "border-zinc-700 bg-zinc-900/60 hover:border-zinc-500 hover:bg-zinc-800/60 cursor-pointer"
      }`}
    >
      <div className="flex items-center gap-3">
        <HardDrive
          className={`w-6 h-6 shrink-0 ${selected ? "text-cyan-400" : "text-zinc-500"}`}
        />
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            <span className="font-semibold text-sm truncate">
              {disk.model || disk.device}
            </span>
            {disk.is_boot && (
              <span className="text-[10px] bg-cyan-500/20 text-cyan-400 border border-cyan-500/30 rounded px-1.5 py-0.5 shrink-0">
                Boot
              </span>
            )}
          </div>
          <div className="text-xs text-zinc-500 mt-0.5 font-mono truncate">{disk.device}</div>
          <div className="flex items-center gap-3 mt-1 text-xs text-zinc-500">
            <span>{disk.size}</span>
            {disk.mountpoint && <span className="text-zinc-600">{disk.mountpoint}</span>}
          </div>
          <div className="text-[10px] text-zinc-600 mt-0.5 font-mono">{disk.uuid}</div>
        </div>
        {selected && <CheckCircle2 className="w-5 h-5 text-cyan-400 shrink-0" />}
      </div>
    </button>
  );

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 backdrop-blur-sm">
      <div className="w-full max-w-lg mx-4 bg-zinc-900 border border-zinc-700 rounded-2xl shadow-2xl overflow-hidden">
        {/* Header */}
        <div className="bg-gradient-to-r from-cyan-900/40 to-zinc-900/40 border-b border-zinc-800 px-6 py-5 flex items-center gap-3">
          <Shield className="w-7 h-7 text-cyan-400" />
          <div>
            <h2 className="text-lg font-bold">arclight einrichten</h2>
            <p className="text-xs text-zinc-400">
              {step === "welcome" && "Willkommen — Erster Start"}
              {step === "primary" && "Schritt 1 von 2 — Primäre Disk wählen"}
              {step === "backup" && "Schritt 2 von 2 — Backup-Disk wählen"}
              {step === "saving" && "Konfiguration wird gespeichert..."}
              {step === "done" && "Einrichtung abgeschlossen!"}
            </p>
          </div>
        </div>

        <div className="px-6 py-6">
          {/* Welcome */}
          {step === "welcome" && (
            <div className="space-y-4">
              <p className="text-zinc-300 text-sm leading-relaxed">
                arclight sichert dein System auf eine zweite NVMe-Disk — so dass du
                direkt von der Backup-Disk booten kannst, falls die primäre Disk
                ausfällt.
              </p>
              <div className="grid grid-cols-3 gap-3 mt-4">
                {[
                  { label: "Snapshots", desc: "via Snapper" },
                  { label: "NVMe Sync", desc: "via rsync" },
                  { label: "Bootbar", desc: "automatisch" },
                ].map((f) => (
                  <div
                    key={f.label}
                    className="rounded-lg bg-zinc-800/60 border border-zinc-700 p-3 text-center"
                  >
                    <div className="text-sm font-semibold text-cyan-400">{f.label}</div>
                    <div className="text-xs text-zinc-500 mt-0.5">{f.desc}</div>
                  </div>
                ))}
              </div>
              <p className="text-xs text-zinc-500">
                Du brauchst zwei NVMe/SATA-Laufwerke. Du kannst die Konfiguration
                jederzeit unter <span className="text-zinc-300">Einstellungen</span> anpassen.
              </p>
            </div>
          )}

          {/* Primary Disk */}
          {step === "primary" && (
            <div className="space-y-3">
              <p className="text-sm text-zinc-400 mb-3">
                Wähle die Disk, von der du aktuell bootest. Sie wird als{" "}
                <span className="text-cyan-400">Primary</span> behandelt und als
                Quelle für alle Backups genutzt.
              </p>
              {loadingDisks ? (
                <div className="flex items-center gap-2 text-zinc-500 py-4 justify-center">
                  <Loader2 className="w-4 h-4 animate-spin" />
                  <span className="text-sm">Laufwerke werden erkannt...</span>
                </div>
              ) : (
                <div className="space-y-2 max-h-64 overflow-y-auto pr-1">
                  {disks.map((d) =>
                    diskCard(d, d.uuid === primaryUuid, () => {
                      setPrimaryUuid(d.uuid);
                      setPrimaryLabel(d.model || d.device);
                    })
                  )}
                </div>
              )}
              {error && (
                <p className="text-red-400 text-xs flex items-center gap-1">
                  <AlertTriangle className="w-3 h-3" /> {error}
                </p>
              )}
            </div>
          )}

          {/* Backup Disk */}
          {step === "backup" && (
            <div className="space-y-3">
              <p className="text-sm text-zinc-400 mb-3">
                Wähle die zweite Disk als{" "}
                <span className="text-emerald-400">Backup</span>. Ihr Inhalt wird
                beim Sync vollständig überschrieben.
              </p>
              <div className="space-y-2 max-h-64 overflow-y-auto pr-1">
                {disks.map((d) =>
                  diskCard(
                    d,
                    d.uuid === backupUuid,
                    () => {
                      setBackupUuid(d.uuid);
                      setBackupLabel(d.model || d.device);
                    },
                    d.uuid === primaryUuid
                  )
                )}
              </div>
              {error && (
                <p className="text-red-400 text-xs flex items-center gap-1">
                  <AlertTriangle className="w-3 h-3" /> {error}
                </p>
              )}
            </div>
          )}

          {/* Saving */}
          {step === "saving" && (
            <div className="flex flex-col items-center py-8 gap-4">
              <Loader2 className="w-10 h-10 text-cyan-400 animate-spin" />
              <p className="text-sm text-zinc-400">Konfiguration wird gespeichert...</p>
            </div>
          )}

          {/* Done */}
          {step === "done" && (
            <div className="flex flex-col items-center py-6 gap-4 text-center">
              <CheckCircle2 className="w-14 h-14 text-emerald-400" />
              <div>
                <h3 className="text-lg font-bold text-emerald-400">Bereit!</h3>
                <p className="text-sm text-zinc-400 mt-1 max-w-xs">
                  Gehe zu <span className="text-zinc-300">NVMe Sync</span> und
                  starte deinen ersten Sync. Einstellungen kannst du jederzeit
                  anpassen.
                </p>
              </div>
              <div className="grid grid-cols-2 gap-2 w-full text-xs">
                <div className="rounded-lg bg-zinc-800/60 border border-zinc-700 p-2">
                  <div className="text-zinc-500">Primary</div>
                  <div className="text-zinc-200 font-semibold truncate">{primaryLabel}</div>
                </div>
                <div className="rounded-lg bg-zinc-800/60 border border-zinc-700 p-2">
                  <div className="text-zinc-500">Backup</div>
                  <div className="text-zinc-200 font-semibold truncate">{backupLabel}</div>
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Footer buttons */}
        <div className="border-t border-zinc-800 px-6 py-4 flex justify-between items-center">
          {step !== "done" && step !== "saving" ? (
            <button
              onClick={onComplete}
              className="text-xs text-zinc-500 hover:text-zinc-300 transition-colors"
            >
              Überspringen
            </button>
          ) : (
            <div />
          )}

          <div className="flex gap-2">
            {step === "welcome" && (
              <button
                onClick={() => {
                  setStep("primary");
                  loadDisks();
                }}
                className="flex items-center gap-2 bg-cyan-600 hover:bg-cyan-500 text-white text-sm font-semibold px-4 py-2 rounded-lg transition-colors"
              >
                Einrichten <ArrowRight className="w-4 h-4" />
              </button>
            )}

            {step === "primary" && (
              <>
                <button
                  onClick={() => {
                    setLoadingDisks(true);
                    loadDisks();
                  }}
                  className="flex items-center gap-1.5 text-zinc-400 hover:text-zinc-200 text-sm px-3 py-2 rounded-lg border border-zinc-700 hover:border-zinc-500 transition-colors"
                >
                  <RefreshCw className="w-3.5 h-3.5" /> Aktualisieren
                </button>
                <button
                  disabled={!primaryUuid}
                  onClick={() => setStep("backup")}
                  className="flex items-center gap-2 bg-cyan-600 hover:bg-cyan-500 disabled:bg-zinc-700 disabled:text-zinc-500 text-white text-sm font-semibold px-4 py-2 rounded-lg transition-colors"
                >
                  Weiter <ArrowRight className="w-4 h-4" />
                </button>
              </>
            )}

            {step === "backup" && (
              <>
                <button
                  onClick={() => setStep("primary")}
                  className="text-zinc-400 hover:text-zinc-200 text-sm px-3 py-2 rounded-lg border border-zinc-700 hover:border-zinc-500 transition-colors"
                >
                  Zurück
                </button>
                <button
                  disabled={!backupUuid || backupUuid === primaryUuid}
                  onClick={handleSave}
                  className="flex items-center gap-2 bg-emerald-600 hover:bg-emerald-500 disabled:bg-zinc-700 disabled:text-zinc-500 text-white text-sm font-semibold px-4 py-2 rounded-lg transition-colors"
                >
                  <CheckCircle2 className="w-4 h-4" /> Speichern
                </button>
              </>
            )}

            {step === "done" && (
              <button
                onClick={onComplete}
                className="flex items-center gap-2 bg-cyan-600 hover:bg-cyan-500 text-white text-sm font-semibold px-4 py-2 rounded-lg transition-colors"
              >
                Loslegen <ArrowRight className="w-4 h-4" />
              </button>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

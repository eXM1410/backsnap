import { useEffect, useState, useCallback } from "react";
import {
  Cpu,
  Thermometer,
  HardDrive,
  Wifi,
  WifiOff,
  RefreshCw,
  Power,
  RotateCcw,
  TerminalSquare,
  Server,
  MemoryStick,
  Activity,
  FolderSync,
  ChevronDown,
  ChevronUp,
  Send,
  CircleDot,
  Plus,
  Monitor,
  Trash2,
  Pencil,
  X,
  Loader2,
  CheckCircle2,
  AlertTriangle,
} from "lucide-react";
import { api, PiStatus, PiDevice, PiActionResult, PiTestResult, apiError } from "../api";
import { Card, Badge, PageHeader, Loading, Button } from "../components/ui";

// ─── Helpers ──────────────────────────────────────────────────

/** Returns true when the window is narrower than the given breakpoint (default 700px). */
function useCompact(breakpoint = 700) {
  const [compact, setCompact] = useState(() => window.innerWidth < breakpoint);
  useEffect(() => {
    const onResize = () => setCompact(window.innerWidth < breakpoint);
    window.addEventListener("resize", onResize);
    return () => window.removeEventListener("resize", onResize);
  }, [breakpoint]);
  return compact;
}

function tempColor(t: number) {
  return t >= 75 ? "text-red-400" : t >= 60 ? "text-amber-400" : "text-emerald-400";
}

function usageColor(p: number) {
  return p >= 90 ? "red" : p >= 70 ? "amber" : "cyan";
}

function RingGauge({
  value,
  size = 64,
  stroke = 5,
  color = "cyan",
  children,
}: {
  value: number;
  size?: number;
  stroke?: number;
  color?: string;
  children?: React.ReactNode;
}) {
  const pct = Math.min(value, 100);
  const r = (size - stroke) / 2;
  const circ = 2 * Math.PI * r;
  const offset = circ - (pct / 100) * circ;
  const sc: Record<string, string> = {
    cyan: "stroke-cyan-500",
    emerald: "stroke-emerald-500",
    amber: "stroke-amber-500",
    red: "stroke-red-500",
  };
  return (
    <div className="relative inline-flex items-center justify-center" style={{ width: size, height: size }}>
      <svg width={size} height={size} className="-rotate-90">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={stroke} className="stroke-zinc-800" />
        <circle
          cx={size / 2}
          cy={size / 2}
          r={r}
          fill="none"
          strokeWidth={stroke}
          className={`${sc[color] || "stroke-cyan-500"} transition-all duration-700`}
          strokeDasharray={circ}
          strokeDashoffset={offset}
          strokeLinecap="round"
        />
      </svg>
      <div className="absolute inset-0 flex items-center justify-center">{children}</div>
    </div>
  );
}

const inputCls =
  "w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-cyan-500/50";

// ─── Add/Edit Pi Wizard ──────────────────────────────────────

type WizardStep = "connection" | "details" | "services" | "done";

interface WizardProps {
  editDevice?: PiDevice | null;
  onClose: () => void;
  onSaved: () => void;
}

function emptyDevice(): PiDevice {
  return {
    id: "",
    label: "",
    model: "",
    ip: "",
    user: "max",
    ssh_key: "~/.ssh/id_ed25519",
    mount_point: "",
    remote_protocol: "rdp",
    remote_port: 3389,
    rdp_password: "",
    watch_services: ["ssh", "xrdp", "nfs-server"],
  };
}

function PiWizard({ editDevice, onClose, onSaved }: WizardProps) {
  const isEdit = !!editDevice;
  const [step, setStep] = useState<WizardStep>("connection");
  const [device, setDevice] = useState<PiDevice>(editDevice ? { ...editDevice } : emptyDevice());
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<PiTestResult | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [svcInput, setSvcInput] = useState("");

  const upd = (patch: Partial<PiDevice>) => setDevice((d) => ({ ...d, ...patch }));

  const handleTest = async () => {
    setTesting(true);
    setTestResult(null);
    setError("");
    try {
      const res = await api.testPiConnection(device.ip, device.user, device.ssh_key);
      setTestResult(res);
      if (res.ssh_ok) {
        // Auto-fill from detected info
        if (res.model && !device.model) upd({ model: res.model });
        if (res.hostname && !device.label) upd({ label: res.hostname });
        if (!device.id) {
          upd({ id: device.ip.replace(/\./g, "-") });
        }
      }
    } catch (e) {
      setError(apiError(e));
    }
    setTesting(false);
  };

  const handleSave = async () => {
    if (!device.id || !device.ip || !device.user) {
      setError("ID, IP und User sind Pflichtfelder");
      return;
    }
    setSaving(true);
    setError("");
    try {
      await api.addPiDevice(device);
      setStep("done");
    } catch (e) {
      setError(apiError(e));
    }
    setSaving(false);
  };

  const addService = () => {
    const s = svcInput.trim();
    if (s && !device.watch_services.includes(s)) {
      upd({ watch_services: [...device.watch_services, s] });
    }
    setSvcInput("");
  };

  const removeService = (name: string) => {
    upd({ watch_services: device.watch_services.filter((s) => s !== name) });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm">
      <div className="bg-zinc-900 border border-zinc-700 rounded-2xl w-full max-w-lg mx-4 shadow-2xl">
        {/* Wizard Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-zinc-800">
          <h2 className="text-lg font-semibold">
            {isEdit ? "Pi bearbeiten" : "Neuen Pi hinzufügen"}
          </h2>
          <button onClick={onClose} className="text-zinc-500 hover:text-zinc-300 transition-colors">
            <X className="w-5 h-5" />
          </button>
        </div>

        {/* Step Indicator */}
        {step !== "done" && (
          <div className="flex items-center gap-2 px-6 pt-4">
            {(["connection", "details", "services"] as WizardStep[]).map((s, i) => (
              <div key={s} className="flex items-center gap-2">
                <div
                  className={`w-7 h-7 rounded-full flex items-center justify-center text-xs font-bold
                    ${step === s ? "bg-cyan-500 text-white" : "bg-zinc-800 text-zinc-500"}`}
                >
                  {i + 1}
                </div>
                {i < 2 && <div className="w-8 h-px bg-zinc-700" />}
              </div>
            ))}
            <span className="text-xs text-zinc-500 ml-2">
              {step === "connection" ? "Verbindung" : step === "details" ? "Details" : "Services"}
            </span>
          </div>
        )}

        <div className="px-6 py-5 space-y-4">
          {/* ── Step 1: Connection ── */}
          {step === "connection" && (
            <>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">IP-Adresse *</label>
                <input
                  value={device.ip}
                  onChange={(e) => upd({ ip: e.target.value })}
                  placeholder="192.168.0.8"
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">Benutzer *</label>
                <input
                  value={device.user}
                  onChange={(e) => upd({ user: e.target.value })}
                  placeholder="max"
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">SSH-Key (leer = Standard)</label>
                <input
                  value={device.ssh_key}
                  onChange={(e) => upd({ ssh_key: e.target.value })}
                  placeholder="~/.ssh/id_ed25519"
                  className={inputCls}
                />
              </div>

              <Button onClick={handleTest} loading={testing} disabled={!device.ip || !device.user}>
                <Wifi className="w-4 h-4" /> Verbindung testen
              </Button>

              {testResult && (
                <div
                  className={`flex items-start gap-3 p-3 rounded-lg border text-sm ${
                    testResult.ssh_ok
                      ? "bg-emerald-500/5 border-emerald-500/20 text-emerald-400"
                      : "bg-red-500/5 border-red-500/20 text-red-400"
                  }`}
                >
                  {testResult.ssh_ok ? (
                    <CheckCircle2 className="w-5 h-5 mt-0.5 shrink-0" />
                  ) : (
                    <AlertTriangle className="w-5 h-5 mt-0.5 shrink-0" />
                  )}
                  <div>
                    {testResult.ssh_ok ? (
                      <>
                        <p className="font-medium">Verbindung erfolgreich!</p>
                        {testResult.hostname && <p className="text-xs mt-1">Hostname: {testResult.hostname}</p>}
                        {testResult.model && <p className="text-xs">Modell: {testResult.model}</p>}
                        {testResult.kernel && <p className="text-xs">Kernel: {testResult.kernel}</p>}
                      </>
                    ) : (
                      <>
                        <p className="font-medium">
                          {!testResult.reachable ? "Nicht erreichbar" : "SSH fehlgeschlagen"}
                        </p>
                        {testResult.error && <p className="text-xs mt-1">{testResult.error}</p>}
                      </>
                    )}
                  </div>
                </div>
              )}

              {error && <p className="text-sm text-red-400">{error}</p>}

              <div className="flex justify-end gap-2 pt-2">
                <Button variant="secondary" size="sm" onClick={onClose}>
                  Abbrechen
                </Button>
                <Button
                  size="sm"
                  onClick={() => setStep("details")}
                  disabled={!device.ip || !device.user}
                >
                  Weiter
                </Button>
              </div>
            </>
          )}

          {/* ── Step 2: Details ── */}
          {step === "details" && (
            <>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">ID (eindeutig) *</label>
                <input
                  value={device.id}
                  onChange={(e) => upd({ id: e.target.value })}
                  placeholder="z.B. pi5"
                  className={inputCls}
                  disabled={isEdit}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">Bezeichnung</label>
                <input
                  value={device.label}
                  onChange={(e) => upd({ label: e.target.value })}
                  placeholder="Raspberry Pi 5"
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">Modell</label>
                <input
                  value={device.model}
                  onChange={(e) => upd({ model: e.target.value })}
                  placeholder="Raspberry Pi 5 Model B Rev 1.0"
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">NFS Mount Point (optional)</label>
                <input
                  value={device.mount_point}
                  onChange={(e) => upd({ mount_point: e.target.value })}
                  placeholder="/home/max/Pi5"
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">Protokoll</label>
                <div className="flex gap-2">
                  <button
                    onClick={() => upd({ remote_protocol: "rdp", remote_port: device.remote_port === 5900 ? 3389 : device.remote_port, watch_services: device.watch_services.map(s => s === "wayvnc" ? "xrdp" : s) })}
                    className={`flex-1 py-1.5 rounded-lg text-sm font-medium transition-colors ${
                      device.remote_protocol === "rdp"
                        ? "bg-purple-500/20 text-purple-300 ring-1 ring-purple-500/40"
                        : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
                    }`}
                  >
                    RDP
                  </button>
                  <button
                    onClick={() => upd({ remote_protocol: "vnc", remote_port: device.remote_port === 3389 ? 5900 : device.remote_port, watch_services: device.watch_services.map(s => s === "xrdp" ? "wayvnc" : s) })}
                    className={`flex-1 py-1.5 rounded-lg text-sm font-medium transition-colors ${
                      device.remote_protocol === "vnc"
                        ? "bg-cyan-500/20 text-cyan-300 ring-1 ring-cyan-500/40"
                        : "bg-zinc-800 text-zinc-400 hover:bg-zinc-700"
                    }`}
                  >
                    VNC
                  </button>
                </div>
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">{device.remote_protocol === "vnc" ? "VNC" : "RDP"} Port (0 = deaktiviert)</label>
                <input
                  type="number"
                  value={device.remote_port}
                  onChange={(e) => upd({ remote_port: parseInt(e.target.value) || 0 })}
                  className={inputCls}
                />
              </div>
              <div>
                <label className="text-xs text-zinc-400 mb-1 block">{device.remote_protocol === "vnc" ? "VNC" : "RDP"} Passwort (für Auto-Login)</label>
                <input
                  type="password"
                  value={device.rdp_password}
                  onChange={(e) => upd({ rdp_password: e.target.value })}
                  placeholder="Leer = manueller Login"
                  className={inputCls}
                />
              </div>

              {error && <p className="text-sm text-red-400">{error}</p>}

              <div className="flex justify-between pt-2">
                <Button variant="secondary" size="sm" onClick={() => setStep("connection")}>
                  Zurück
                </Button>
                <Button size="sm" onClick={() => setStep("services")} disabled={!device.id}>
                  Weiter
                </Button>
              </div>
            </>
          )}

          {/* ── Step 3: Services ── */}
          {step === "services" && (
            <>
              <div>
                <label className="text-xs text-zinc-400 mb-2 block">Überwachte Services</label>
                <div className="flex flex-wrap gap-1.5 mb-3">
                  {device.watch_services.map((svc) => (
                    <span
                      key={svc}
                      className="inline-flex items-center gap-1 bg-zinc-800 text-zinc-300 text-xs px-2 py-1 rounded-lg"
                    >
                      {svc}
                      <button
                        onClick={() => removeService(svc)}
                        className="hover:text-red-400 transition-colors"
                      >
                        <X className="w-3 h-3" />
                      </button>
                    </span>
                  ))}
                </div>
                <div className="flex gap-2">
                  <input
                    value={svcInput}
                    onChange={(e) => setSvcInput(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && addService()}
                    placeholder="Service hinzufügen..."
                    className={inputCls}
                  />
                  <Button size="sm" variant="secondary" onClick={addService}>
                    <Plus className="w-4 h-4" />
                  </Button>
                </div>
              </div>

              {/* Summary */}
              <div className="bg-zinc-800/40 rounded-lg p-3 text-xs space-y-1">
                <p className="text-zinc-400 font-medium mb-2">Zusammenfassung</p>
                <p>
                  <span className="text-zinc-500">ID:</span> <span className="text-zinc-200">{device.id}</span>
                </p>
                <p>
                  <span className="text-zinc-500">IP:</span> <span className="text-zinc-200">{device.ip}</span>
                </p>
                <p>
                  <span className="text-zinc-500">User:</span> <span className="text-zinc-200">{device.user}</span>
                </p>
                {device.label && (
                  <p>
                    <span className="text-zinc-500">Label:</span>{" "}
                    <span className="text-zinc-200">{device.label}</span>
                  </p>
                )}
                {device.mount_point && (
                  <p>
                    <span className="text-zinc-500">NFS:</span>{" "}
                    <span className="text-zinc-200">{device.mount_point}</span>
                  </p>
                )}
                <p>
                  <span className="text-zinc-500">Remote:</span>{" "}
                  <span className="text-zinc-200">{device.remote_protocol.toUpperCase()} : {device.remote_port || "deaktiviert"}</span>
                </p>
              </div>

              {error && <p className="text-sm text-red-400">{error}</p>}

              <div className="flex justify-between pt-2">
                <Button variant="secondary" size="sm" onClick={() => setStep("details")}>
                  Zurück
                </Button>
                <Button size="sm" onClick={handleSave} loading={saving}>
                  <CheckCircle2 className="w-4 h-4" /> Speichern
                </Button>
              </div>
            </>
          )}

          {/* ── Done ── */}
          {step === "done" && (
            <div className="text-center py-6">
              <CheckCircle2 className="w-12 h-12 text-emerald-400 mx-auto mb-3" />
              <p className="text-lg font-semibold mb-1">
                {isEdit ? "Pi aktualisiert!" : "Pi hinzugefügt!"}
              </p>
              <p className="text-sm text-zinc-500 mb-4">{device.label || device.id}</p>
              <Button
                onClick={() => {
                  onSaved();
                  onClose();
                }}
              >
                Fertig
              </Button>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

// ─── Pi Card ──────────────────────────────────────────────────

function PiCard({
  pi,
  onRefresh,
  onEdit,
  onRemove,
  onRdp,
}: {
  pi: PiStatus;
  onRefresh: (id: string) => void;
  onEdit: (id: string) => void;
  onRemove: (id: string) => void;
  onRdp: (id: string) => void;
}) {
  const [expanded, setExpanded] = useState(true);
  const [cmdInput, setCmdInput] = useState("");
  const [cmdResult, setCmdResult] = useState<PiActionResult | null>(null);
  const [cmdLoading, setCmdLoading] = useState(false);
  const [actionLoading, setActionLoading] = useState<string | null>(null);
  const compact = useCompact();

  const handleAction = async (action: "reboot" | "shutdown") => {
    if (!confirm(`${pi.label} wirklich ${action === "reboot" ? "neustarten" : "herunterfahren"}?`)) return;
    setActionLoading(action);
    try {
      const fn = action === "reboot" ? api.piReboot : api.piShutdown;
      const result = await fn(pi.id);
      setCmdResult(result);
    } catch (e) {
      setCmdResult({ success: false, message: String(e) });
    }
    setActionLoading(null);
  };

  const handleRunCmd = async () => {
    if (!cmdInput.trim()) return;
    setCmdLoading(true);
    setCmdResult(null);
    try {
      const result = await api.piRunCommand(pi.id, cmdInput);
      setCmdResult(result);
    } catch (e) {
      setCmdResult({ success: false, message: String(e) });
    }
    setCmdLoading(false);
  };

  const memPercent =
    pi.mem_total_mb && pi.mem_used_mb ? Math.round((pi.mem_used_mb / pi.mem_total_mb) * 100) : 0;

  return (
    <Card className="!p-0 overflow-hidden">
      {/* Header */}
      <div
        className="flex items-center justify-between px-3 sm:px-5 py-3 sm:py-4 cursor-pointer hover:bg-zinc-800/30 transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-2 sm:gap-3 min-w-0 flex-1">
          <div className={`p-1.5 sm:p-2 rounded-lg shrink-0 ${pi.online ? "bg-emerald-500/10" : "bg-zinc-800"}`}>
            <Server className={`w-4 h-4 sm:w-5 sm:h-5 ${pi.online ? "text-emerald-400" : "text-zinc-600"}`} />
          </div>
          <div className="min-w-0">
            <div className="flex items-center gap-1.5 sm:gap-2 flex-wrap">
              <span className="font-semibold text-sm sm:text-base truncate">{pi.label || pi.id}</span>
              <Badge color={pi.online ? "green" : "red"}>{pi.online ? "Online" : "Offline"}</Badge>
              {pi.nfs_mounted && <Badge color="cyan">NFS</Badge>}
              {pi.remote_port > 0 && <Badge color={pi.remote_protocol === "vnc" ? "cyan" : "purple"}>{pi.remote_protocol.toUpperCase()}</Badge>}
            </div>
            <div className="text-[11px] sm:text-xs text-zinc-500 mt-0.5 truncate">
              {pi.ip}
              {pi.hostname ? ` — ${pi.hostname}` : ""}
              {pi.kernel ? ` — ${pi.kernel}` : ""}
            </div>
          </div>
        </div>
        <div className="flex items-center gap-1 sm:gap-1.5 shrink-0 ml-2">
          {pi.online && pi.cpu_temp != null && (
            <span className={`text-sm font-mono mr-1 ${tempColor(pi.cpu_temp)}`}>
              {pi.cpu_temp.toFixed(1)}°C
            </span>
          )}
          <button
            onClick={(e) => { e.stopPropagation(); onEdit(pi.id); }}
            className="p-1.5 rounded-lg hover:bg-zinc-700/50 text-zinc-500 hover:text-zinc-300 transition-colors"
            title="Bearbeiten"
          >
            <Pencil className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={(e) => { e.stopPropagation(); onRefresh(pi.id); }}
            className="p-1.5 rounded-lg hover:bg-zinc-700/50 text-zinc-500 hover:text-zinc-300 transition-colors"
            title="Aktualisieren"
          >
            <RefreshCw className="w-4 h-4" />
          </button>
          {expanded ? (
            <ChevronUp className="w-4 h-4 text-zinc-500" />
          ) : (
            <ChevronDown className="w-4 h-4 text-zinc-500" />
          )}
        </div>
      </div>

      {/* Expanded Content — Online */}
      {expanded && pi.online && (
        <div className="border-t border-zinc-800 px-3 sm:px-5 py-3 sm:py-4 space-y-3 sm:space-y-4">
          {/* Gauges Row */}
          <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 sm:gap-4">
            <div className="flex flex-col items-center gap-1">
              <RingGauge value={pi.cpu_usage ?? 0} size={compact ? 48 : 64} stroke={compact ? 4 : 5} color={usageColor(pi.cpu_usage ?? 0)}>
                <span className="text-[10px] sm:text-xs font-bold">{(pi.cpu_usage ?? 0).toFixed(0)}%</span>
              </RingGauge>
              <div className="flex items-center gap-1 text-[10px] sm:text-xs text-zinc-500">
                <Cpu className="w-3 h-3" /> CPU
              </div>
              {pi.cpu_freq_mhz != null && (
                <span className="text-[10px] text-zinc-600">
                  {pi.cpu_freq_mhz >= 1000 ? `${(pi.cpu_freq_mhz / 1000).toFixed(1)} GHz` : `${pi.cpu_freq_mhz} MHz`}
                </span>
              )}
            </div>

            <div className="flex flex-col items-center gap-1">
              <RingGauge value={memPercent} size={compact ? 48 : 64} stroke={compact ? 4 : 5} color={usageColor(memPercent)}>
                <span className="text-[10px] sm:text-xs font-bold">{memPercent}%</span>
              </RingGauge>
              <div className="flex items-center gap-1 text-[10px] sm:text-xs text-zinc-500">
                <MemoryStick className="w-3 h-3" /> RAM
              </div>
              {pi.mem_total_mb && pi.mem_used_mb && (
                <span className="text-[10px] text-zinc-600">
                  {pi.mem_used_mb} / {pi.mem_total_mb} MB
                </span>
              )}
            </div>

            <div className="flex flex-col items-center gap-1">
              <RingGauge
                value={pi.cpu_temp ?? 0}
                size={compact ? 48 : 64}
                stroke={compact ? 4 : 5}
                color={pi.cpu_temp != null ? (pi.cpu_temp >= 75 ? "red" : pi.cpu_temp >= 60 ? "amber" : "emerald") : "cyan"}
              >
                <span className="text-[10px] sm:text-xs font-bold">
                  {pi.cpu_temp != null ? `${pi.cpu_temp.toFixed(0)}°` : "—"}
                </span>
              </RingGauge>
              <div className="flex items-center gap-1 text-[10px] sm:text-xs text-zinc-500">
                <Thermometer className="w-3 h-3" /> Temp
              </div>
            </div>

            <div className="flex flex-col items-center gap-1">
              <RingGauge value={pi.disk_percent ?? 0} size={compact ? 48 : 64} stroke={compact ? 4 : 5} color={usageColor(pi.disk_percent ?? 0)}>
                <span className="text-[10px] sm:text-xs font-bold">{pi.disk_percent ?? 0}%</span>
              </RingGauge>
              <div className="flex items-center gap-1 text-[10px] sm:text-xs text-zinc-500">
                <HardDrive className="w-3 h-3" /> Disk
              </div>
              {pi.disk_total_gb != null && pi.disk_used_gb != null && (
                <span className="text-[10px] text-zinc-600">
                  {pi.disk_used_gb}G / {pi.disk_total_gb}G
                </span>
              )}
            </div>
          </div>

          {/* Info Row */}
          <div className="grid grid-cols-1 sm:grid-cols-2 gap-2 sm:gap-3 text-xs">
            <div className="flex items-center gap-2 bg-zinc-800/40 rounded-lg px-3 py-2">
              <Activity className="w-3.5 h-3.5 text-zinc-500" />
              <span className="text-zinc-400">Uptime:</span>
              <span className="text-zinc-200">{pi.uptime || "—"}</span>
            </div>
            <div className="flex items-center gap-2 bg-zinc-800/40 rounded-lg px-3 py-2">
              <FolderSync className="w-3.5 h-3.5 text-zinc-500" />
              <span className="text-zinc-400">NFS:</span>
              <span className={pi.nfs_mounted ? "text-emerald-400" : "text-zinc-500"}>
                {pi.nfs_mounted ? "Gemountet" : "Nicht gemountet"}
              </span>
            </div>
          </div>

          {/* Throttled */}
          {pi.throttled && (
            <div className="text-xs bg-zinc-800/40 rounded-lg px-3 py-2 flex items-center gap-2">
              <CircleDot className="w-3.5 h-3.5 text-zinc-500" />
              <span className="text-zinc-400">Throttled:</span>
              <span className={pi.throttled === "throttled=0x0" ? "text-emerald-400" : "text-amber-400"}>
                {pi.throttled}
              </span>
            </div>
          )}

          {/* Services */}
          {pi.services.length > 0 && (
            <div>
              <div className="text-xs text-zinc-500 mb-2 font-medium">Services</div>
              <div className="flex flex-wrap gap-1.5">
                {pi.services.map((svc) => (
                  <Badge key={svc.name} color={svc.active ? "green" : "zinc"}>
                    {svc.name}
                    {svc.active ? " ✓" : " ✗"}
                  </Badge>
                ))}
              </div>
            </div>
          )}

          {/* Actions */}
          <div className="flex items-center gap-1.5 sm:gap-2 pt-1 flex-wrap">
            {pi.remote_port > 0 && (
              <Button
                size="sm"
                onClick={() => onRdp(pi.id)}
              >
                <Monitor className="w-3.5 h-3.5" /> Remote Desktop
              </Button>
            )}
            <Button
              size="sm"
              variant="secondary"
              onClick={() => handleAction("reboot")}
              loading={actionLoading === "reboot"}
              disabled={!!actionLoading}
            >
              <RotateCcw className="w-3.5 h-3.5" /> Neustart
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() => handleAction("shutdown")}
              loading={actionLoading === "shutdown"}
              disabled={!!actionLoading}
            >
              <Power className="w-3.5 h-3.5" /> Herunterfahren
            </Button>
            <Button
              size="sm"
              variant="danger"
              onClick={() => onRemove(pi.id)}
            >
              <Trash2 className="w-3.5 h-3.5" /> Entfernen
            </Button>
          </div>

          {/* Remote Terminal */}
          <div>
            <div className="text-xs text-zinc-500 mb-2 font-medium flex items-center gap-1">
              <TerminalSquare className="w-3.5 h-3.5" /> Remote Befehl
            </div>
            <div className="flex gap-2">
              <input
                value={cmdInput}
                onChange={(e) => setCmdInput(e.target.value)}
                onKeyDown={(e) => e.key === "Enter" && handleRunCmd()}
                placeholder="z.B. vcgencmd measure_temp"
                className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-200 placeholder:text-zinc-600 focus:outline-none focus:border-cyan-500/50"
              />
              <Button size="sm" onClick={handleRunCmd} loading={cmdLoading} disabled={cmdLoading}>
                <Send className="w-3.5 h-3.5" />
              </Button>
            </div>
            {cmdResult && (
              <pre
                className={`mt-2 text-xs font-mono p-3 rounded-lg border max-h-40 overflow-auto ${
                  cmdResult.success
                    ? "bg-zinc-800/60 border-zinc-700 text-zinc-300"
                    : "bg-red-500/5 border-red-500/20 text-red-400"
                }`}
              >
                {cmdResult.message || "(keine Ausgabe)"}
              </pre>
            )}
          </div>
        </div>
      )}

      {/* Offline expanded */}
      {expanded && !pi.online && (
        <div className="border-t border-zinc-800 px-3 sm:px-5 py-4 sm:py-6 text-center">
          <WifiOff className="w-10 h-10 text-zinc-700 mx-auto mb-2" />
          <p className="text-sm text-zinc-500">
            {pi.label || pi.id} ist nicht erreichbar ({pi.ip})
          </p>
          <p className="text-xs text-zinc-600 mt-1">
            {pi.nfs_mounted ? "NFS ist noch gemountet (Cache)" : "NFS nicht gemountet"}
          </p>
          <div className="flex items-center justify-center gap-2 mt-3">
            <Button size="sm" variant="secondary" onClick={() => onEdit(pi.id)}>
              <Pencil className="w-3.5 h-3.5" /> Bearbeiten
            </Button>
            <Button size="sm" variant="danger" onClick={() => onRemove(pi.id)}>
              <Trash2 className="w-3.5 h-3.5" /> Entfernen
            </Button>
          </div>
        </div>
      )}
    </Card>
  );
}

// ─── Main Page ────────────────────────────────────────────────

export default function PiRemote() {
  const [pis, setPis] = useState<PiStatus[]>([]);
  const [devices, setDevices] = useState<PiDevice[]>([]);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [wizardOpen, setWizardOpen] = useState(false);
  const [editDevice, setEditDevice] = useState<PiDevice | null>(null);
  const [rdpMsg, setRdpMsg] = useState<string | null>(null);

  const fetchDevices = useCallback(async () => {
    try {
      const d = await api.getPiDevices();
      setDevices(d);
    } catch (e) {
      console.error("Pi devices fetch failed:", e);
    }
  }, []);

  const fetchAll = useCallback(async (showLoading = false) => {
    if (showLoading) setLoading(true);
    setRefreshing(true);
    try {
      const [data, devs] = await Promise.all([api.getPiStatusAll(), api.getPiDevices()]);
      setPis(data);
      setDevices(devs);
    } catch (e) {
      console.error("Pi status fetch failed:", e);
    }
    setLoading(false);
    setRefreshing(false);
  }, []);

  const refreshOne = useCallback(async (id: string) => {
    try {
      const updated = await api.getPiStatus(id);
      setPis((prev) => prev.map((p) => (p.id === id ? updated : p)));
    } catch (e) {
      console.error("Pi refresh failed:", e);
    }
  }, []);

  const handleEdit = (id: string) => {
    const dev = devices.find((d) => d.id === id);
    if (dev) {
      setEditDevice(dev);
      setWizardOpen(true);
    }
  };

  const handleRemove = async (id: string) => {
    const pi = pis.find((p) => p.id === id) || devices.find((d) => d.id === id);
    const name = pi?.label || id;
    if (!confirm(`${name} wirklich entfernen?`)) return;
    try {
      await api.removePiDevice(id);
      fetchAll(false);
    } catch (e) {
      console.error("Remove failed:", e);
    }
  };

  const handleRdp = async (id: string) => {
    setRdpMsg(null);
    try {
      const res = await api.openPiRemote(id);
      setRdpMsg(res.message);
      setTimeout(() => setRdpMsg(null), 4000);
    } catch (e) {
      setRdpMsg(apiError(e));
      setTimeout(() => setRdpMsg(null), 6000);
    }
  };

  useEffect(() => {
    fetchAll(true);
    const iv = setInterval(() => fetchAll(false), 2_000);
    return () => clearInterval(iv);
  }, [fetchAll]);

  const onlineCount = pis.filter((p) => p.online).length;

  return (
    <div className="p-3 sm:p-6 max-w-4xl mx-auto">
      <PageHeader
        title="Pi Remote"
        description={`${pis.length} Geräte — ${onlineCount} online`}
        actions={
          <div className="flex items-center gap-2">
            <Button size="sm" onClick={() => { setEditDevice(null); setWizardOpen(true); }}>
              <Plus className="w-4 h-4" /> Pi hinzufügen
            </Button>
            <Button variant="secondary" size="sm" onClick={() => fetchAll(false)} loading={refreshing}>
              <RefreshCw className="w-4 h-4" /> Aktualisieren
            </Button>
          </div>
        }
      />

      {/* RDP toast */}
      {rdpMsg && (
        <div className="mb-4 px-4 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-300">
          <Monitor className="w-4 h-4 inline mr-2" />
          {rdpMsg}
        </div>
      )}

      {loading ? (
        <Loading text="Raspberry Pis werden abgefragt..." />
      ) : (
        <div className="space-y-4">
          {pis.map((pi) => (
            <PiCard
              key={pi.id}
              pi={pi}
              onRefresh={refreshOne}
              onEdit={handleEdit}
              onRemove={handleRemove}
              onRdp={handleRdp}
            />
          ))}
          {pis.length === 0 && (
            <Card>
              <div className="text-center py-10">
                <Server className="w-12 h-12 text-zinc-700 mx-auto mb-3" />
                <p className="text-zinc-400 font-medium mb-2">Keine Pis konfiguriert</p>
                <p className="text-sm text-zinc-600 mb-4">
                  Füge deinen ersten Raspberry Pi hinzu, um ihn remote zu überwachen und zu steuern.
                </p>
                <Button onClick={() => { setEditDevice(null); setWizardOpen(true); }}>
                  <Plus className="w-4 h-4" /> Pi hinzufügen
                </Button>
              </div>
            </Card>
          )}
        </div>
      )}

      {/* Wizard Modal */}
      {wizardOpen && (
        <PiWizard
          editDevice={editDevice}
          onClose={() => { setWizardOpen(false); setEditDevice(null); }}
          onSaved={() => fetchAll(false)}
        />
      )}
    </div>
  );
}

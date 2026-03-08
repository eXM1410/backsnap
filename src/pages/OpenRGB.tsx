import { useEffect, useState, useCallback } from "react";
import {
  Lightbulb,
  Power,
  PowerOff,
  RefreshCw,
  Loader2,
  Layers,
  Palette,
  Cpu,
  Keyboard,
  Mouse,
  Disc,
  Fan,
  HardDrive,
  CheckCircle2,
  AlertTriangle,
} from "lucide-react";
import {
  api,
  RgbStatus,
  RgbDeviceInfo,
  apiError,
} from "../api";
import { Card, Badge, Button } from "../components/ui";

// ─── Device type icons ────────────────────────────────────────

const DEVICE_ICONS: Record<string, typeof Cpu> = {
  Motherboard: Cpu,
  Keyboard: Keyboard,
  Mouse: Mouse,
  Mousemat: Disc,
  Cooler: Fan,
  "LED Strip": Lightbulb,
  Case: HardDrive,
};


const EFFECTS = [
  { id: 0, name: "Aus" },
  { id: 1, name: "Statisch" },
  { id: 2, name: "Pulsieren" },
  { id: 3, name: "Blinken" },
  { id: 4, name: "Farbwechsel" },
  { id: 5, name: "Welle" },
  { id: 6, name: "Zufall" },
];


// ─── Component ────────────────────────────────────────────────

export default function OpenRGBSection() {
  const [status, setStatus] = useState<RgbStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [selectedDevice, setSelectedDevice] = useState<string | null>(null);
  const [rgbR, setRgbR] = useState(20);
  const [rgbG, setRgbG] = useState(0);
  const [rgbB, setRgbB] = useState(255);

  // ── helpers ──
  const showToast = useCallback((msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 3000);
  }, []);

  const loadStatus = useCallback(async () => {
    try {
      const s = await api.openrgbStatus();
      setStatus(s);
      setError(null);
      return s;
    } catch {
      setStatus({ connected: false, devices: [] });
      return null;
    } finally {
      setLoading(false);
    }
  }, []);

  // Auto-connect on mount if not already connected
  useEffect(() => {
    (async () => {
      const s = await loadStatus();
      if (!s?.connected) {
        try {
          const connected = await api.openrgbConnect();
          setStatus(connected);
          if (connected.devices.length > 0) setSelectedDevice(connected.devices[0].id);
        } catch {
          // silently ignore — status already shows disconnected
        }
      } else if (s.devices.length > 0 && !selectedDevice) {
        setSelectedDevice(s.devices[0].id);
      }
    })();
  }, []);  // eslint-disable-line react-hooks/exhaustive-deps

  // ── connect ──
  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await api.openrgbConnect();
      setStatus(s);
      showToast(`${s.devices.length} Geräte verbunden`);
      if (s.devices.length > 0) setSelectedDevice(s.devices[0].id);
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };

  const disconnect = async () => {
    setBusy(true);
    try {
      await api.openrgbDisconnect();
      setStatus({ connected: false, devices: [] });
      setSelectedDevice(null);
      showToast("Getrennt");
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };

  const refresh = async () => {
    setBusy(true);
    try {
      const s = await api.openrgbRefresh();
      setStatus(s);
      showToast("Geräteliste aktualisiert");
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };

  const setEffect = async (devId: string, effectId: number) => {
    setBusy(true);
    try {
      const msg = await api.openrgbSetMode(devId, effectId, 5, 255, undefined, [
        { r: rgbR, g: rgbG, b: rgbB },
      ]);
      showToast(msg);
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };

  const setDeviceColor = async (devId: string, r: number, g: number, b: number) => {
    setBusy(true);
    try {
      const msg = await api.openrgbSetColor(devId, r, g, b);
      showToast(msg);
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };

  const allOff = async () => {
    setBusy(true);
    try {
      const msg = await api.openrgbAllOff();
      showToast(msg);
    } catch (e) {
      setError(apiError(e));
    } finally {
      setBusy(false);
    }
  };



  // ── derived ──
  const connected = status?.connected ?? false;
  const devices = status?.devices ?? [];
  const device: RgbDeviceInfo | null =
    devices.find((d) => d.id === selectedDevice) ?? null;

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full">
        <Loader2 className="w-6 h-6 animate-spin text-cyan-400" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Section header — matches Corsair layout */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Lightbulb className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold">Mainboard & Peripherie</h2>
          <span className="text-xs text-zinc-500">Direkte USB HID Kontrolle</span>
        </div>
        {connected && (
          <div className="flex items-center gap-2">
            <Button onClick={refresh} disabled={busy} variant="secondary" size="sm">
              <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${busy ? "animate-spin" : ""}`} />
              Rescan
            </Button>
            <Button onClick={allOff} disabled={busy} variant="secondary" size="sm">
              <PowerOff className="w-3.5 h-3.5 mr-1.5" />
              Alle Aus
            </Button>
            <Button onClick={disconnect} disabled={busy} variant="danger" size="sm">
              Trennen
            </Button>
          </div>
        )}
      </div>

      {/* Toast — bottom-right, matching Corsair style */}
      {toast && (
        <div className="fixed bottom-6 right-6 z-50 bg-zinc-800 border border-zinc-700 rounded-xl px-4 py-3 shadow-2xl text-sm animate-in slide-in-from-bottom-4 flex items-center gap-2">
          <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
          {toast}
        </div>
      )}

      {/* Error */}
      {error && (
        <Card className="border-red-500/30 bg-red-500/5">
          <div className="flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 text-red-400 shrink-0" />
            <p className="text-sm text-red-400">{error}</p>
          </div>
        </Card>
      )}

      {/* Device tabs — compact row to select active device */}
      {connected && devices.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {devices.map((dev) => {
            const Icon = DEVICE_ICONS[dev.device_type] ?? Lightbulb;
            const active = dev.id === selectedDevice;
            return (
              <button
                key={dev.id}
                onClick={() => setSelectedDevice(dev.id)}
                className={`flex items-center gap-2 text-xs px-3 py-1.5 rounded-lg border transition-colors ${
                  active
                    ? "bg-cyan-500/10 text-cyan-400 border-cyan-500/30"
                    : "bg-zinc-800/40 text-zinc-500 border-zinc-800 hover:text-white hover:border-zinc-700"
                }`}
              >
                <Icon className="w-3.5 h-3.5" />
                {dev.name}
              </button>
            );
          })}
        </div>
      )}

      {/* RGB Color Control */}
      {connected && device && (
        <Card>
          <div className="flex items-center gap-2 mb-3">
            <Palette className="w-4 h-4 text-purple-400" />
            <h3 className="text-sm font-semibold">Farbe — {device.name}</h3>
          </div>
          <div className="flex items-center gap-4">
            <div
              className="w-10 h-10 rounded-lg border border-zinc-700 shrink-0"
              style={{ backgroundColor: `rgb(${rgbR},${rgbG},${rgbB})` }}
            />
            <div className="flex-1 space-y-2">
              <ColorSlider label="R" value={rgbR} onChange={setRgbR} color="bg-red-500" />
              <ColorSlider label="G" value={rgbG} onChange={setRgbG} color="bg-green-500" />
              <ColorSlider label="B" value={rgbB} onChange={setRgbB} color="bg-blue-500" />
            </div>
            <div className="flex flex-col gap-1.5">
              <Button
                size="sm"
                disabled={busy}
                onClick={() => setDeviceColor(device.id, rgbR, rgbG, rgbB)}
              >
                Anwenden
              </Button>
              <Button
                size="sm"
                variant="secondary"
                disabled={busy}
                onClick={() =>
                  api
                    .openrgbOff(device.id)
                    .then((m) => showToast(m))
                    .catch((e) => setError(apiError(e)))
                }
              >
                <PowerOff className="w-3 h-3 mr-1" />
                Aus
              </Button>
            </div>
          </div>
        </Card>
      )}

      {/* Effects — only if device supports them */}
      {connected && device && device.effects.length > 1 && (
        <Card>
          <div className="flex items-center gap-2 mb-3">
            <Layers className="w-4 h-4 text-purple-400" />
            <h3 className="text-sm font-semibold">Effekte — {device.name}</h3>
          </div>
          <div className="flex flex-wrap gap-1.5">
            {EFFECTS.filter(
              (e) =>
                e.id === 0 ||
                device.effects.includes(
                  e.name === "Statisch"
                    ? "Static"
                    : e.name === "Pulsieren"
                      ? "Pulse"
                      : e.name === "Blinken"
                        ? "Blinking"
                        : e.name === "Farbwechsel"
                          ? "ColorCycle"
                          : e.name === "Welle"
                            ? "Wave"
                            : e.name === "Zufall"
                              ? "Random"
                              : e.name
                )
            ).map((eff) => (
              <button
                key={eff.id}
                disabled={busy}
                onClick={() => setEffect(device.id, eff.id)}
                className="text-[11px] px-3 py-1.5 rounded-lg bg-zinc-800/40 text-zinc-400 border border-zinc-800 hover:border-zinc-700 hover:text-white transition-all"
              >
                {eff.name}
              </button>
            ))}
          </div>
        </Card>
      )}

      {/* No devices */}
      {connected && devices.length === 0 && (
        <Card>
          <div className="text-center py-6 text-zinc-500">
            <Lightbulb className="w-6 h-6 mx-auto mb-2 opacity-30" />
            <p className="text-sm">Keine RGB-Geräte gefunden</p>
            <p className="text-xs mt-1">
              Stelle sicher, dass die Geräte angeschlossen sind und du
              Leserechte hast (udev-Regeln).
            </p>
          </div>
        </Card>
      )}
    </div>
  );
}

// ─── Sub-Components ───────────────────────────────────────────

function ColorSlider({
  label,
  value,
  onChange,
  color,
}: {
  label: string;
  value: number;
  onChange: (v: number) => void;
  color: string;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="text-[10px] font-mono text-zinc-500 w-3">{label}</span>
      <input
        type="range"
        min={0}
        max={255}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className={`flex-1 h-1 accent-current ${color.replace("bg-", "text-")}`}
      />
      <span className="text-[10px] font-mono text-zinc-400 w-6 text-right tabular-nums">
        {value}
      </span>
    </div>
  );
}

import { useEffect, useState, useCallback, useRef } from "react";
import {
  Gauge,
  Cpu,
  HardDrive,
  Wifi,
  Monitor,
  MemoryStick,
  Server,
  ChevronDown,
  CheckCircle2,
  AlertTriangle,
  RefreshCw,
  Loader2,
  Info,
  Zap,
  Thermometer,
  Fan,
  RotateCcw,
  Flame,
  Activity,
} from "lucide-react";
import { api, TweakInfo, GpuOcStatus, apiError } from "../api";
import { Card, PageHeader, Loading, Badge, Button } from "../components/ui";

// ─── Category icons & colors ──────────────────────────────────

const CATEGORY_META: Record<string, { icon: typeof Cpu; color: string; bg: string }> = {
  "I/O":          { icon: HardDrive,   color: "text-blue-400",   bg: "bg-blue-500/10" },
  Memory:         { icon: MemoryStick, color: "text-purple-400", bg: "bg-purple-500/10" },
  Netzwerk:       { icon: Wifi,        color: "text-green-400",  bg: "bg-green-500/10" },
  GPU:            { icon: Monitor,     color: "text-amber-400",  bg: "bg-amber-500/10" },
  Dienste:        { icon: Server,      color: "text-cyan-400",   bg: "bg-cyan-500/10" },
  Dateisystem:    { icon: HardDrive,   color: "text-emerald-400",bg: "bg-emerald-500/10" },
  System:         { icon: Cpu,         color: "text-red-400",    bg: "bg-red-500/10" },
};

function getCategoryMeta(cat: string) {
  return CATEGORY_META[cat] || { icon: Zap, color: "text-zinc-400", bg: "bg-zinc-800" };
}

// ─── Tuning Page ──────────────────────────────────────────────

export default function Tuning() {
  const [tweaks, setTweaks] = useState<TweakInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setError(null);
      const status = await api.getTuningStatus();
      setTweaks(status.tweaks);
    } catch (e) {
      setError(apiError(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { load(); }, [load]);

  // Show toast briefly
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 3000);
    return () => clearTimeout(t);
  }, [toast]);

  const apply = useCallback(async (id: string, value: string) => {
    setApplying(id);
    try {
      const res = await api.applyTuning(id, value);
      if (res.success) {
        setToast(res.message);
        // Update local state
        setTweaks((prev) =>
          prev.map((t) =>
            t.id === id ? { ...t, current: res.new_value, active: true } : t
          )
        );
        // Reload full state for side effects
        setTimeout(() => load(), 500);
      } else {
        setToast("Fehler: " + res.message);
      }
    } catch (e) {
      setToast("Fehler: " + apiError(e));
    } finally {
      setApplying(null);
    }
  }, [load]);

  // Group by category
  const categories = tweaks.reduce<Record<string, TweakInfo[]>>((acc, t) => {
    (acc[t.category] ||= []).push(t);
    return acc;
  }, {});

  // Count optimal
  const optimal = tweaks.filter((t) => t.active).length;
  const total = tweaks.length;

  if (loading) return <div className="p-6"><Loading text="Lade System-Tuning..." /></div>;

  return (
    <div className="p-6 space-y-6 max-w-5xl">
      <PageHeader
        title="System-Tuning"
        description={`${optimal}/${total} Einstellungen optimal`}
        actions={
          <button
            onClick={() => { setLoading(true); load(); }}
            className="p-2 rounded-lg text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 transition-colors"
            title="Aktualisieren"
          >
            <RefreshCw className="w-4 h-4" />
          </button>
        }
      />

      {error && (
        <Card className="border-red-500/30 bg-red-500/5">
          <p className="text-sm text-red-400">{error}</p>
        </Card>
      )}

      {/* Score bar */}
      <Card>
        <div className="flex items-center gap-4">
          <Gauge className="w-8 h-8 text-cyan-400" />
          <div className="flex-1">
            <div className="flex justify-between text-sm mb-1.5">
              <span className="font-medium">Performance Score</span>
              <span className={optimal === total ? "text-emerald-400" : "text-amber-400"}>
                {Math.round((optimal / Math.max(total, 1)) * 100)}%
              </span>
            </div>
            <div className="h-2 bg-zinc-800 rounded-full overflow-hidden">
              <div
                className={`h-full rounded-full transition-all duration-500 ${
                  optimal === total ? "bg-emerald-500" : "bg-cyan-500"
                }`}
                style={{ width: `${(optimal / Math.max(total, 1)) * 100}%` }}
              />
            </div>
          </div>
        </div>
      </Card>

      {/* Tweak categories */}
      {Object.entries(categories).map(([cat, items]) => {
        const meta = getCategoryMeta(cat);
        const CatIcon = meta.icon;
        return (
          <div key={cat} className="space-y-3">
            <div className="flex items-center gap-2 px-1">
              <div className={`p-1.5 rounded-lg ${meta.bg}`}>
                <CatIcon className={`w-4 h-4 ${meta.color}`} />
              </div>
              <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wider">
                {cat}
              </h2>
              <Badge color={items.every((i) => i.active) ? "green" : "zinc"}>
                {items.filter((i) => i.active).length}/{items.length}
              </Badge>
            </div>

            <div className="grid gap-3">
              {items.map((tweak) => (
                <TweakCard
                  key={tweak.id}
                  tweak={tweak}
                  applying={applying === tweak.id}
                  onApply={apply}
                />
              ))}
            </div>
          </div>
        );
      })}

      {/* GPU Overclock Panel */}
      <GpuOcPanel toast={setToast} />

      {/* Toast */}
      {toast && (
        <div className="fixed bottom-6 right-6 bg-zinc-800 border border-zinc-700 rounded-xl px-4 py-3 shadow-2xl text-sm animate-in slide-in-from-bottom-4 z-50">
          {toast}
        </div>
      )}
    </div>
  );
}

// ─── Individual Tweak Card ────────────────────────────────────

function TweakCard({
  tweak,
  applying,
  onApply,
}: {
  tweak: TweakInfo;
  applying: boolean;
  onApply: (id: string, value: string) => void;
}) {
  const [sliderValue, setSliderValue] = useState(Number(tweak.current) || 0);
  const [expanded, setExpanded] = useState(false);

  return (
    <Card className={`transition-all ${tweak.active ? "border-zinc-800" : "border-amber-500/20 bg-amber-500/[0.02]"}`}>
      <div className="flex items-start gap-4">
        {/* Status indicator */}
        <div className="mt-0.5">
          {tweak.active ? (
            <CheckCircle2 className="w-5 h-5 text-emerald-400" />
          ) : (
            <AlertTriangle className="w-5 h-5 text-amber-400" />
          )}
        </div>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2 mb-1">
            <h3 className="font-medium text-sm">{tweak.name}</h3>
            <span className="text-xs text-zinc-500 font-mono">{tweak.status}</span>
          </div>

          {/* Expandable description */}
          <button
            onClick={() => setExpanded(!expanded)}
            className="flex items-center gap-1 text-xs text-zinc-500 hover:text-zinc-400 transition-colors"
          >
            <Info className="w-3 h-3" />
            <span>{expanded ? "Weniger" : "Details"}</span>
            <ChevronDown className={`w-3 h-3 transition-transform ${expanded ? "rotate-180" : ""}`} />
          </button>

          {expanded && (
            <p className="text-xs text-zinc-500 mt-2 leading-relaxed">
              {tweak.description}
              {tweak.recommended && tweak.control !== "info" && (
                <span className="text-cyan-400/70"> Empfohlen: {tweak.recommended}</span>
              )}
            </p>
          )}
        </div>

        {/* Control */}
        <div className="shrink-0">
          {tweak.control === "toggle" && (
            <ToggleSwitch
              active={tweak.active}
              loading={applying}
              onChange={() => {
                const next = tweak.active
                  ? tweak.id === "fstrim_timer" ? "disabled" : "inactive"
                  : tweak.id === "fstrim_timer" ? "enabled" : "active";
                onApply(tweak.id, next);
              }}
            />
          )}

          {tweak.control === "select" && (
            <SelectControl
              value={tweak.current}
              options={tweak.options}
              loading={applying}
              recommended={tweak.recommended}
              onChange={(v) => onApply(tweak.id, v)}
            />
          )}

          {tweak.control === "slider" && (
            <div className="flex items-center gap-2">
              <input
                type="range"
                min={tweak.min ?? 0}
                max={tweak.max ?? 100}
                value={sliderValue}
                onChange={(e) => setSliderValue(Number(e.target.value))}
                className="w-24 accent-cyan-500"
              />
              <span className="text-xs font-mono text-zinc-400 w-8 text-right">
                {sliderValue}
              </span>
              {Number(tweak.current) !== sliderValue && (
                <button
                  onClick={() => onApply(tweak.id, String(sliderValue))}
                  disabled={applying}
                  className="px-2 py-1 text-xs bg-cyan-500/10 text-cyan-400 border border-cyan-500/30 rounded-md hover:bg-cyan-500/20 transition-colors disabled:opacity-50"
                >
                  {applying ? <Loader2 className="w-3 h-3 animate-spin" /> : "OK"}
                </button>
              )}
            </div>
          )}

          {tweak.control === "info" && (
            <div className="text-xs text-zinc-500 font-mono">
              {tweak.active ? "✓" : "—"}
            </div>
          )}
        </div>
      </div>
    </Card>
  );
}

// ─── Toggle Switch ────────────────────────────────────────────

function ToggleSwitch({
  active,
  loading,
  onChange,
}: {
  active: boolean;
  loading: boolean;
  onChange: () => void;
}) {
  return (
    <button
      onClick={onChange}
      disabled={loading}
      className={`relative w-11 h-6 rounded-full transition-colors duration-200 ${
        active ? "bg-emerald-500" : "bg-zinc-700"
      } ${loading ? "opacity-50" : ""}`}
    >
      {loading ? (
        <Loader2 className="w-4 h-4 animate-spin absolute top-1 left-1 text-white" />
      ) : (
        <div
          className={`absolute top-0.5 w-5 h-5 bg-white rounded-full shadow-sm transition-transform duration-200 ${
            active ? "translate-x-[22px]" : "translate-x-0.5"
          }`}
        />
      )}
    </button>
  );
}

// ─── Select Dropdown ──────────────────────────────────────────

function SelectControl({
  value,
  options,
  loading,
  recommended,
  onChange,
}: {
  value: string;
  options: string[];
  loading: boolean;
  recommended: string;
  onChange: (v: string) => void;
}) {
  return (
    <div className="relative">
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        disabled={loading}
        className="appearance-none bg-zinc-800 border border-zinc-700 rounded-lg pl-3 pr-8 py-1.5 text-xs font-mono text-zinc-300 hover:border-zinc-600 focus:border-cyan-500 focus:ring-1 focus:ring-cyan-500/30 transition-colors disabled:opacity-50 cursor-pointer"
      >
        {options.map((opt) => (
          <option key={opt} value={opt}>
            {opt}{opt === recommended ? " ★" : ""}
          </option>
        ))}
      </select>
      <ChevronDown className="absolute right-2 top-1/2 -translate-y-1/2 w-3 h-3 text-zinc-500 pointer-events-none" />
      {loading && (
        <Loader2 className="absolute right-7 top-1/2 -translate-y-1/2 w-3 h-3 animate-spin text-cyan-400" />
      )}
    </div>
  );
}

// ─── GPU Overclock Panel ──────────────────────────────────────

function GpuOcPanel({ toast }: { toast: (msg: string) => void }) {
  const [oc, setOc] = useState<GpuOcStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [applying, setApplying] = useState(false);
  const [expanded, setExpanded] = useState(true);
  const pollRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const [bootEnabled, setBootEnabled] = useState(false);
  const [bootLoading, setBootLoading] = useState(false);
  const initializedRef = useRef(false);
  // Tracks the last applied/loaded GPU values — stable reference for hasChanges
  const appliedRef = useRef({ sclk: 0, mclk: 0, volt: 0, power: 0, fan: "auto", fanPwm: 0 });

  // Editable state
  const [sclkMax, setSclkMax] = useState(0);
  const [mclkMax, setMclkMax] = useState(0);
  const [voltOffset, setVoltOffset] = useState(0);
  const [powerCap, setPowerCap] = useState(0);
  const [fanMode, setFanMode] = useState("auto");
  const [fanPwm, setFanPwm] = useState(0);

  // Initial load — runs once
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const status = await api.getGpuOcStatus();
        if (cancelled) return;
        setOc(status);
        if (!initializedRef.current) {
          initializedRef.current = true;
          setSclkMax(status.sclk_max);
          setMclkMax(status.mclk_max);
          setVoltOffset(status.voltage_offset);
          setPowerCap(status.power_cap_w);
          setFanMode(status.fan_mode);
          setFanPwm(status.fan_pwm);
          appliedRef.current = { sclk: status.sclk_max, mclk: status.mclk_max, volt: status.voltage_offset, power: status.power_cap_w, fan: status.fan_mode, fanPwm: status.fan_pwm };
          try {
            const svcEnabled = await api.getGpuOcServiceStatus();
            if (!cancelled) setBootEnabled(svcEnabled);
          } catch { /* ignore */ }
        }
      } catch { /* ignore */ }
      finally { if (!cancelled) setLoading(false); }
    })();
    return () => { cancelled = true; };
  }, []);

  // Poll live stats only — never touches slider values
  useEffect(() => {
    pollRef.current = setInterval(async () => {
      try {
        const status = await api.getGpuOcStatus();
        setOc(status);
      } catch { /* ignore */ }
    }, 2000);
    return () => { if (pollRef.current) clearInterval(pollRef.current); };
  }, []);

  const applyOc = useCallback(async () => {
    if (!oc) return;
    setApplying(true);
    try {
      const params: Record<string, unknown> = {
        sclk_max: sclkMax,
        mclk_max: mclkMax,
        voltage_offset: voltOffset,
        power_cap_w: powerCap,
        fan_mode: fanMode,
        fan_pwm: fanPwm,
      };

      const res = await api.applyGpuOc(params as any);
      toast(res.message);
      // Refresh
      const fresh = await api.getGpuOcStatus();
      setOc(fresh);
      setSclkMax(fresh.sclk_max);
      setMclkMax(fresh.mclk_max);
      setVoltOffset(fresh.voltage_offset);
      setPowerCap(fresh.power_cap_w);
      setFanMode(fresh.fan_mode);
      setFanPwm(fresh.fan_pwm);
      appliedRef.current = { sclk: fresh.sclk_max, mclk: fresh.mclk_max, volt: fresh.voltage_offset, power: fresh.power_cap_w, fan: fresh.fan_mode, fanPwm: fresh.fan_pwm };
    } catch (e) {
      toast("Fehler: " + apiError(e));
    } finally {
      setApplying(false);
    }
  }, [oc, sclkMax, mclkMax, voltOffset, powerCap, fanMode, fanPwm, toast]);

  const resetOc = useCallback(async () => {
    setApplying(true);
    try {
      const res = await api.resetGpuOc();
      toast(res.message);
      const fresh = await api.getGpuOcStatus();
      setOc(fresh);
      setSclkMax(fresh.sclk_max);
      setMclkMax(fresh.mclk_max);
      setVoltOffset(fresh.voltage_offset);
      setPowerCap(fresh.power_cap_w);
      setFanMode(fresh.fan_mode);
      setFanPwm(fresh.fan_pwm);
      appliedRef.current = { sclk: fresh.sclk_max, mclk: fresh.mclk_max, volt: fresh.voltage_offset, power: fresh.power_cap_w, fan: fresh.fan_mode, fanPwm: fresh.fan_pwm };
    } catch (e) {
      toast("Fehler: " + apiError(e));
    } finally {
      setApplying(false);
    }
  }, [toast]);

  if (loading) return null;
  if (!oc) return null;

  if (!oc.available) {
    return (
      <div className="space-y-3">
        <div className="flex items-center gap-2 px-1">
          <div className="p-1.5 rounded-lg bg-red-500/10">
            <Flame className="w-4 h-4 text-red-400" />
          </div>
          <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wider">
            GPU Overclock
          </h2>
          <Badge color="red">
            {oc.gpu_name.split("[").pop()?.replace("]", "").split("(")[0].trim() || "GPU"}
          </Badge>
          <span className="text-xs text-zinc-500 ml-auto">nicht verfügbar</span>
        </div>

        <Card>
          <div className="space-y-2">
            <p className="text-sm text-zinc-300">
              Die Overclock-Regler sind ausgeblendet, weil das amdgpu Overdrive-Interface
              <span className="font-mono"> pp_od_clk_voltage</span> nicht vorhanden ist.
            </p>
            <p className="text-xs text-zinc-500">
              Typische Ursache: Overdrive ist per Kernel-Featuremask deaktiviert.
            </p>
            <div className="text-xs text-zinc-400 space-y-1">
              <div>
                1) Kernel-Parameter setzen: <span className="font-mono">amdgpu.ppfeaturemask=0xffffffff</span>
              </div>
              <div>2) Reboot</div>
              <div>
                3) Prüfen: <span className="font-mono">/sys/class/drm/cardX/device/pp_od_clk_voltage</span>
              </div>
            </div>
          </div>
        </Card>
      </div>
    );
  }

  const a = appliedRef.current;
  const hasChanges =
    sclkMax !== a.sclk ||
    mclkMax !== a.mclk ||
    voltOffset !== a.volt ||
    powerCap !== a.power ||
    fanMode !== a.fan ||
    (fanMode === "manual" && fanPwm !== a.fanPwm);

  const tempColor = (t: number) =>
    t >= 90 ? "text-red-400" : t >= 70 ? "text-amber-400" : "text-emerald-400";

  return (
    <div className="space-y-3">
      {/* Header */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 px-1 w-full text-left"
      >
        <div className="p-1.5 rounded-lg bg-red-500/10">
          <Flame className="w-4 h-4 text-red-400" />
        </div>
        <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wider">
          GPU Overclock
        </h2>
        <Badge color="red">{oc.gpu_name.split('[').pop()?.replace(']','').split('(')[0].trim() || "GPU"}</Badge>
        <span className="text-xs text-zinc-500 ml-auto">{oc.vram_mb / 1024} GB VRAM</span>
        <ChevronDown className={`w-4 h-4 text-zinc-500 transition-transform ${expanded ? "rotate-180" : ""}`} />
      </button>

      {expanded && (
        <div className="space-y-3">
          {/* Live Stats Bar */}
          <Card>
            <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                  <Activity className="w-3 h-3" /> GPU Clock
                </div>
                <p className="text-lg font-bold font-mono">{oc.current_sclk_mhz} <span className="text-xs text-zinc-500">MHz</span></p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                  <MemoryStick className="w-3 h-3" /> VRAM Clock
                </div>
                <p className="text-lg font-bold font-mono">{oc.current_mclk_mhz} <span className="text-xs text-zinc-500">MHz</span></p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                  <Zap className="w-3 h-3" /> Power
                </div>
                <p className="text-lg font-bold font-mono">{oc.power_current_w} <span className="text-xs text-zinc-500">/ {oc.power_cap_w}W</span></p>
              </div>
              <div className="space-y-1">
                <div className="flex items-center gap-1.5 text-xs text-zinc-500">
                  <Gauge className="w-3 h-3" /> GPU Last
                </div>
                <p className="text-lg font-bold font-mono">{oc.gpu_busy_percent}<span className="text-xs text-zinc-500">%</span></p>
              </div>
            </div>

            {/* Temps */}
            <div className="flex gap-6 mt-4 pt-3 border-t border-zinc-800">
              <div className="flex items-center gap-1.5">
                <Thermometer className="w-3.5 h-3.5 text-zinc-500" />
                <span className="text-xs text-zinc-500">Edge</span>
                <span className={`text-sm font-mono font-medium ${tempColor(oc.temp_edge)}`}>{oc.temp_edge.toFixed(0)}°C</span>
              </div>
              <div className="flex items-center gap-1.5">
                <Thermometer className="w-3.5 h-3.5 text-zinc-500" />
                <span className="text-xs text-zinc-500">Junction</span>
                <span className={`text-sm font-mono font-medium ${tempColor(oc.temp_junction)}`}>{oc.temp_junction.toFixed(0)}°C</span>
              </div>
              <div className="flex items-center gap-1.5">
                <Thermometer className="w-3.5 h-3.5 text-zinc-500" />
                <span className="text-xs text-zinc-500">VRAM</span>
                <span className={`text-sm font-mono font-medium ${tempColor(oc.temp_mem)}`}>{oc.temp_mem.toFixed(0)}°C</span>
              </div>
              <div className="flex items-center gap-1.5 ml-auto">
                <Fan className="w-3.5 h-3.5 text-zinc-500" />
                <span className="text-xs text-zinc-500">Lüfter</span>
                <span className="text-sm font-mono font-medium text-zinc-300">{oc.fan_rpm} RPM</span>
              </div>
            </div>
          </Card>

          {/* OC Controls */}
          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            {/* GPU Clock */}
            <Card>
              <OcSlider
                label="GPU Clock (Max)"
                value={sclkMax}
                min={oc.sclk_range_min}
                max={oc.sclk_range_max}
                step={10}
                unit="MHz"
                defaultVal={oc.sclk_max}
                onChange={setSclkMax}
              />
            </Card>

            {/* VRAM Clock */}
            <Card>
              <OcSlider
                label="VRAM Clock (Max)"
                value={mclkMax}
                min={oc.mclk_range_min}
                max={oc.mclk_range_max}
                step={10}
                unit="MHz"
                defaultVal={oc.mclk_max}
                onChange={setMclkMax}
              />
            </Card>

            {/* Voltage Offset */}
            <Card>
              <OcSlider
                label="Voltage Offset"
                value={voltOffset}
                min={oc.voltage_min}
                max={oc.voltage_max}
                step={5}
                unit="mV"
                defaultVal={oc.voltage_offset}
                onChange={setVoltOffset}
                negative
              />
            </Card>

            {/* Power Limit */}
            <Card>
              <OcSlider
                label="Power Limit"
                value={powerCap}
                min={oc.power_default_w}
                max={oc.power_max_w}
                step={5}
                unit="W"
                defaultVal={oc.power_cap_w}
                onChange={setPowerCap}
              />
              <p className="text-[10px] text-zinc-600 mt-1">Default: {oc.power_default_w}W / Max: {oc.power_max_w}W</p>
            </Card>

            {/* Fan Control */}
            <Card className="md:col-span-2">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Fan className="w-4 h-4 text-zinc-400" />
                  <span className="text-sm font-medium">Lüftersteuerung</span>
                </div>
                <div className="flex gap-2">
                  <button
                    onClick={() => setFanMode("auto")}
                    className={`px-3 py-1 text-xs rounded-lg border transition-colors ${
                      fanMode === "auto"
                        ? "bg-emerald-500/10 text-emerald-400 border-emerald-500/30"
                        : "bg-zinc-800 text-zinc-400 border-zinc-700 hover:border-zinc-600"
                    }`}
                  >
                    Auto
                  </button>
                  <button
                    onClick={() => setFanMode("manual")}
                    className={`px-3 py-1 text-xs rounded-lg border transition-colors ${
                      fanMode === "manual"
                        ? "bg-amber-500/10 text-amber-400 border-amber-500/30"
                        : "bg-zinc-800 text-zinc-400 border-zinc-700 hover:border-zinc-600"
                    }`}
                  >
                    Manuell
                  </button>
                </div>
              </div>
              {fanMode === "manual" && (
                <div className="flex items-center gap-3">
                  <input
                    type="range"
                    min={0}
                    max={255}
                    value={fanPwm}
                    onChange={(e) => setFanPwm(Number(e.target.value))}
                    className="flex-1 accent-amber-500"
                  />
                  <span className="text-sm font-mono text-zinc-300 w-12 text-right">
                    {Math.round(fanPwm / 255 * 100)}%
                  </span>
                </div>
              )}
              {fanMode === "auto" && (
                <p className="text-xs text-zinc-500">Automatische Lüftersteuerung durch GPU-Firmware</p>
              )}
            </Card>
          </div>

          {/* Apply / Reset / Boot Service */}
          <div className="flex gap-3 items-center">
            <div className="flex items-center gap-2 mr-auto">
              <button
                onClick={async () => {
                  setBootLoading(true);
                  try {
                    if (bootEnabled) {
                      const res = await api.uninstallGpuOcService();
                      toast(res.message);
                      setBootEnabled(false);
                    } else {
                      const res = await api.installGpuOcService();
                      toast(res.message);
                      setBootEnabled(true);
                    }
                  } catch (e) {
                    toast("Fehler: " + apiError(e));
                  } finally {
                    setBootLoading(false);
                  }
                }}
                disabled={bootLoading}
                className={`relative inline-flex h-5 w-9 items-center rounded-full transition-colors ${
                  bootEnabled ? "bg-cyan-500" : "bg-zinc-700"
                }`}
              >
                <span
                  className={`inline-block h-3.5 w-3.5 rounded-full bg-white transition-transform ${
                    bootEnabled ? "translate-x-4" : "translate-x-0.5"
                  }`}
                />
              </button>
              <span className="text-xs text-zinc-400">
                {bootLoading ? "..." : bootEnabled ? "Boot-Service aktiv" : "Bei Neustart anwenden"}
              </span>
            </div>
            <Button
              variant="secondary"
              size="sm"
              onClick={resetOc}
              disabled={applying}
              loading={applying}
            >
              <RotateCcw className="w-3.5 h-3.5" />
              Werkseinstellungen
            </Button>
            <Button
              variant="primary"
              size="sm"
              onClick={applyOc}
              disabled={applying || !hasChanges}
              loading={applying}
            >
              Übernehmen
            </Button>
          </div>
        </div>
      )}
    </div>
  );
}

// ─── OC Slider Component ──────────────────────────────────────

function OcSlider({
  label,
  value,
  min,
  max,
  step,
  unit,
  defaultVal,
  onChange,
  negative = false,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  unit: string;
  defaultVal: number;
  onChange: (v: number) => void;
  negative?: boolean;
}) {
  const changed = value !== defaultVal;
  return (
    <div>
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm font-medium">{label}</span>
        <span className={`text-sm font-mono ${changed ? "text-cyan-400" : "text-zinc-400"}`}>
          {negative && value > 0 ? "+" : ""}{value} {unit}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
        className="w-full accent-cyan-500"
      />
      <div className="flex justify-between text-[10px] text-zinc-600 mt-0.5">
        <span>{min} {unit}</span>
        <span>{max} {unit}</span>
      </div>
    </div>
  );
}

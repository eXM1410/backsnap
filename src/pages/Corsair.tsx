import { useEffect, useState, useCallback, useRef } from "react";
import {
  Fan,
  Palette,
  Power,
  PowerOff,
  Monitor,
  RefreshCw,
  Loader2,
  CheckCircle2,
  AlertTriangle,
  Zap,
  Sliders,
  RotateCcw,
  Plus,
  Trash2,
  X,
  Save,
  Move,
  RotateCw,
  Pipette,
} from "lucide-react";
import {
  api,
  apiError,
  CorsairStatus,
  CcxtStatus,
  NexusStatus,
  NexusLayout,
  NexusWidget,
  WidgetKind,
  WidgetColor,
  DataSource,
  CorsairDeviceInfo,
  FanMode,
  FanCurvePoint,
} from "../api";
import { Card, Loading, Badge, Button } from "../components/ui";

// ─── Constants ────────────────────────────────────────────────

const POLL_MS = 2_000;

const DEFAULT_CURVE: FanCurvePoint[] = [
  { temp: 25, speed: 30 },
  { temp: 35, speed: 40 },
  { temp: 45, speed: 55 },
  { temp: 55, speed: 75 },
  { temp: 65, speed: 100 },
];

// ─── Helpers ──────────────────────────────────────────────────

function fanModeLabel(m: FanMode): string {
  if (m.type === "fixed") return `${m.speed}%`;
  return "Kurve";
}

/** Animated fan blade visual – spins proportional to RPM. */
function SpinningFan({
  rpm,
  duty,
  size = 64,
}: {
  rpm: number;
  duty: number;
  size?: number;
}) {
  const c = size / 2;
  const bladeR = size * 0.38;
  const hubR = size * 0.22;
  // Duration: faster RPM → faster spin. 0 RPM → stopped.
  const duration = rpm > 0 ? Math.max(0.15, 60 / rpm) : 0;
  const color =
    duty > 80 ? "#f87171" : duty > 50 ? "#22d3ee" : "#22d3ee";
  const dimColor =
    duty > 80 ? "rgba(248,113,113,0.15)" : duty > 50 ? "rgba(34,211,238,0.15)" : "rgba(34,211,238,0.15)";

  // 7 blade paths (curved fan blades)
  const blades = Array.from({ length: 7 }, (_, i) => {
    const angle = (i * 360) / 7;
    return (
      <path
        key={i}
        d={`M ${c} ${c} 
            L ${c + hubR * Math.cos(((angle - 18) * Math.PI) / 180)} ${c + hubR * Math.sin(((angle - 18) * Math.PI) / 180)}
            Q ${c + bladeR * 0.7 * Math.cos(((angle - 8) * Math.PI) / 180)} ${c + bladeR * 0.7 * Math.sin(((angle - 8) * Math.PI) / 180)}
              ${c + bladeR * Math.cos((angle * Math.PI) / 180)} ${c + bladeR * Math.sin((angle * Math.PI) / 180)}
            Q ${c + bladeR * 0.7 * Math.cos(((angle + 8) * Math.PI) / 180)} ${c + bladeR * 0.7 * Math.sin(((angle + 8) * Math.PI) / 180)}
              ${c + hubR * Math.cos(((angle + 18) * Math.PI) / 180)} ${c + hubR * Math.sin(((angle + 18) * Math.PI) / 180)}
            Z`}
        fill={color}
        opacity={0.85}
      />
    );
  });

  return (
    <div
      className="relative flex items-center justify-center"
      style={{ width: size, height: size }}
    >
      <svg width={size} height={size}>
        {/* outer ring */}
        <circle cx={c} cy={c} r={size * 0.46} fill="none" stroke={color} strokeWidth={1.5} opacity={0.3} />
        <circle cx={c} cy={c} r={size * 0.44} fill={dimColor} />

        {/* spinning blades group */}
        <g
          style={
            duration > 0
              ? { animation: `spin ${duration}s linear infinite`, transformOrigin: `${c}px ${c}px` }
              : { transform: `rotate(0deg)`, transformOrigin: `${c}px ${c}px` }
          }
        >
          {blades}
        </g>

        {/* hub — enlarged for readable % */}
        <circle cx={c} cy={c} r={hubR} fill="#18181b" stroke={color} strokeWidth={1.5} />
        <text
          x={c} y={c + 1}
          textAnchor="middle" dominantBaseline="central"
          fill={color}
          fontSize={size * 0.17}
          fontFamily="ui-monospace, monospace"
          fontWeight="bold"
        >
          {duty}%
        </text>
      </svg>

      {/* Global keyframes (injected once) */}
      <style>{`@keyframes spin { to { transform: rotate(360deg) } }`}</style>
    </div>
  );
}

// ─── Main ─────────────────────────────────────────────────────

export default function CorsairSection() {
  const [status, setStatus] = useState<CorsairStatus | null>(null);
  const [ccxt, setCcxt] = useState<CcxtStatus | null>(null);
  const [nexus, setNexus] = useState<NexusStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const pollRef = useRef<number | null>(null);
  const fetchingRef = useRef(false);

  // ── RGB state ────────────────────────────────────────────
  const [rgbR, setRgbR] = useState(0);
  const [rgbG, setRgbG] = useState(120);
  const [rgbB, setRgbB] = useState(255);

  // ── Fan override state ───────────────────────────────────
  const [editFanChannel, setEditFanChannel] = useState<number | null>(null);
  const [editFanSpeed, setEditFanSpeed] = useState(50);

  // ── Fan curve editor state ───────────────────────────────
  const [curveChannel, setCurveChannel] = useState<number | null>(null);
  const [curvePoints, setCurvePoints] = useState<FanCurvePoint[]>([...DEFAULT_CURVE]);

  // Toast auto-dismiss
  useEffect(() => {
    if (!toast) return;
    const t = setTimeout(() => setToast(null), 3000);
    return () => clearTimeout(t);
  }, [toast]);

  // ── Fetch ───────────────────────────────────────────────
  const fetchAll = useCallback(async (initial = false) => {
    if (fetchingRef.current) return;
    fetchingRef.current = true;
    try {
      const s = await api.getCorsairStatus();
      setStatus(s);
      setCcxt(s.ccxt);
      setNexus(s.nexus);
      if (initial) setError(null);
    } catch (e) {
      if (initial) setError(apiError(e));
    } finally {
      if (initial) setLoading(false);
      fetchingRef.current = false;
    }
  }, []);

  // Poll CCXT separately for faster updates when connected
  const pollCcxt = useCallback(async () => {
    if (fetchingRef.current) return;
    fetchingRef.current = true;
    try {
      const data = await api.corsairCcxtPoll();
      setCcxt(data);
    } catch {
      // silently ignore poll failures
    } finally {
      fetchingRef.current = false;
    }
  }, []);

  useEffect(() => {
    fetchAll(true);
    pollRef.current = window.setInterval(() => {
      if (ccxt?.connected) {
        pollCcxt();
      } else {
        fetchAll(false);
      }
    }, POLL_MS);
    return () => {
      if (pollRef.current) clearInterval(pollRef.current);
    };
  }, [fetchAll, pollCcxt, ccxt?.connected]);

  // ── Actions ─────────────────────────────────────────────
  const action = useCallback(
    async (label: string, fn: () => Promise<string>) => {
      setBusyAction(label);
      try {
        const msg = await fn();
        setToast(msg);
        await fetchAll(false);
      } catch (e) {
        setToast("Fehler: " + apiError(e));
      } finally {
        setBusyAction(null);
      }
    },
    [fetchAll]
  );

  const connectCcxt = useCallback(
    (serial: string) => action("CCXT connect", () => api.corsairCcxtConnect(serial)),
    [action]
  );
  const disconnectCcxt = useCallback(
    () => action("CCXT disconnect", () => api.corsairCcxtDisconnect()),
    [action]
  );
  const connectNexus = useCallback(
    (serial: string) => action("NEXUS connect", () => api.corsairNexusConnect(serial)),
    [action]
  );
  const disconnectNexus = useCallback(
    () => action("NEXUS disconnect", () => api.corsairNexusDisconnect()),
    [action]
  );

  const setFanSpeed = useCallback(
    (channel: number, speed: number | null) =>
      action(`Fan ${channel}`, () => api.corsairSetFanSpeed(channel, speed)),
    [action]
  );

  const setFanCurve = useCallback(
    (channel: number, points: FanCurvePoint[]) =>
      action(`Kurve ${channel}`, () => api.corsairSetFanCurve(channel, points)),
    [action]
  );

  const applyFanCurves = useCallback(
    () => action("Kurven anwenden", () => api.corsairApplyFanCurves()),
    [action]
  );

  const setRgb = useCallback(
    () => action("RGB", () => api.corsairSetRgb(rgbR, rgbG, rgbB)),
    [action, rgbR, rgbG, rgbB]
  );

  // ── Render ──────────────────────────────────────────────

  if (loading) return <div className="py-4"><Loading text="Corsair-Geräte werden erkannt…" /></div>;

  const devices = status?.devices ?? [];
  const ccxtDevices = devices.filter((d) => d.productId === 3114);
  const nexusDevices = devices.filter((d) => d.productId === 7054);

  return (
    <div className="space-y-6">
      {/* Section header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Fan className="w-5 h-5 text-cyan-400" />
          <h2 className="text-base font-semibold">Corsair</h2>
          <span className="text-xs text-zinc-500">Commander Core XT & iCUE NEXUS</span>
        </div>
        <Button
          variant="secondary"
          size="sm"
          onClick={() => fetchAll(false)}
          disabled={!!busyAction}
        >
          <RefreshCw className={`w-3.5 h-3.5 mr-1.5 ${busyAction ? "animate-spin" : ""}`} />
          Aktualisieren
        </Button>
      </div>

      {error && (
        <Card className="border-red-500/30 bg-red-500/5">
          <div className="flex items-center gap-2">
            <AlertTriangle className="w-4 h-4 text-red-400" />
            <p className="text-sm text-red-400">{error}</p>
          </div>
        </Card>
      )}

      {/* ─── Commander Core XT ──────────────────────────────── */}
      {ccxt && (
        <>
          {/* Fan Status */}
          <Card>
            <div className="flex items-center gap-2 mb-4">
              <Fan className="w-4 h-4 text-blue-400" />
              <h3 className="text-sm font-semibold">Lüfter</h3>
              <Badge color="blue">{ccxt.fans.filter((f) => f.connected).length} aktiv</Badge>
              <div className="flex-1" />
              <Button
                variant="secondary"
                size="sm"
                onClick={applyFanCurves}
                disabled={!!busyAction}
              >
                <Sliders className="w-3.5 h-3.5 mr-1" />
                Kurven anwenden
              </Button>
            </div>
            <div className="grid grid-cols-3 gap-3">
              {ccxt.fans.map((fan) => {
                const mode = ccxt.fanModes[fan.channel];
                return (
                  <div
                    key={fan.channel}
                    className={`bg-zinc-800/40 rounded-lg p-3 ${
                      fan.connected
                        ? "border border-zinc-700/50"
                        : "opacity-40 border border-zinc-800"
                    }`}
                  >
                    <div className="flex items-center justify-between mb-2">
                      <span className="text-xs text-zinc-400 font-medium">
                        Fan {fan.channel + 1}
                      </span>
                      {fan.connected ? (
                        <Badge color="cyan">
                          {mode ? fanModeLabel(mode) : "—"}
                        </Badge>
                      ) : (
                        <Badge color="zinc">getrennt</Badge>
                      )}
                    </div>
                    {fan.connected && (
                      <div className="flex items-center gap-3">
                        <SpinningFan rpm={fan.rpm} duty={fan.duty} />
                        <div>
                          <div className="text-lg font-bold font-mono tabular-nums">
                            {fan.rpm} <span className="text-xs text-zinc-500 font-normal">RPM</span>
                          </div>
                          <div className="flex gap-1 mt-1">
                            {editFanChannel === fan.channel ? (
                              <div className="flex items-center gap-1">
                                <input
                                  type="range"
                                  min={0}
                                  max={100}
                                  value={editFanSpeed}
                                  onChange={(e) => setEditFanSpeed(Number(e.target.value))}
                                  className="w-16 h-1 accent-cyan-400"
                                />
                                <span className="text-[10px] font-mono w-7 text-right tabular-nums">
                                  {editFanSpeed}%
                                </span>
                                <button
                                  onClick={() => {
                                    setFanSpeed(fan.channel, editFanSpeed);
                                    setEditFanChannel(null);
                                  }}
                                  className="text-cyan-400 hover:text-cyan-300 ml-1"
                                  title="Übernehmen"
                                >
                                  <CheckCircle2 className="w-3.5 h-3.5" />
                                </button>
                              </div>
                            ) : (
                              <>
                                <button
                                  onClick={() => {
                                    setEditFanChannel(fan.channel);
                                    setEditFanSpeed(fan.duty);
                                  }}
                                  className="text-[10px] px-1.5 py-0.5 rounded bg-zinc-700/50 hover:bg-zinc-700 text-zinc-300"
                                  title="Feste Drehzahl setzen"
                                >
                                  Manuell
                                </button>
                                <button
                                  onClick={() => {
                                    setCurveChannel(fan.channel);
                                    // Load existing curve for this channel or default
                                    const mode = ccxt.fanModes[fan.channel];
                                    if (mode?.type === "curve" && mode.points.length > 0) {
                                      setCurvePoints(mode.points.map(p => ({ ...p })));
                                    } else {
                                      setCurvePoints(DEFAULT_CURVE.map(p => ({ ...p })));
                                    }
                                  }}
                                  className="text-[10px] px-1.5 py-0.5 rounded bg-zinc-700/50 hover:bg-zinc-700 text-zinc-300"
                                  title="Kurve bearbeiten"
                                >
                                  Kurve
                                </button>
                                {mode?.type === "fixed" && (
                                  <button
                                    onClick={() => setFanSpeed(fan.channel, null)}
                                    className="text-[10px] px-1.5 py-0.5 rounded bg-zinc-700/50 hover:bg-zinc-700 text-zinc-300"
                                    title="Zurück zur Kurve"
                                  >
                                    <RotateCcw className="w-2.5 h-2.5" />
                                  </button>
                                )}
                              </>
                            )}
                          </div>
                        </div>
                      </div>
                    )}
                  </div>
                );
              })}
            </div>

            {/* ─── Curve Editor ─────────────── */}
            {curveChannel !== null && (
              <div className="mt-4 pt-4 border-t border-zinc-700/50">
                <div className="flex items-center justify-between mb-3">
                  <h4 className="text-xs font-semibold text-zinc-300">
                    Lüfterkurve — Fan {curveChannel + 1}
                  </h4>
                  <button
                    onClick={() => setCurveChannel(null)}
                    className="text-zinc-500 hover:text-zinc-300"
                  >
                    <X className="w-4 h-4" />
                  </button>
                </div>

                {/* SVG Chart */}
                <div className="bg-zinc-900 rounded-lg p-3 mb-3">
                  {(() => {
                    const waterProbe = ccxt.temps.find(t => t.connected);
                    const waterTemp = waterProbe?.temp ?? null;
                    return (
                      <>
                        <svg viewBox="0 0 300 160" className="w-full h-40">
                          {/* Grid lines */}
                          {[0, 25, 50, 75, 100].map((v) => (
                            <g key={`g-${v}`}>
                              <line
                                x1={30} y1={150 - v * 1.4} x2={290} y2={150 - v * 1.4}
                                stroke="#27272a" strokeWidth={0.5}
                              />
                              <text x={2} y={154 - v * 1.4} fill="#52525b" fontSize={8} fontFamily="monospace">
                                {v}%
                              </text>
                            </g>
                          ))}
                          {[20, 30, 40, 50, 60, 70, 80].map((t) => (
                            <g key={`t-${t}`}>
                              <line
                                x1={30 + (t - 15) * (260 / 75)} y1={10} x2={30 + (t - 15) * (260 / 75)} y2={150}
                                stroke="#27272a" strokeWidth={0.5}
                              />
                              <text x={30 + (t - 15) * (260 / 75) - 4} y={160} fill="#52525b" fontSize={7} fontFamily="monospace">
                                {t}°
                              </text>
                            </g>
                          ))}

                          {/* Current water temp indicator */}
                          {waterTemp !== null && waterTemp >= 15 && waterTemp <= 90 && (() => {
                            const tx = 30 + (waterTemp - 15) * (260 / 75);
                            return (
                              <g>
                                <line x1={tx} y1={10} x2={tx} y2={150} stroke="#facc15" strokeWidth={1} strokeDasharray="3 2" opacity={0.7} />
                                <text x={tx} y={7} textAnchor="middle" fill="#facc15" fontSize={7} fontFamily="monospace" fontWeight="bold">
                                  {waterTemp.toFixed(1)}°
                                </text>
                              </g>
                            );
                          })()}

                          {/* Curve line */}
                    {curvePoints.length > 1 && (
                      <polyline
                        fill="none"
                        stroke="#06b6d4"
                        strokeWidth={2}
                        strokeLinejoin="round"
                        points={curvePoints
                          .map((p) => `${30 + (p.temp - 15) * (260 / 75)},${150 - p.speed * 1.4}`)
                          .join(" ")}
                      />
                    )}
                    {/* Fill area */}
                    {curvePoints.length > 1 && (
                      <polygon
                        fill="rgba(6, 182, 212, 0.08)"
                        points={
                          `${30 + (curvePoints[0].temp - 15) * (260 / 75)},150 ` +
                          curvePoints.map((p) => `${30 + (p.temp - 15) * (260 / 75)},${150 - p.speed * 1.4}`).join(" ") +
                          ` ${30 + (curvePoints[curvePoints.length - 1].temp - 15) * (260 / 75)},150`
                        }
                      />
                    )}

                    {/* Draggable points */}
                    {curvePoints.map((p, idx) => (
                      <circle
                        key={idx}
                        cx={30 + (p.temp - 15) * (260 / 75)}
                        cy={150 - p.speed * 1.4}
                        r={5}
                        fill="#06b6d4"
                        stroke="#0e1217"
                        strokeWidth={2}
                        className="cursor-pointer"
                      />
                    ))}
                  </svg>
                  {waterTemp !== null ? (
                    <div className="flex items-center gap-2 mt-2 text-[10px] text-zinc-400 font-mono">
                      <span className="inline-block w-2 h-2 rounded-full bg-yellow-400" />
                      H₂O Sonde: <span className="text-yellow-400 font-bold">{waterTemp.toFixed(1)}°C</span>
                    </div>
                  ) : (
                    <div className="flex items-center gap-2 mt-2 text-[10px] text-amber-500 font-mono">
                      <AlertTriangle className="w-3 h-3" />
                      Keine Sonde verbunden — Fallback 35°C
                    </div>
                  )}
                  </>
                    );
                  })()}
                </div>

                {/* Point rows */}
                <div className="space-y-2">
                  {curvePoints.map((pt, idx) => (
                    <div key={idx} className="flex items-center gap-2 bg-zinc-800/40 rounded px-2 py-1.5">
                      <span className="text-[10px] text-zinc-500 w-4 text-center font-mono">{idx + 1}</span>
                      <label className="text-[10px] text-zinc-400 w-8">Temp</label>
                      <input
                        type="range" min={15} max={90}
                        value={pt.temp}
                        onChange={(e) => {
                          const pts = [...curvePoints];
                          pts[idx] = { ...pts[idx], temp: Number(e.target.value) };
                          setCurvePoints(pts.sort((a, b) => a.temp - b.temp));
                        }}
                        className="flex-1 h-1 accent-cyan-400"
                      />
                      <span className="text-[10px] font-mono w-8 text-right tabular-nums text-zinc-300">{pt.temp}°C</span>
                      <label className="text-[10px] text-zinc-400 w-8 ml-1">Speed</label>
                      <input
                        type="range" min={0} max={100}
                        value={pt.speed}
                        onChange={(e) => {
                          const pts = [...curvePoints];
                          pts[idx] = { ...pts[idx], speed: Number(e.target.value) };
                          setCurvePoints(pts);
                        }}
                        className="flex-1 h-1 accent-cyan-400"
                      />
                      <span className="text-[10px] font-mono w-8 text-right tabular-nums text-zinc-300">{pt.speed}%</span>
                      {curvePoints.length > 2 && (
                        <button
                          onClick={() => setCurvePoints(curvePoints.filter((_, i) => i !== idx))}
                          className="text-zinc-600 hover:text-red-400 ml-1"
                        >
                          <Trash2 className="w-3 h-3" />
                        </button>
                      )}
                    </div>
                  ))}
                </div>

                {/* Actions */}
                <div className="flex items-center gap-2 mt-3">
                  {curvePoints.length < 8 && (
                    <Button
                      variant="secondary" size="sm"
                      onClick={() => {
                        const last = curvePoints[curvePoints.length - 1];
                        const newTemp = Math.min((last?.temp ?? 50) + 10, 90);
                        const newSpeed = Math.min((last?.speed ?? 50) + 10, 100);
                        setCurvePoints([...curvePoints, { temp: newTemp, speed: newSpeed }].sort((a, b) => a.temp - b.temp));
                      }}
                    >
                      <Plus className="w-3 h-3 mr-1" /> Punkt
                    </Button>
                  )}
                  <Button
                    variant="secondary" size="sm"
                    onClick={() => setCurvePoints(DEFAULT_CURVE.map(p => ({ ...p })))}
                  >
                    <RotateCcw className="w-3 h-3 mr-1" /> Standard
                  </Button>
                  <div className="flex-1" />
                  <Button
                    variant="secondary" size="sm"
                    onClick={() => setCurveChannel(null)}
                  >
                    Abbrechen
                  </Button>
                  <Button
                    variant="primary" size="sm"
                    disabled={!!busyAction}
                    onClick={async () => {
                      await setFanCurve(curveChannel, curvePoints);
                      await applyFanCurves();
                      setCurveChannel(null);
                    }}
                  >
                    <CheckCircle2 className="w-3 h-3 mr-1" /> Übernehmen
                  </Button>
                </div>
              </div>
            )}
          </Card>

          {/* RGB */}
          <Card>
            <div className="flex items-center gap-2 mb-4">
              <Palette className="w-4 h-4 text-purple-400" />
              <h3 className="text-sm font-semibold">RGB</h3>
            </div>
            <div className="flex items-center gap-4">
              <div
                className="w-10 h-10 rounded-lg border border-zinc-700 shrink-0"
                style={{ backgroundColor: `rgb(${rgbR}, ${rgbG}, ${rgbB})` }}
              />
              <div className="flex-1 space-y-2">
                <ColorSlider label="R" value={rgbR} onChange={setRgbR} color="bg-red-500" />
                <ColorSlider label="G" value={rgbG} onChange={setRgbG} color="bg-green-500" />
                <ColorSlider label="B" value={rgbB} onChange={setRgbB} color="bg-blue-500" />
              </div>
              <Button variant="primary" size="sm" onClick={setRgb} disabled={!!busyAction}>
                Übernehmen
              </Button>
            </div>
          </Card>
        </>
      )}

      {/* ─── iCUE NEXUS ────────────────────────────────────── */}
      {nexus && (
        <Card>
          <div className="flex items-center gap-2 mb-4">
            <Monitor className="w-4 h-4 text-cyan-400" />
            <h3 className="text-sm font-semibold">iCUE NEXUS</h3>
            <Badge color="green">verbunden</Badge>
            <div className="flex-1" />
            <span className="text-[10px] text-zinc-500">FW {nexus.firmware}</span>
          </div>

          {/* Page navigation */}
          <div className="flex items-center gap-2 mb-3">
            <button
              onClick={() => action("NEXUS prev", () => api.corsairNexusPrevPage())}
              disabled={!!busyAction}
              className="text-zinc-400 hover:text-white disabled:opacity-40 px-2 py-1 rounded bg-zinc-800/60"
            >
              ◀
            </button>

            {["FANS", "TEMPS", "SYSTEM", "UHR"].map((name, i) => (
              <button
                key={i}
                onClick={() => action(`Seite ${name}`, () => api.corsairNexusSetPage(i))}
                disabled={!!busyAction}
                className={`text-[10px] px-2.5 py-1 rounded font-semibold transition-colors ${
                  nexus.currentPage === i
                    ? "bg-cyan-500/20 text-cyan-400 border border-cyan-500/30"
                    : "bg-zinc-800/60 text-zinc-400 hover:text-white border border-transparent"
                }`}
              >
                {name}
              </button>
            ))}

            <button
              onClick={() => action("NEXUS next", () => api.corsairNexusNextPage())}
              disabled={!!busyAction}
              className="text-zinc-400 hover:text-white disabled:opacity-40 px-2 py-1 rounded bg-zinc-800/60"
            >
              ▶
            </button>

            <div className="flex-1" />

            <button
              onClick={() =>
                action("Auto-Cycle", () =>
                  api.corsairNexusSetAutoCycle(!nexus.autoCycle)
                )
              }
              disabled={!!busyAction}
              className={`text-[10px] px-2 py-1 rounded font-medium transition-colors ${
                nexus.autoCycle
                  ? "bg-cyan-500/15 text-cyan-400 border border-cyan-500/30"
                  : "bg-zinc-800/60 text-zinc-500 border border-transparent"
              }`}
              title="Automatisch zwischen Seiten wechseln"
            >
              <RefreshCw className={`w-3 h-3 inline mr-1 ${nexus.autoCycle ? "animate-spin" : ""}`} />
              Auto
            </button>
          </div>

          {/* NEXUS LCD interactive editor */}
          <NexusLcdEditor
            currentPage={nexus.currentPage}
            busy={!!busyAction}
            onClear={() => action("NEXUS clear", () => api.corsairNexusDisplay({ type: "clear" }))}
          />
        </Card>
      )}

      {/* ─── Empty state ───────────────────────────────────── */}
      {!ccxt && !nexus && devices.length > 0 && (
        <Card className="border-zinc-700/50">
          <div className="text-center py-6">
            <Power className="w-8 h-8 text-zinc-600 mx-auto mb-2" />
            <p className="text-sm text-zinc-400">
              Geräte erkannt — klicke <strong>Verbinden</strong> um loszulegen.
            </p>
          </div>
        </Card>
      )}

      {/* ─── Save Profile (page bottom) ──────────────────────── */}
      {(ccxt || nexus) && (
        <div className="border-t border-zinc-700/50 pt-6 mt-4 flex justify-center">
          <Button
            variant="primary"
            size="sm"
            onClick={() => action("Profil speichern", () => api.corsairSaveProfile())}
            disabled={!!busyAction}
            className="px-6"
          >
            <Save className="w-4 h-4 mr-2" />
            Profil speichern
          </Button>
        </div>
      )}

      {/* ─── Toast ─────────────────────────────────────────── */}
      {toast && (
        <div className="fixed bottom-6 right-6 bg-zinc-800 border border-zinc-700 rounded-xl px-4 py-3 shadow-2xl text-sm animate-in slide-in-from-bottom-4 z-50 flex items-center gap-2">
          {toast.startsWith("Fehler") ? (
            <AlertTriangle className="w-4 h-4 text-red-400 shrink-0" />
          ) : (
            <CheckCircle2 className="w-4 h-4 text-emerald-400 shrink-0" />
          )}
          {toast}
        </div>
      )}
    </div>
  );
}

// ─── Sub-Components ───────────────────────────────────────────

function DeviceConnectBtn({
  device,
  ccxt,
  nexus,
  onConnectCcxt,
  onDisconnectCcxt,
  onConnectNexus,
  onDisconnectNexus,
  busy,
}: {
  device: CorsairDeviceInfo;
  ccxt: CcxtStatus | null;
  nexus: NexusStatus | null;
  onConnectCcxt: (serial: string) => void;
  onDisconnectCcxt: () => void;
  onConnectNexus: (serial: string) => void;
  onDisconnectNexus: () => void;
  busy: boolean;
}) {
  const isCcxt = device.productId === 3114;
  const isNexus = device.productId === 7054;
  const connected =
    (isCcxt && ccxt?.connected && ccxt.serial === device.serial) ||
    (isNexus && nexus?.connected && nexus.serial === device.serial);

  if (connected) {
    return (
      <Button
        variant="danger"
        size="sm"
        onClick={() => (isCcxt ? onDisconnectCcxt() : onDisconnectNexus())}
        disabled={busy}
      >
        <PowerOff className="w-3.5 h-3.5 mr-1" />
        Trennen
      </Button>
    );
  }

  return (
    <Button
      variant="primary"
      size="sm"
      onClick={() =>
        isCcxt
          ? onConnectCcxt(device.serial)
          : isNexus
          ? onConnectNexus(device.serial)
          : undefined
      }
      disabled={busy}
    >
      {busy ? (
        <Loader2 className="w-3.5 h-3.5 mr-1 animate-spin" />
      ) : (
        <Power className="w-3.5 h-3.5 mr-1" />
      )}
      Verbinden
    </Button>
  );
}

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

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="bg-zinc-800/40 rounded-lg px-3 py-2">
      <div className="text-[10px] text-zinc-500 uppercase tracking-wider">{label}</div>
      <div className="text-sm font-mono">{value}</div>
    </div>
  );
}

// ─── NEXUS LCD Interactive Editor ─────────────────────────────

const LCD_W = 640;
const LCD_H = 48;
const SCALE = 6; // render canvas at 6x for crisp HiDPI scaling

/** Remove duplicate widget IDs (keep first), auto-reset if way too many widgets */
function deduplicateLayout(l: NexusLayout): NexusLayout {
  return {
    ...l,
    pages: l.pages.map((p) => ({
      ...p,
      widgets: p.widgets.filter(
        (w, i, arr) => arr.findIndex((w2) => w2.id === w.id) === i,
      ),
    })),
  };
}

const WIDGET_COLORS: WidgetColor[] = ["white", "cyan", "amber", "red", "purple", "dim"];
const COLOR_CSS: Record<WidgetColor, string> = {
  white: "#fff", cyan: "#00c8dc", amber: "#fbBf24",
  red: "#f87171", purple: "#7850a0", dim: "#646464",
};

const DATA_SOURCES: { value: DataSource; label: string }[] = [
  { value: "waterTemp", label: "H2O" },
  { value: "cpuTemp", label: "CPU Temp" },
  { value: "gpuTemp", label: "GPU Temp" },
  { value: "totalPower", label: "Power" },
  { value: "cpuUsage", label: "CPU %" },
  { value: "ramUsage", label: "RAM %" },
  { value: "cpuFreq", label: "CPU Freq" },
  { value: "ramTotal", label: "RAM Total" },
];

/** Widget kind label for tooltip / display */
function widgetLabel(w: NexusWidget): string {
  switch (w.kind.type) {
    case "fanIcon": return `Fan ${w.kind.channel}`;
    case "sensor": return w.kind.label || w.kind.source;
    case "statusBar": return w.kind.label;
    case "label": return w.kind.text;
    case "clock": return "Uhr";
    case "divider": return "│";
    case "pageDots": return "•••";
  }
}

/** All addable widget templates */
const WIDGET_TEMPLATES: { label: string; icon: string; factory: () => WidgetKind }[] = [
  { label: "Fan Icon", icon: "🌀", factory: () => ({ type: "fanIcon" as const, channel: 0, color: "white" as WidgetColor, scale: 1 }) },
  { label: "Sensor", icon: "🌡️", factory: () => ({ type: "sensor" as const, source: "cpuTemp" as DataSource, label: "CPU", scale: 2, color: "white" as WidgetColor }) },
  { label: "Status Bar", icon: "📊", factory: () => ({ type: "statusBar" as const, source: "cpuUsage" as DataSource, label: "CPU", color: "cyan" as WidgetColor, scale: 1 }) },
  { label: "Label", icon: "Aa", factory: () => ({ type: "label" as const, text: "TEXT", scale: 1, color: "white" as WidgetColor }) },
  { label: "Uhr", icon: "🕐", factory: () => ({ type: "clock" as const, color: "white" as WidgetColor, scale: 1 }) },
  { label: "Trennlinie", icon: "│", factory: () => ({ type: "divider" as const, color: "dim" as WidgetColor }) },
  { label: "Seiten-Dots", icon: "•••", factory: () => ({ type: "pageDots" as const, color: "white" as WidgetColor }) },
];

function NexusLcdEditor({
  currentPage,
  busy,
  onClear,
}: {
  currentPage: number;
  busy: boolean;
  onClear: () => void;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const containerRef = useRef<HTMLDivElement>(null);
  const tmpCanvasRef = useRef<HTMLCanvasElement | null>(null);

  // Layout: ref is the synchronous source of truth, state drives renders
  const layoutRef = useRef<NexusLayout | null>(null);
  const [, forceRender] = useState(0);
  const layout = layoutRef.current;

  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [editMode, setEditMode] = useState(false);
  const [showAddMenu, setShowAddMenu] = useState(false);

  // Drag state lives entirely in a ref (no React state → no stale closures)
  const dragRef = useRef<{
    anchorId: string;
    startX: number;
    startY: number;
    started: boolean;
    mode: "move" | "resize";
    // Per-widget original positions/sizes for multi-drag
    origins: Map<string, { x: number; y: number; w: number; h: number; offX: number; offY: number }>;
  } | null>(null);

  /** Update layout ref + trigger render */
  const setLayout = useCallback((val: NexusLayout | null) => {
    layoutRef.current = val;
    forceRender((n) => n + 1);
  }, []);

  /** Send ref layout to Rust */
  const commitLayout = useCallback(() => {
    const l = layoutRef.current;
    if (l) api.corsairNexusSetLayout(l).catch(() => {});
  }, []);

  // Load layout once (with dedup to clean corrupted layouts)
  useEffect(() => {
    api
      .corsairNexusGetLayout()
      .then((l) => {
        const cleaned = deduplicateLayout(l);
        setLayout(cleaned);
        // If dedup removed widgets, push the cleaned version back
        const origCount = l.pages.reduce((n, p) => n + p.widgets.length, 0);
        const cleanCount = cleaned.pages.reduce((n, p) => n + p.widgets.length, 0);
        if (cleanCount < origCount) {
          api.corsairNexusSetLayout(cleaned).catch(() => {});
        }
      })
      .catch(() => {});
  }, []);

  // Poll frame preview (500 ms) — reuse tmp canvas for performance
  useEffect(() => {
    const interval = setInterval(async () => {
      try {
        const b64 = await api.corsairNexusGetFrame();
        if (!b64 || !canvasRef.current) return;
        const ctx = canvasRef.current.getContext("2d");
        if (!ctx) return;
        const raw = atob(b64);
        const data = new Uint8ClampedArray(raw.length);
        for (let i = 0; i < raw.length; i++) data[i] = raw.charCodeAt(i);
        // Cached frame is RGBA — matches web ImageData, no swap needed.
        // (BGRA conversion for the LCD hardware is done in Rust.)

        // Auto-detect frame dimensions from data length
        const pixels = data.length / 4;
        const aspect = LCD_W / LCD_H;
        const srcH = Math.round(Math.sqrt(pixels / aspect));
        const srcW = Math.round(pixels / srcH);
        if (srcW * srcH * 4 !== data.length) return;

        const srcImg = new ImageData(data, srcW, srcH);

        // Reuse offscreen canvas
        let tmp = tmpCanvasRef.current;
        if (!tmp || tmp.width !== srcW || tmp.height !== srcH) {
          tmp = document.createElement("canvas");
          tmp.width = srcW;
          tmp.height = srcH;
          tmpCanvasRef.current = tmp;
        }
        const tmpCtx = tmp.getContext("2d");
        if (!tmpCtx) return;
        tmpCtx.putImageData(srcImg, 0, 0);

        // Clear previous frame completely before drawing the new one
        ctx.clearRect(0, 0, canvasRef.current.width, canvasRef.current.height);
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(tmp, 0, 0, LCD_W * SCALE, LCD_H * SCALE);
      } catch {
        /* ignore */
      }
    }, 500);
    return () => clearInterval(interval);
  }, []);

  const pageWidgets = layout?.pages[currentPage]?.widgets ?? [];
  const selectedWidgets = pageWidgets.filter((w) => selected.has(w.id));
  // For single-select property editing, use the last-clicked (first in set)
  const selectedWidget = selectedWidgets.length === 1 ? selectedWidgets[0] : null;

  // ── Helpers ──────────────────────────────────────────────

  const toLcd = useCallback(
    (clientX: number, clientY: number): { x: number; y: number } => {
      const rect = containerRef.current?.getBoundingClientRect();
      if (!rect) return { x: 0, y: 0 };
      return {
        x: Math.round(((clientX - rect.left) / rect.width) * LCD_W),
        y: Math.round(((clientY - rect.top) / rect.height) * LCD_H),
      };
    },
    [],
  );

  /** Mutate widgets on the current page directly in ref */
  const mutateWidgets = useCallback(
    (fn: (widgets: NexusWidget[]) => NexusWidget[]) => {
      const cur = layoutRef.current;
      if (!cur) return;
      const pages = [...cur.pages];
      const page = { ...pages[currentPage] };
      page.widgets = fn(page.widgets);
      pages[currentPage] = page;
      layoutRef.current = { ...cur, pages };
      forceRender((n) => n + 1);
    },
    [currentPage],
  );

  const updateWidgetKind = useCallback(
    (ids: Set<string>, kindPatch: Partial<WidgetKind>) => {
      mutateWidgets((widgets) =>
        widgets.map((w) =>
          ids.has(w.id)
            ? { ...w, kind: { ...w.kind, ...kindPatch } as WidgetKind }
            : w,
        ),
      );
      commitLayout();
    },
    [mutateWidgets, commitLayout],
  );

  const addWidget = useCallback(
    (kind: WidgetKind) => {
      const id = `w-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`;
      // Auto-stagger: find a free position that doesn't overlap existing widgets
      const existing = layoutRef.current?.pages[currentPage]?.widgets ?? [];
      let x = 10;
      let y = 10;
      for (const ew of existing) {
        if (Math.abs(ew.x - x) < 40 && Math.abs(ew.y - y) < 20) {
          x += 60;
          if (x > 550) {
            x = 10;
            y = y < 25 ? 28 : 10;
          }
        }
      }
      const ww = kind.type === "divider" ? 1 : kind.type === "pageDots" ? 32 : 80;
      const hh = kind.type === "divider" ? 38 : kind.type === "pageDots" ? 8 : 30;
      const w: NexusWidget = { id, kind, x, y, w: ww, h: hh };
      mutateWidgets((widgets) => [...widgets, w]);
      commitLayout();
      setSelected(new Set([id]));
      setShowAddMenu(false);
    },
    [mutateWidgets, commitLayout, currentPage],
  );

  const deleteSelected = useCallback(() => {
    if (selected.size === 0) return;
    mutateWidgets((widgets) => widgets.filter((w) => !selected.has(w.id)));
    commitLayout();
    setSelected(new Set());
  }, [selected, mutateWidgets, commitLayout]);

  const resetLayout = useCallback(() => {
    api
      .corsairNexusResetLayout()
      .then((l) => {
        setLayout(deduplicateLayout(l));
        setSelected(new Set());
      })
      .catch(() => {});
  }, [setLayout]);

  // ── Global pointer handlers (attached once) ─────────────

  useEffect(() => {
    const THRESHOLD = 4; // px screen distance before drag begins

    const onPointerMove = (e: PointerEvent) => {
      const d = dragRef.current;
      if (!d) return;

      const dx = e.clientX - d.startX;
      const dy = e.clientY - d.startY;

      // Haven't exceeded threshold yet → not dragging
      if (!d.started && dx * dx + dy * dy < THRESHOLD * THRESHOLD) return;
      d.started = true;

      const cur = layoutRef.current;
      if (!cur) return;
      const pages = [...cur.pages];
      const page = { ...pages[currentPage] };

      if (d.mode === "move") {
        const lcd = toLcd(e.clientX, e.clientY);
        page.widgets = page.widgets.map((ww) => {
          const orig = d.origins.get(ww.id);
          if (!orig) return ww;
          return {
            ...ww,
            x: Math.max(0, Math.min(LCD_W - 1, lcd.x - orig.offX)),
            y: Math.max(0, Math.min(LCD_H - 1, lcd.y - orig.offY)),
          };
        });
      } else {
        // resize — apply delta to all selected
        const rect = containerRef.current?.getBoundingClientRect();
        if (!rect) return;
        const dxLcd = (dx / rect.width) * LCD_W;
        const dyLcd = (dy / rect.height) * LCD_H;
        page.widgets = page.widgets.map((ww) => {
          const orig = d.origins.get(ww.id);
          if (!orig) return ww;
          return {
            ...ww,
            w: Math.max(4, Math.round(orig.w + dxLcd)),
            h: Math.max(4, Math.round(orig.h + dyLcd)),
          };
        });
      }

      pages[currentPage] = page;
      layoutRef.current = { ...cur, pages };
      forceRender((n) => n + 1);
    };

    const onPointerUp = () => {
      const d = dragRef.current;
      dragRef.current = null;
      if (d?.started) {
        // Commit after drag/resize
        const l = layoutRef.current;
        if (l) api.corsairNexusSetLayout(l).catch(() => {});
      }
    };

    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
    return () => {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
    };
  }, [currentPage, toLcd]);

  // ── Widget mouse-down (select + begin potential drag) ───

  const handleWidgetPointerDown = useCallback(
    (e: React.PointerEvent, w: NexusWidget) => {
      if (!editMode) return;
      e.stopPropagation();
      e.preventDefault();

      const lcd = toLcd(e.clientX, e.clientY);

      // Ctrl/Meta+click toggles selection; plain click replaces
      let newSel: Set<string>;
      if (e.ctrlKey || e.metaKey) {
        newSel = new Set(selected);
        if (newSel.has(w.id)) {
          newSel.delete(w.id);
        } else {
          newSel.add(w.id);
        }
      } else {
        // If clicking an already-selected widget in a group, keep group
        newSel = selected.has(w.id) ? new Set(selected) : new Set([w.id]);
      }
      setSelected(newSel);
      setShowAddMenu(false);

      // Build per-widget origins for all selected widgets
      const widgets = layoutRef.current?.pages[currentPage]?.widgets ?? [];
      const origins = new Map<string, { x: number; y: number; w: number; h: number; offX: number; offY: number }>();
      for (const ww of widgets) {
        if (newSel.has(ww.id)) {
          // offX/offY: offset from the clicked LCD position to each widget's origin
          origins.set(ww.id, {
            x: ww.x, y: ww.y, w: ww.w, h: ww.h,
            offX: lcd.x - ww.x + (ww.id === w.id ? 0 : (w.x - ww.x)),
            offY: lcd.y - ww.y + (ww.id === w.id ? 0 : (w.y - ww.y)),
          });
        }
      }

      dragRef.current = {
        anchorId: w.id,
        startX: e.clientX,
        startY: e.clientY,
        started: false,
        mode: "move",
        origins,
      };
    },
    [editMode, toLcd, selected, currentPage],
  );

  const handleResizePointerDown = useCallback(
    (e: React.PointerEvent, w: NexusWidget) => {
      e.stopPropagation();
      e.preventDefault();

      // Build origins for all selected (resize applies delta to all)
      const widgets = layoutRef.current?.pages[currentPage]?.widgets ?? [];
      const origins = new Map<string, { x: number; y: number; w: number; h: number; offX: number; offY: number }>();
      for (const ww of widgets) {
        if (selected.has(ww.id)) {
          origins.set(ww.id, { x: ww.x, y: ww.y, w: ww.w, h: ww.h, offX: 0, offY: 0 });
        }
      }

      dragRef.current = {
        anchorId: w.id,
        startX: e.clientX,
        startY: e.clientY,
        started: false,
        mode: "resize",
        origins,
      };
    },
    [selected, currentPage],
  );

  // For multi-select: determine common color/scale across all selected widgets
  const selColor: WidgetColor | null = (() => {
    const colors = selectedWidgets
      .filter((w) => "color" in w.kind)
      .map((w) => (w.kind as { color: WidgetColor }).color);
    if (colors.length === 0) return null;
    return colors.every((c) => c === colors[0]) ? colors[0] : colors[0]; // show first
  })();
  const hasColor = selectedWidgets.some((w) => "color" in w.kind);

  const selScale: number | null = (() => {
    const scales = selectedWidgets
      .filter((w) => "scale" in w.kind)
      .map((w) => (w.kind as { scale: number }).scale);
    if (scales.length === 0) return null;
    return scales.every((s) => s === scales[0]) ? scales[0] : scales[0];
  })();
  const hasScale = selectedWidgets.some((w) => "scale" in w.kind);

  return (
    <div className="mb-3">
      {/* ── Toolbar ────────────────────────────────────── */}
      <div className="flex items-center gap-1.5 mb-2 flex-wrap">
        <button
          onClick={() => {
            setEditMode(!editMode);
            setShowAddMenu(false);
          }}
          className={`text-[10px] px-2 py-1 rounded font-medium transition-colors ${
            editMode
              ? "bg-cyan-500/15 text-cyan-400 border border-cyan-500/30"
              : "bg-zinc-800/60 text-zinc-500 border border-transparent hover:text-white"
          }`}
          title="Widgets verschieben & skalieren"
        >
          <Move className="w-3 h-3 inline mr-1" />
          Editor
        </button>

        {editMode && (
          <>
            {/* Add widget */}
            <div className="relative">
              <button
                onClick={() => setShowAddMenu(!showAddMenu)}
                className="text-[10px] px-2 py-1 rounded bg-zinc-800/60 text-emerald-400 hover:text-emerald-300 border border-transparent"
                title="Widget hinzufügen"
              >
                <Plus className="w-3 h-3 inline mr-1" />
                Hinzufügen
              </button>
              {showAddMenu && (
                <div className="absolute left-0 top-full mt-1 z-30 bg-zinc-900 border border-zinc-700 rounded-lg shadow-xl p-1 min-w-[140px]">
                  {WIDGET_TEMPLATES.map((t) => (
                    <button
                      key={t.label}
                      onClick={() => addWidget(t.factory())}
                      className="flex items-center gap-2 w-full text-left text-[10px] px-2 py-1.5 rounded hover:bg-zinc-800 text-zinc-300"
                    >
                      <span className="w-4 text-center">{t.icon}</span>
                      {t.label}
                    </button>
                  ))}
                </div>
              )}
            </div>

            {/* Delete selected */}
            {selected.size > 0 && (
              <button
                onClick={deleteSelected}
                className="text-[10px] px-2 py-1 rounded bg-zinc-800/60 text-red-400 hover:text-red-300 border border-transparent"
                title="Ausgewählte Widgets löschen"
              >
                <Trash2 className="w-3 h-3 inline mr-1" />
                Löschen{selected.size > 1 ? ` (${selected.size})` : ""}
              </button>
            )}

            {/* Reset to defaults */}
            <button
              onClick={resetLayout}
              className="text-[10px] px-2 py-1 rounded bg-zinc-800/60 text-zinc-500 hover:text-orange-400 border border-transparent"
              title="Layout auf Werkseinstellung zurücksetzen"
            >
              <RotateCcw className="w-3 h-3 inline mr-1" />
              Reset
            </button>

            {/* Reload from backend */}
            <button
              onClick={() => {
                api
                  .corsairNexusGetLayout()
                  .then((l) => setLayout(deduplicateLayout(l)))
                  .catch(() => {});
                setSelected(new Set());
              }}
              className="text-[10px] px-2 py-1 rounded bg-zinc-800/60 text-zinc-500 hover:text-white border border-transparent"
              title="Layout vom Server neu laden"
            >
              <RotateCw className="w-3 h-3 inline mr-1" />
              Neuladen
            </button>

            {/* Selected info */}
            {selectedWidgets.length === 1 && selectedWidget && (
              <span className="text-[10px] text-cyan-400 font-mono ml-1">
                {widgetLabel(selectedWidget)} x:{selectedWidget.x} y:
                {selectedWidget.y} {selectedWidget.w}×{selectedWidget.h}
              </span>
            )}
            {selectedWidgets.length > 1 && (
              <span className="text-[10px] text-cyan-400 font-mono ml-1">
                {selectedWidgets.length} Widgets ausgewählt
              </span>
            )}
          </>
        )}

        <div className="flex-1" />
        <Button variant="secondary" size="sm" onClick={onClear} disabled={busy}>
          Display leeren
        </Button>
      </div>

      {/* ── Properties + inline color (visible when widget(s) selected) ── */}
      {editMode && selected.size > 0 && (
        <div className="flex items-center gap-2 mb-2 px-1 flex-wrap">
          {/* Color swatches (apply to all selected) */}
          {hasColor && (
            <>
              <span className="text-[10px] text-zinc-500">Farbe:</span>
              {WIDGET_COLORS.map((c) => (
                <button
                  key={c}
                  onClick={() =>
                    updateWidgetKind(selected, {
                      color: c,
                    } as Partial<WidgetKind>)
                  }
                  className={`w-5 h-5 rounded border-2 transition-transform ${
                    selColor === c
                      ? "border-white scale-125"
                      : "border-zinc-600 hover:border-zinc-400"
                  }`}
                  style={{ backgroundColor: COLOR_CSS[c] }}
                  title={c}
                />
              ))}
              <div className="w-px h-4 bg-zinc-700 mx-1" />
            </>
          )}

          {/* Scale (apply to all selected) */}
          {hasScale && selScale !== null && (
            <label className="flex items-center gap-1 text-[10px] text-zinc-400">
              Größe:
              <input
                type="number"
                min={0.5}
                max={4}
                step={0.5}
                value={selScale}
                onChange={(e) =>
                  updateWidgetKind(selected, {
                    scale: Number(e.target.value),
                  } as Partial<WidgetKind>)
                }
                className="w-10 bg-zinc-800 border border-zinc-700 rounded px-1 py-0.5 text-[10px] text-white"
              />
            </label>
          )}

          {/* ── Single-select only properties ── */}
          {selectedWidget && (
            <>
              {/* Channel for FanIcon */}
              {selectedWidget.kind.type === "fanIcon" && (
                <label className="flex items-center gap-1 text-[10px] text-zinc-400">
                  Kanal:
                  <input
                    type="number"
                    min={0}
                    max={5}
                    value={selectedWidget.kind.channel}
                    onChange={(e) =>
                      updateWidgetKind(selected, {
                        channel: Number(e.target.value),
                      } as Partial<WidgetKind>)
                    }
                    className="w-10 bg-zinc-800 border border-zinc-700 rounded px-1 py-0.5 text-[10px] text-white"
                  />
                </label>
              )}

              {/* Source for Sensor/StatusBar */}
              {(selectedWidget.kind.type === "sensor" ||
                selectedWidget.kind.type === "statusBar") && (
                <label className="flex items-center gap-1 text-[10px] text-zinc-400">
                  Quelle:
                  <select
                    value={(selectedWidget.kind as { source: DataSource }).source}
                    onChange={(e) =>
                      updateWidgetKind(selected, {
                        source: e.target.value as DataSource,
                      } as Partial<WidgetKind>)
                    }
                    className="bg-zinc-800 border border-zinc-700 rounded px-1 py-0.5 text-[10px] text-white"
                  >
                    {DATA_SOURCES.map((d) => (
                      <option key={d.value} value={d.value}>
                        {d.label}
                      </option>
                    ))}
                  </select>
                </label>
              )}

              {/* Label/Text */}
              {(selectedWidget.kind.type === "sensor" ||
                selectedWidget.kind.type === "statusBar" ||
                selectedWidget.kind.type === "label") && (
                <label className="flex items-center gap-1 text-[10px] text-zinc-400">
                  {selectedWidget.kind.type === "label" ? "Text:" : "Label:"}
                  <input
                    type="text"
                    value={
                      selectedWidget.kind.type === "label"
                        ? (selectedWidget.kind as { text: string }).text
                        : (selectedWidget.kind as { label: string }).label
                    }
                    onChange={(e) => {
                      const key =
                        selectedWidget.kind.type === "label" ? "text" : "label";
                      updateWidgetKind(selected, {
                        [key]: e.target.value,
                      } as Partial<WidgetKind>);
                    }}
                    className="w-20 bg-zinc-800 border border-zinc-700 rounded px-1 py-0.5 text-[10px] text-white"
                  />
                </label>
              )}
            </>
          )}
        </div>
      )}

      {/* ── LCD Canvas + Widget Overlays ──────────────── */}
      <div
        ref={containerRef}
        className={`relative rounded-lg border overflow-hidden select-none ${
          editMode
            ? "border-cyan-500/40 shadow-[0_0_12px_rgba(34,211,238,0.15)]"
            : "border-zinc-800"
        }`}
        style={{ aspectRatio: `${LCD_W}/${LCD_H}`, background: "#000" }}
        onPointerDown={() => {
          if (editMode) {
            setSelected(new Set());
            setShowAddMenu(false);
          }
        }}
      >
        {/* Canvas — pointer-events-none so overlays get all clicks */}
        <canvas
          ref={canvasRef}
          width={LCD_W * SCALE}
          height={LCD_H * SCALE}
          className="absolute inset-0 w-full h-full pointer-events-none"
        />

        {/* Widget overlay handles (edit mode only) */}
        {editMode &&
          pageWidgets.map((w) => (
            <div
              key={w.id}
              className={`absolute ${
                selected.has(w.id)
                  ? "border border-cyan-400 bg-cyan-500/10"
                  : "border border-transparent hover:border-zinc-500/50"
              } cursor-move`}
              style={{
                left: `${(w.x / LCD_W) * 100}%`,
                top: `${(w.y / LCD_H) * 100}%`,
                width: `${(w.w / LCD_W) * 100}%`,
                height: `${(w.h / LCD_H) * 100}%`,
              }}
              title={widgetLabel(w)}
              onPointerDown={(e) => handleWidgetPointerDown(e, w)}
            >
              {selected.has(w.id) && (
                <div
                  className="absolute -bottom-[3px] -right-[3px] w-[6px] h-[6px] bg-cyan-400 rounded-sm cursor-se-resize z-10"
                  onPointerDown={(e) => handleResizePointerDown(e, w)}
                />
              )}
            </div>
          ))}
      </div>
    </div>
  );
}

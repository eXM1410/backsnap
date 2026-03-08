import { memo, useState, useRef, useEffect, useCallback, useMemo, type RefObject } from "react";
import { Mic, Send, Volume2, VolumeX, Zap, Power, ChevronUp, ArrowLeft, Droplets, Thermometer, Sun, Gauge, Activity } from "lucide-react";
import { useNavigate } from "react-router-dom";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { api, apiError, type SystemMonitorData, type GpuOcStatus, type PiTentStatus, type DesktopApp } from "../api";

// ── Types ────────────────────────────────────────────────────
interface Message { role: "user" | "assistant"; content: string; actions?: { action: string; success: boolean; message: string }[]; }
type Phase = "idle" | "listening" | "processing" | "speaking";
type OrbitItem = { l: string; v: string; w?: boolean };
interface AudioTelemetry { rms: number; peak: number; wake: number; state: string; }
interface VoicePhaseEvent { payload: Phase; }
interface ChatStreamEvent { payload: Message & { source?: string }; }
interface LauncherHex { id: string; label: string; x: number; y: number; appId: string; accent: string; }
interface LauncherDraft { label: string; appId: string; }

// ── Constants ────────────────────────────────────────────────
const CY = "#22d3ee", CY2 = "#06b6d4", CYD = "rgba(34,211,238,0.12)";
const AM = "#f59e0b"; // amber for tent
const GR = "#22c55e"; // green for plant
const RD = "#ef4444", RDD = "rgba(239,68,68,0.12)";
const HIST = 50;
const SAFE_HUD = false;
const AMD = "rgba(245,158,11,0.12)";

const HUD = 1020;
const Cx = HUD / 2;
const R1 = Cx * 0.96, R2 = Cx * 0.88, R3 = Cx * 0.78, R4 = Cx * 0.68;
const R5 = Cx * 0.56, R6 = Cx * 0.46, RC = Cx * 0.26;

// graph dimensions (wider/taller for readability)
const GW = 410, GH = 68, GHS = 56;
const ORBIT_R = R1 + 100;
const ORBIT_BADGE_W = 132;

function useNow(ms = 1000) {
  const [now, setNow] = useState(new Date());
  useEffect(() => { const id = setInterval(() => setNow(new Date()), ms); return () => clearInterval(id); }, [ms]);
  return now;
}
const p2 = (n: number) => String(n).padStart(2, "0");
const WD = ["SO","MO","DI","MI","DO","FR","SA"];
const MN = ["JAN","FEB","MÄR","APR","MAI","JUN","JUL","AUG","SEP","OKT","NOV","DEZ"];

/** SVG sparkline path from array values within a configurable range */
function spark(data: number[], w: number, h: number, maxValue = 100, minValue = 0): string {
  if (data.length < 2) return "";
  const step = w / (data.length - 1);
  const range = Math.max(1, maxValue - minValue);
  return data
    .map((v, i) => {
      const normalized = Math.max(0, Math.min(1, (v - minValue) / range));
      return `${i === 0 ? "M" : "L"}${(i * step).toFixed(1)},${(h - normalized * h).toFixed(1)}`;
    })
    .join(" ");
}

const HBar = memo(function HBar({ pct, color, w = 120, h = 6 }: { pct: number; color: string; w?: number; h?: number }) {
  return (
    <svg width={w} height={h} className="block">
      <rect x={0} y={0} width={w} height={h} rx={3} fill="rgba(255,255,255,0.04)" />
      <rect x={0} y={0} width={Math.max(0, Math.min(pct, 100)) / 100 * w} height={h} rx={3} fill={color} opacity={0.7} />
    </svg>
  );
});

const SL = memo(function SL({ children }: { children: string }) {
  return <div className="text-[10px] font-black tracking-[0.3em] text-zinc-500 mb-2 mt-4 first:mt-0 uppercase">{children}</div>;
});

const HeroCard = memo(function HeroCard({ label, value, sub, color }: { label: string; value: string; sub?: string; color: string }) {
  return (
    <div className="hero-card relative overflow-hidden px-4 py-3 min-h-[88px]">
      <div className="hero-sweep" />
      <div className="hero-corner tl" />
      <div className="hero-corner br" />
      <div className="text-[9px] tracking-[0.28em] text-zinc-500 uppercase">{label}</div>
      <div className="text-[28px] font-black font-mono tracking-wide leading-none mt-2" style={{ color }}>{value}</div>
      {sub && <div className="text-[11px] tracking-[0.14em] text-zinc-500 mt-1">{sub}</div>}
    </div>
  );
});

const KV = memo(function KV({ k, v, warn, unit, color = CY, warnColor = RD }: { k: string; v: string | number; warn?: boolean; unit?: string; color?: string; warnColor?: string }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-[1px]">
      <span className="text-[11px] tracking-[0.15em] text-zinc-500">{k}</span>
      <span className="text-[15px] font-bold font-mono tracking-wide" style={{ color: warn ? warnColor : color }}>
        {v}{unit && <span className="text-[10px] text-zinc-500 ml-0.5">{unit}</span>}
      </span>
    </div>
  );
});

const Spark = memo(function Spark({ data, color, w = GW, h = GH, label, valNow, maxValue = 100, minValue = 0, subLabel }: { data: number[]; color: string; w?: number; h?: number; label: string; valNow: string; maxValue?: number; minValue?: number; subLabel?: string }) {
  const path = useMemo(() => spark(data, w, h, maxValue, minValue), [data, w, h, maxValue, minValue]);
  return (
    <div className="mt-2">
      <div className="flex items-baseline justify-between mb-1">
        <span className="text-[10px] tracking-[0.15em] text-zinc-500 font-semibold">{label}</span>
        <div className="text-right">
          <span className="text-[14px] font-black font-mono block leading-none" style={{ color }}>{valNow}</span>
          {subLabel && <span className="text-[9px] tracking-[0.14em] text-zinc-600 block mt-0.5">{subLabel}</span>}
        </div>
      </div>
      <svg width={w} height={h} className="block">
        <rect x={0} y={0} width={w} height={h} rx={3} fill="rgba(255,255,255,0.02)" />
        {data.length >= 2 && path && <>
          <path d={path} fill="none" stroke={color} strokeWidth={5} opacity={0.08} />
          <path d={path + ` L${w},${h} L0,${h} Z`} fill={color} opacity={0.08} />
          <path d={path} fill="none" stroke={color} strokeWidth={1.5} opacity={0.7} />
        </>}
        {[0.25, 0.5, 0.75].map(f => <line key={f} x1={0} y1={h * f} x2={w} y2={h * f} stroke="rgba(255,255,255,0.04)" strokeWidth={0.5} />)}
      </svg>
    </div>
  );
});

const ClockReadout = memo(function ClockReadout({ rc, uptime }: { rc: string; uptime: string }) {
  const now = useNow();
  const timeStr = `${p2(now.getHours())}:${p2(now.getMinutes())}:${p2(now.getSeconds())}`;
  const dateStr = `${WD[now.getDay()]} ${p2(now.getDate())} ${MN[now.getMonth()]} ${now.getFullYear()}`;

  return (
    <>
      <div className="text-[40px] font-black font-mono tracking-wider leading-none" style={{ color: rc }}>{timeStr}</div>
      <div className="text-[15px] font-mono tracking-wider text-zinc-500 mt-1">{dateStr}</div>
      <div className="flex gap-6 mt-1">
        <span className="text-[11px] text-zinc-500">UPTIME <span className="font-bold font-mono text-[13px]" style={{ color: rc }}>{uptime}</span></span>
      </div>
    </>
  );
});

const HudBackground = memo(function HudBackground({ gd, activated }: { gd: string; activated: boolean }) {
  return (
    <>
      <div className="pointer-events-none absolute inset-0" style={{ background: `radial-gradient(ellipse 55% 45% at 50% 50%, ${AMD}, transparent 72%)` }} />
      <div className="pointer-events-none absolute inset-0" style={{ background: "radial-gradient(ellipse 72% 62% at 50% 48%, rgba(249,115,22,0.08), transparent 62%)" }} />
      <div className="pointer-events-none absolute inset-0" style={{ background: `radial-gradient(ellipse 35% 40% at 50% 50%, ${SAFE_HUD ? "rgba(34,211,238,0.06)" : gd}, transparent 70%)` }} />
      <div className="pointer-events-none absolute inset-0" style={{ background: "radial-gradient(ellipse 80% 80% at 50% 50%, transparent 50%, rgba(2,6,23,0.94))" }} />
      <div className="pointer-events-none absolute inset-0 hud-grid opacity-40" />
      <div className="scan-line" />
      {activated && <div className="clap-flash pointer-events-none absolute inset-0 z-50" style={{ background: `radial-gradient(circle at 50% 50%, ${CY}30, transparent 60%)` }} />}
    </>
  );
});

const ReactorCore = memo(function ReactorCore({ rc, rc2, gd, phaseLabel, phaseAccent, ticks, majorTicks }: { rc: string; rc2: string; gd: string; phaseLabel: string; phaseAccent: string; ticks: number[]; majorTicks: number[] }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center z-10 pointer-events-none">
      <div className="hud-boot relative contain-layout" style={{ width: HUD, height: HUD }}>
        <div className="pulse absolute inset-0 will-transform">
          <svg className="absolute inset-0" width={HUD} height={HUD} style={{ opacity: 0.12 }}>
            {ticks.map(a => {
              const rad = (a * Math.PI) / 180;
              const len = a % 30 === 0 ? 14 : a % 10 === 0 ? 8 : 2;
              const x1 = Cx + Math.cos(rad) * (R1 - len), y1 = Cx + Math.sin(rad) * (R1 - len);
              const x2 = Cx + Math.cos(rad) * R1, y2 = Cx + Math.sin(rad) * R1;
              return <line key={a} x1={x1} y1={y1} x2={x2} y2={y2} stroke={rc} strokeWidth={a % 30 === 0 ? 1.5 : 0.5} opacity={a % 30 === 0 ? 0.5 : 0.25} />;
            })}
          </svg>
          <svg className="absolute inset-0 r1 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R2} stroke={rc} strokeWidth={1.5} strokeDasharray={`${R2*0.55} ${R2*0.25}`} fill="none" opacity={0.5} strokeLinecap="round" />
            <circle cx={Cx} cy={Cx} r={R2} stroke={rc} strokeWidth={8} strokeDasharray={`${R2*0.55} ${R2*0.25}`} fill="none" opacity={0.025} strokeLinecap="round" />
          </svg>
          <svg className="absolute inset-0 r2 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R3} stroke={rc} strokeWidth={1} strokeDasharray="4 10" fill="none" opacity={0.3} />
            <circle cx={Cx} cy={Cx} r={R3} stroke={rc} strokeWidth={6} strokeDasharray={`${R3*1.2} ${R3*0.4}`} fill="none" opacity={0.025} strokeLinecap="round" />
          </svg>
          <svg className="absolute inset-0 r3 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R4} stroke={rc} strokeWidth={2.5} strokeDasharray={`${R4*0.9} ${R4*0.38}`} fill="none" opacity={0.65} strokeLinecap="round" />
            <circle cx={Cx} cy={Cx} r={R4} stroke={rc} strokeWidth={10} strokeDasharray={`${R4*0.9} ${R4*0.38}`} fill="none" opacity={0.025} strokeLinecap="round" />
          </svg>
          <svg className="absolute inset-0 r4 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R5} stroke={rc2} strokeWidth={2} strokeDasharray={`${R5*0.5} ${R5*0.18}`} fill="none" opacity={0.55} strokeLinecap="round" />
          </svg>
          <svg className="absolute inset-0 r5 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R6} stroke={rc} strokeWidth={1.5} strokeDasharray={`${R6*0.4} ${R6*0.15}`} fill="none" opacity={0.4} strokeLinecap="round" />
            {[0,60,120,180,240,300].map(a => { const rad=(a*Math.PI)/180; return <circle key={a} cx={Cx+Math.cos(rad)*R6} cy={Cx+Math.sin(rad)*R6} r={3} fill={rc} opacity={0.5} />; })}
          </svg>
          <svg className="absolute inset-0 ring-breathe will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={R5 * 1.07} stroke={AM} strokeWidth={1.4} strokeDasharray={`${R5*0.72} ${R5*0.28}`} fill="none" opacity={0.34} strokeLinecap="round" />
            <circle cx={Cx} cy={Cx} r={R3 * 1.015} stroke={AM} strokeWidth={0.9} strokeDasharray="5 14" fill="none" opacity={0.2} />
          </svg>
          <svg className="absolute inset-0 r6 will-transform" width={HUD} height={HUD}>
            <circle cx={Cx} cy={Cx} r={RC} stroke={rc} strokeWidth={1} strokeDasharray="3 5" fill="none" opacity={0.3} />
          </svg>

          <svg className="absolute inset-0" width={HUD} height={HUD} style={{ opacity: 0.04 }}>
            <line x1={Cx} y1={Cx-R2} x2={Cx} y2={Cx-RC-15} stroke={rc} strokeWidth={0.5} />
            <line x1={Cx} y1={Cx+RC+15} x2={Cx} y2={Cx+R2} stroke={rc} strokeWidth={0.5} />
            <line x1={Cx-R2} y1={Cx} x2={Cx-RC-15} y2={Cx} stroke={rc} strokeWidth={0.5} />
            <line x1={Cx+RC+15} y1={Cx} x2={Cx+R2} y2={Cx} stroke={rc} strokeWidth={0.5} />
          </svg>
        </div>

        <div className="absolute inset-0 flex items-center justify-center pointer-events-none">
          <div className="core-g rounded-full will-transform" style={{ width: RC*2.1, height: RC*2.1, backgroundColor: rc, opacity: SAFE_HUD ? 0.12 : undefined, filter: `blur(${RC*(SAFE_HUD ? 0.26 : 0.55)}px)` }} />
        </div>
        <div className="absolute inset-0 flex items-center justify-center pointer-events-none ring-breathe will-transform">
          <div className="rounded-full" style={{ width: RC*1.18, height: RC*1.18, border: `1px solid ${AM}`, boxShadow: `0 0 26px ${AMD}` }} />
        </div>

        <div className="absolute inset-0 flex flex-col items-center justify-center select-none">
          <h1 className="text-6xl font-black tracking-[0.52em] leading-none" style={{ color: rc, textShadow: SAFE_HUD ? "none" : `0 0 26px ${gd}, 0 0 52px ${gd}, 0 0 110px rgba(249,115,22,0.14)` }}>J.A.R.V.I.S</h1>
          <p className="text-[11px] font-medium tracking-[0.45em] mt-3 text-zinc-700">JUST A RATHER VERY INTELLIGENT SYSTEM</p>
          <p className="text-[14px] font-bold tracking-[0.32em] mt-4 transition-colors duration-300" style={{ color: phaseAccent }}>{phaseLabel}</p>
          <p className="text-[11px] tracking-[0.34em] mt-2 text-zinc-600">NEURAL RESPONSE MATRIX // ARCLIGHT SUITE</p>
          <p className="text-[9px] font-black tracking-[0.5em] mt-4 text-orange-400/70">MARK VII DEFENSE SHELL</p>
        </div>

        {majorTicks.map(a => {
          const rad = ((a-90)*Math.PI)/180; const d = R1+16;
          return <span key={a} className="absolute text-[8px] font-mono text-zinc-800/40 select-none" style={{ left:Cx+Math.cos(rad)*d, top:Cx+Math.sin(rad)*d, transform:"translate(-50%,-50%)" }}>{String(a).padStart(3,"0")}</span>;
        })}

        {[0, 1, 2, 3].map(i => {
          const size = 34;
          const off = 170;
          const positions = [
            { left: Cx - off, top: Cx - off },
            { left: Cx + off - size, top: Cx - off },
            { left: Cx - off, top: Cx + off - size },
            { left: Cx + off - size, top: Cx + off - size },
          ][i];
          return <div key={i} className={`hud-bracket ${i === 0 ? "tl" : i === 1 ? "tr" : i === 2 ? "bl" : "br"}`} style={positions} />;
        })}
      </div>
    </div>
  );
});

const OrbitBadges = memo(function OrbitBadges({ orbitData, rc, gd }: { orbitData: OrbitItem[]; rc: string; gd: string }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center z-[11] pointer-events-none">
      <div className="relative contain-layout overflow-visible" style={{ width: HUD, height: HUD }}>
        <div className="orbit-ring absolute inset-0 pointer-events-none will-transform">
          {orbitData.map((item, i, arr) => {
            const angle = (360 / arr.length) * i - 90;
            return (
              <div key={item.l} className="absolute left-1/2 top-1/2 w-0 h-0 orbit-anchor will-transform" style={{ transform: `translate(-50%, -50%) rotate(${angle}deg)` }}>
                <div className="orbit-radius-anchor w-0 h-0 will-transform" style={{ transform: `translateY(-${ORBIT_R}px)` }}>
                  <div className="orbit-counter-rotate w-0 h-0 will-transform">
                    <div className="text-center whitespace-nowrap will-transform orbit-badge-shell" style={{ width: ORBIT_BADGE_W, transform: `translate(-50%, -50%) rotate(${-angle}deg)` }}>
                      <div className="text-[9px] font-bold tracking-[0.22em] text-zinc-500">{item.l}</div>
                      <div className="px-2 py-0.5 rounded-full border text-[18px] font-black font-mono tracking-wide" style={{ color: item.w ? RD : rc, borderColor: item.w ? "rgba(239,68,68,0.25)" : "rgba(34,211,238,0.18)", background: item.w ? "rgba(127,29,29,0.18)" : "rgba(8,20,40,0.28)", textShadow: SAFE_HUD ? "none" : `0 0 12px ${item.w ? RDD : gd}` }}>{item.v}</div>
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
});

// ── Audio Canvas constants ────────────────────────────────────
const AUDIO_RING_LEN = 128;
const AUDIO_INNER_R = R1 + 16;
const AUDIO_BAR_MAX = 72;
const AUDIO_TRAIL_COUNT = 40;
const AUDIO_TRAIL_ARC = Math.PI * 0.82;
const AUDIO_RMS_CEIL = 2200;
const WAKE_ARC_R = R1 + 8;
const WAKE_THRESHOLD = 0.42;
const PARTICLE_COUNT = 120;
const PARTICLE_SPAWN_R_MIN = R1 - 20;
const PARTICLE_SPAWN_R_MAX = R1 + 60;
const SCANNER_SPEED = 0.4; // radians per second
const ENERGY_PULSE_INTERVAL = 3000; // ms
const LAUNCHER_STORAGE_KEY = "arclight.launcher.hexes.v6";
const DEFAULT_LAUNCHER_HEXES: Array<LauncherHex & { matchers: string[] }> = [
  { id: "browser", label: "Browser", appId: "", x: 90, y: 0, accent: "#60a5fa", matchers: ["firefox", "browser", "chrom", "brave"] },
  { id: "files", label: "Files", appId: "", x: 0, y: 52, accent: "#34d399", matchers: ["thunar", "files", "dolphin", "nautilus"] },
  { id: "steam", label: "Steam", appId: "", x: 180, y: 52, accent: "#38bdf8", matchers: ["steam"] },
  { id: "code", label: "Code", appId: "", x: 90, y: 104, accent: "#818cf8", matchers: ["code", "vscodium", "visual studio code"] },
  { id: "music", label: "Music", appId: "", x: 0, y: 156, accent: "#f59e0b", matchers: ["spotify", "music"] },
  { id: "terminal", label: "Terminal", appId: "", x: 180, y: 156, accent: "#f472b6", matchers: ["kitty", "konsole", "terminal", "alacritty"] },
  { id: "discord", label: "Discord", appId: "", x: 90, y: 208, accent: "#8b5cf6", matchers: ["discord", "vesktop"] },
];

function loadLauncherHexes(): LauncherHex[] {
  if (typeof window === "undefined") {
    return DEFAULT_LAUNCHER_HEXES.map(({ matchers: _matchers, ...hex }) => hex);
  }
  try {
    const raw = window.localStorage.getItem(LAUNCHER_STORAGE_KEY);
    if (!raw) {
      return DEFAULT_LAUNCHER_HEXES.map(({ matchers: _matchers, ...hex }) => hex);
    }
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) {
      return DEFAULT_LAUNCHER_HEXES.map(({ matchers: _matchers, ...hex }) => hex);
    }
    const byId = new Map<string, LauncherHex>();
    for (const item of parsed) {
      if (!item || typeof item !== "object") continue;
      const { id, label, appId, x, y, accent } = item as Partial<LauncherHex>;
      if (typeof id !== "string" || typeof label !== "string" || typeof appId !== "string" || typeof x !== "number" || typeof y !== "number" || typeof accent !== "string") {
        continue;
      }
      byId.set(id, { id, label, appId, x, y, accent });
    }
    return DEFAULT_LAUNCHER_HEXES.map(({ matchers: _matchers, ...hex }) => byId.get(hex.id) ?? hex);
  } catch {
    return DEFAULT_LAUNCHER_HEXES.map(({ matchers: _matchers, ...hex }) => hex);
  }
}

function resolveDefaultAppId(apps: DesktopApp[], matchers: string[]): string {
  const lowered = apps.map(app => ({ ...app, haystack: `${app.id} ${app.name}`.toLowerCase() }));
  for (const matcher of matchers) {
    const found = lowered.find(app => app.haystack.includes(matcher));
    if (found) return found.id;
  }
  return "";
}

function desktopIconSrc(app?: DesktopApp): string | null {
  if (!app?.iconPath) return null;
  return convertFileSrc(app.iconPath);
}

function hydrateLauncherHexes(hexes: LauncherHex[], apps: DesktopApp[]): LauncherHex[] {
  const known = new Set(apps.map(app => app.id));
  return hexes.map(hex => {
    if (hex.appId && known.has(hex.appId)) {
      return hex;
    }
    const fallback = DEFAULT_LAUNCHER_HEXES.find(item => item.id === hex.id);
    if (!fallback) {
      return { ...hex, appId: "" };
    }
    return { ...hex, appId: resolveDefaultAppId(apps, fallback.matchers) };
  });
}

interface Particle {
  x: number; y: number; vx: number; vy: number;
  life: number; maxLife: number; size: number;
  hue: number; bright: number;
}

/** High-performance Canvas overlay: AudioRing + WakeArc + particles + scanner + shockwaves.
 *  Full-viewport canvas — no clipping. Runs entirely on requestAnimationFrame. */
const AudioCanvas = memo(function AudioCanvas({ telemRef, phaseRef, activatedAtRef }: {
  telemRef: RefObject<{ ring: number[]; head: number; rms: number; peak: number; wake: number; state: string; rmsHist: number[]; wakeHist: number[] }>;
  phaseRef: RefObject<string>;
  activatedAtRef: RefObject<number>;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const particlesRef = useRef<Particle[]>([]);
  const scanAngleRef = useRef(0);
  const visualRingRef = useRef<number[]>(new Array(AUDIO_RING_LEN).fill(0));
  const lastPulseRef = useRef(0);
  const pulsesRef = useRef<{ r: number; birth: number; intensity: number }[]>([]);
  const prevTimeRef = useRef(0);
  const smoothRmsRef = useRef(0);
  const shockwavesRef = useRef<{ r: number; birth: number; intensity: number }[]>([]);
  const lastActivatedRef = useRef(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    let cw = 0, ch = 0;
    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      cw = window.innerWidth; ch = window.innerHeight;
      canvas.width = cw * dpr; canvas.height = ch * dpr;
    };
    resize();
    window.addEventListener("resize", resize);
    const ctx = canvas.getContext("2d", { alpha: true })!;

    // Spawn initial particles
    const particles = particlesRef.current;
    for (let i = particles.length; i < PARTICLE_COUNT; i++) {
      particles.push(spawnParticle());
    }

    let raf = 0;
    const draw = (timestamp: number) => {
      const dt = prevTimeRef.current ? (timestamp - prevTimeRef.current) / 1000 : 0.016;
      prevTimeRef.current = timestamp;
      const t = telemRef.current!;
      const phase = phaseRef.current!;
      const isListening = phase === "listening";
      const isProcessing = phase === "processing";
      const isSpeaking = phase === "speaking";

      ctx.clearRect(0, 0, HUD, HUD);

      // ── Viewport offset (translate so Cx,Cx maps to screen center) ──
      const dpr = window.devicePixelRatio || 1;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const offX = cw / 2 - Cx;
      const offY = ch / 2 - Cx;
      ctx.translate(offX, offY);
      ctx.clearRect(-offX, -offY, cw, ch);

      // Smooth RMS for VU effects
      const targetRms = Math.min(t.rms / AUDIO_RMS_CEIL, 1);
      smoothRmsRef.current += (targetRms - smoothRmsRef.current) * Math.min(1, dt * 12);
      const sRms = smoothRmsRef.current;

      // ── Check activation (shockwave trigger) ──
      const activAt = activatedAtRef.current;
      if (activAt > lastActivatedRef.current) {
        lastActivatedRef.current = activAt;
        for (let i = 0; i < 5; i++) {
          shockwavesRef.current.push({ r: RC * 0.3, birth: timestamp + i * 100, intensity: 1.5 - i * 0.2 });
        }
        for (let j = 0; j < 40; j++) {
          const bp = spawnParticle();
          bp.size *= 2.5; bp.bright = 1.8;
          bp.hue = Math.random() < 0.5 ? 190 : 30;
          const a2 = Math.random() * Math.PI * 2;
          bp.vx = Math.cos(a2) * (40 + Math.random() * 60);
          bp.vy = Math.sin(a2) * (40 + Math.random() * 60);
          particlesRef.current.push(bp);
        }
      }

      // ── Energy pulses (expanding rings from core) ──────────
      const now = timestamp;
      if (now - lastPulseRef.current > ENERGY_PULSE_INTERVAL || (t.wake > 0.3 && now - lastPulseRef.current > 800)) {
        lastPulseRef.current = now;
        pulsesRef.current.push({ r: RC * 0.5, birth: now, intensity: 0.3 + t.wake * 0.7 });
      }
      const pulses = pulsesRef.current;
      for (let i = pulses.length - 1; i >= 0; i--) {
        const p = pulses[i];
        const age = (now - p.birth) / 1000;
        p.r += dt * 180;
        const alpha = Math.max(0, p.intensity * (1 - age / 2.5));
        if (alpha <= 0 || p.r > R1 + 80) { pulses.splice(i, 1); continue; }
        ctx.beginPath();
        ctx.arc(Cx, Cx, p.r, 0, Math.PI * 2);
        ctx.strokeStyle = isListening ? `rgba(239,68,68,${alpha * 0.3})` : `rgba(34,211,238,${alpha * 0.2})`;
        ctx.lineWidth = 1.5 + alpha * 2;
        ctx.stroke();
        // Second ghost ring
        ctx.beginPath();
        ctx.arc(Cx, Cx, p.r + 4, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(249,115,22,${alpha * 0.1})`;
        ctx.lineWidth = 1;
        ctx.stroke();
      }

      // ── Scanner beam (smooth radar master) ─────────────────
      scanAngleRef.current += SCANNER_SPEED * dt * (isListening ? 3 : isSpeaking ? 1.8 : 1);
      const sa = scanAngleRef.current;
      const headIndex = ((t.head % AUDIO_RING_LEN) + AUDIO_RING_LEN) % AUDIO_RING_LEN;
      const headAngle = (headIndex * 360 / AUDIO_RING_LEN - 90) * Math.PI / 180;
      let sweepOffset = sa - headAngle;
      while (sweepOffset > Math.PI) sweepOffset -= Math.PI * 2;
      while (sweepOffset < -Math.PI) sweepOffset += Math.PI * 2;
      const scanGrad = ctx.createConicGradient(sa, Cx, Cx);
      const scanColor = isListening ? "239,68,68" : "34,211,238";
      scanGrad.addColorStop(0, `rgba(${scanColor},0.14)`);
      scanGrad.addColorStop(0.06, `rgba(${scanColor},0.02)`);
      scanGrad.addColorStop(0.07, "rgba(0,0,0,0)");
      scanGrad.addColorStop(1, "rgba(0,0,0,0)");
      ctx.beginPath();
      ctx.arc(Cx, Cx, R1 + 2, 0, Math.PI * 2);
      ctx.fillStyle = scanGrad;
      ctx.fill();

      // Scanner line
      const slx = Cx + Math.cos(sa) * (RC + 20);
      const sly = Cx + Math.sin(sa) * (RC + 20);
      const slx2 = Cx + Math.cos(sa) * (R1 + 8);
      const sly2 = Cx + Math.sin(sa) * (R1 + 8);
      const scanLineGrad = ctx.createLinearGradient(slx, sly, slx2, sly2);
      scanLineGrad.addColorStop(0, "rgba(0,0,0,0)");
      scanLineGrad.addColorStop(0.3, `rgba(${scanColor},0.25)`);
      scanLineGrad.addColorStop(1, `rgba(${scanColor},0.5)`);
      ctx.beginPath();
      ctx.moveTo(slx, sly);
      ctx.lineTo(slx2, sly2);
      ctx.strokeStyle = scanLineGrad;
      ctx.lineWidth = 1.5;
      ctx.stroke();
      // Scanner tip glow
      ctx.beginPath();
      ctx.arc(slx2, sly2, 4, 0, Math.PI * 2);
      ctx.fillStyle = `rgba(${scanColor},0.6)`;
      ctx.fill();

      // ── Radar-coupled persistent audio ring buffer ────────
      const visualRing = visualRingRef.current;
      const decay = Math.exp(-dt * 2.6);
      for (let i = 0; i < AUDIO_RING_LEN; i++) {
        visualRing[i] *= decay;
        if (visualRing[i] < 0.5) {
          visualRing[i] = 0;
        }
      }
      const normalizedScan = ((sa + Math.PI / 2) / (Math.PI * 2) % 1 + 1) % 1;
      const scanSlotFloat = normalizedScan * AUDIO_RING_LEN;
      const slotA = Math.floor(scanSlotFloat) % AUDIO_RING_LEN;
      const slotB = (slotA + 1) % AUDIO_RING_LEN;
      const slotFrac = scanSlotFloat - Math.floor(scanSlotFloat);
      const depositedRms = Math.max(t.rms, sRms * AUDIO_RMS_CEIL);
      visualRing[slotA] = Math.max(visualRing[slotA], depositedRms * (1 - slotFrac));
      visualRing[slotB] = Math.max(visualRing[slotB], depositedRms * slotFrac);

      // ── Shockwaves (expanding rings from activation) ──────
      const shocks = shockwavesRef.current;
      for (let i = shocks.length - 1; i >= 0; i--) {
        const s = shocks[i];
        if (timestamp < s.birth) continue;
        const age = (timestamp - s.birth) / 1000;
        s.r += dt * 450;
        const alpha = Math.max(0, s.intensity * (1 - age / 1.8));
        if (alpha <= 0 || s.r > Math.max(cw, ch)) { shocks.splice(i, 1); continue; }
        const shockColor = isListening ? "239,68,68" : "34,211,238";
        ctx.beginPath(); ctx.arc(Cx, Cx, s.r, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(${shockColor},${alpha * 0.5})`; ctx.lineWidth = 2.5 + alpha * 5; ctx.stroke();
        ctx.beginPath(); ctx.arc(Cx, Cx, s.r, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(255,255,255,${alpha * 0.08})`; ctx.lineWidth = 16 + alpha * 14; ctx.stroke();
        ctx.beginPath(); ctx.arc(Cx, Cx, s.r - 6, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(249,115,22,${alpha * 0.25})`; ctx.lineWidth = 1.5; ctx.stroke();
      }

      // ── Base ring (always visible, breathes with volume) ──
      const baseAlpha = 0.08 + sRms * 0.32;
      ctx.beginPath(); ctx.arc(Cx, Cx, AUDIO_INNER_R, 0, Math.PI * 2);
      ctx.strokeStyle = isListening ? `rgba(239,68,68,${baseAlpha})` : `rgba(34,211,238,${baseAlpha})`;
      ctx.lineWidth = 1 + sRms * 2.5; ctx.stroke();
      // VU glow halo
      if (sRms > 0.03) {
        const glowC = sRms > 0.6 ? "239,68,68" : sRms > 0.25 ? "245,158,11" : isListening ? "239,68,68" : "34,211,238";
        ctx.beginPath(); ctx.arc(Cx, Cx, AUDIO_INNER_R, 0, Math.PI * 2);
        ctx.strokeStyle = `rgba(${glowC},${sRms * 0.1})`; ctx.lineWidth = 10 + sRms * 28; ctx.stroke();
      }

      // ── Tick marks around base ring ────────────────────────
      for (let a = 0; a < 360; a += 5) {
        const rad = (a - 90) * Math.PI / 180;
        const isMajor = a % 30 === 0, isMid = a % 10 === 0;
        const len = isMajor ? 10 : isMid ? 6 : 3;
        ctx.beginPath();
        ctx.moveTo(Cx + Math.cos(rad) * (AUDIO_INNER_R - len - 2), Cx + Math.sin(rad) * (AUDIO_INNER_R - len - 2));
        ctx.lineTo(Cx + Math.cos(rad) * (AUDIO_INNER_R - 2), Cx + Math.sin(rad) * (AUDIO_INNER_R - 2));
        ctx.strokeStyle = isListening
          ? `rgba(239,68,68,${isMajor ? 0.15 + sRms * 0.2 : 0.04 + sRms * 0.06})`
          : `rgba(34,211,238,${isMajor ? 0.15 + sRms * 0.2 : 0.04 + sRms * 0.06})`;
        ctx.lineWidth = isMajor ? 1.5 : 0.5; ctx.stroke();
      }

      // ── Audio Ring (bars) ──────────────────────────────────
      for (let i = 0; i < AUDIO_RING_LEN; i++) {
        const angle = (i * 360 / AUDIO_RING_LEN - 90) * Math.PI / 180;
        let age = scanSlotFloat - i;
        while (age < 0) age += AUDIO_RING_LEN;
        while (age >= AUDIO_RING_LEN) age -= AUDIO_RING_LEN;
        const rms = visualRing[i] || 0;
        const norm = Math.min(rms / AUDIO_RMS_CEIL, 1);
        const barLen = 5 + norm * AUDIO_BAR_MAX;
        const freshness = 1 - age / AUDIO_RING_LEN;
        const alpha = Math.max(0.08, freshness * (0.35 + norm * 0.65));
        const isHead = age < 1;
        const cos = Math.cos(angle), sin = Math.sin(angle);
        const x1 = Cx + cos * AUDIO_INNER_R;
        const y1 = Cx + sin * AUDIO_INNER_R;
        const x2 = Cx + cos * (AUDIO_INNER_R + barLen);
        const y2 = Cx + sin * (AUDIO_INNER_R + barLen);

        // Color: red if loud, amber if mid, cyan/red by phase
        let r: number, g: number, b: number;
        if (norm > 0.75) { r = 239; g = 68; b = 68; }
        else if (norm > 0.35) { r = 245; g = 158; b = 11; }
        else if (isListening) { r = 239; g = 68; b = 68; }
        else { r = 34; g = 211; b = 238; }

        ctx.beginPath();
        ctx.moveTo(x1, y1);
        ctx.lineTo(x2, y2);
        ctx.strokeStyle = `rgba(${r},${g},${b},${alpha})`;
        ctx.lineWidth = isHead ? 4 : 2.5;
        ctx.lineCap = "round";
        ctx.stroke();

        // Glow on loud bars
        if (norm > 0.5 && freshness > 0.3) {
          ctx.beginPath(); ctx.moveTo(x1, y1); ctx.lineTo(x2, y2);
          ctx.strokeStyle = `rgba(${r},${g},${b},${alpha * 0.15})`; ctx.lineWidth = 8; ctx.stroke();
        }

        // Head glow
        if (isHead) {
          ctx.beginPath();
          ctx.arc(x2, y2, 6 + norm * 6, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(${r},${g},${b},${0.3 + norm * 0.4})`;
          ctx.fill();
        }

        // Outer tip dot for high-energy bars
        if (norm > 0.5 && freshness > 0.5) {
          ctx.beginPath();
          ctx.arc(x2, y2, 1.5, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(255,255,255,${alpha * 0.6})`;
          ctx.fill();
        }
      }

      // ── Wake Arc ────────────────────────────────────────────
      const wakeStartAngle = -225 * Math.PI / 180;
      const wakeSweep = 270 * Math.PI / 180;
      // Background track
      ctx.beginPath();
      ctx.arc(Cx, Cx, WAKE_ARC_R, wakeStartAngle, wakeStartAngle + wakeSweep);
      ctx.strokeStyle = "rgba(255,255,255,0.04)";
      ctx.lineWidth = 4;
      ctx.stroke();

      // Fill arc
      const wakeFill = t.wake * wakeSweep;
      if (wakeFill > 0.01) {
        const wakeColor = t.wake > WAKE_THRESHOLD ? "239,68,68" : t.wake > 0.25 ? "245,158,11" : "34,197,94";
        const glowI = Math.min(t.wake / WAKE_THRESHOLD, 1);
        ctx.beginPath();
        ctx.arc(Cx, Cx, WAKE_ARC_R, wakeStartAngle, wakeStartAngle + wakeFill);
        ctx.strokeStyle = `rgba(${wakeColor},${0.5 + glowI * 0.5})`;
        ctx.lineWidth = 4;
        ctx.lineCap = "round";
        ctx.stroke();
        // Glow layer
        if (glowI > 0.5) {
          ctx.beginPath();
          ctx.arc(Cx, Cx, WAKE_ARC_R, wakeStartAngle, wakeStartAngle + wakeFill);
          ctx.strokeStyle = `rgba(${wakeColor},${glowI * 0.15})`;
          ctx.lineWidth = 12;
          ctx.stroke();
        }
        // Fill tip
        const tipAngle = wakeStartAngle + wakeFill;
        const tx = Cx + Math.cos(tipAngle) * WAKE_ARC_R;
        const ty = Cx + Math.sin(tipAngle) * WAKE_ARC_R;
        ctx.beginPath();
        ctx.arc(tx, ty, 3 + glowI * 4, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${wakeColor},${0.5 + glowI * 0.3})`;
        ctx.fill();
      }

      // Threshold marker
      const threshAngle = wakeStartAngle + WAKE_THRESHOLD * wakeSweep;
      const thx = Cx + Math.cos(threshAngle) * WAKE_ARC_R;
      const thy = Cx + Math.sin(threshAngle) * WAKE_ARC_R;
      ctx.beginPath();
      ctx.arc(thx, thy, 4, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(245,158,11,0.5)";
      ctx.fill();
      ctx.beginPath();
      ctx.arc(thx, thy, 2, 0, Math.PI * 2);
      ctx.fillStyle = "rgba(245,158,11,1)";
      ctx.fill();

      // ── Particles ──────────────────────────────────────────
      const pts = particlesRef.current;
      // Spawn new particles based on RMS energy
      const spawnRate = 0.3 + (t.rms / AUDIO_RMS_CEIL) * 3;
      if (Math.random() < spawnRate * dt * 60) {
        if (pts.length < PARTICLE_COUNT * 1.5) pts.push(spawnParticle());
      }
      // Wake burst particles
      if (t.wake > WAKE_THRESHOLD && Math.random() < 0.3) {
        const bp = spawnParticle();
        bp.hue = 0; bp.bright = 1.5; bp.size *= 2;
        pts.push(bp);
      }

      for (let i = pts.length - 1; i >= 0; i--) {
        const p = pts[i];
        p.life -= dt;
        if (p.life <= 0) { pts.splice(i, 1); continue; }
        p.x += p.vx * dt;
        p.y += p.vy * dt;
        // Gentle outward drift
        const dx = p.x - Cx, dy = p.y - Cx;
        const dist = Math.sqrt(dx * dx + dy * dy);
        if (dist > 0) {
          p.vx += (dx / dist) * 8 * dt;
          p.vy += (dy / dist) * 8 * dt;
        }
        const lifeRatio = p.life / p.maxLife;
        const fadeIn = Math.min(lifeRatio * 5, 1);
        const fadeOut = Math.min((1 - lifeRatio) * 10, 1);
        const alpha = fadeIn * (1 - fadeOut) < 0 ? 0 : Math.min(fadeIn, 1 - (1 - fadeOut < 0 ? 0 : 1 - fadeOut));
        const computedAlpha = Math.max(0, Math.min(1, lifeRatio < 0.8 ? lifeRatio * 1.2 : (1 - lifeRatio) * 5)) * p.bright;
        const sz = p.size * (0.5 + lifeRatio * 0.5);

        // Color based on hue: 0=red, 190=cyan, 30=amber
        let pr: number, pg: number, pb: number;
        if (p.hue < 15) { pr = 239; pg = 68; pb = 68; }
        else if (p.hue < 45) { pr = 245; pg = 158; pb = 11; }
        else { pr = 34; pg = 211; pb = 238; }

        ctx.beginPath();
        ctx.arc(p.x, p.y, sz, 0, Math.PI * 2);
        ctx.fillStyle = `rgba(${pr},${pg},${pb},${computedAlpha * 0.6})`;
        ctx.fill();
        // Bright core
        if (sz > 1.5) {
          ctx.beginPath();
          ctx.arc(p.x, p.y, sz * 0.4, 0, Math.PI * 2);
          ctx.fillStyle = `rgba(255,255,255,${computedAlpha * 0.3})`;
          ctx.fill();
        }
      }

      // ── Hex grid overlay (subtle) ──────────────────────────
      const hexR = 28;
      const hexH = hexR * Math.sqrt(3);
      ctx.strokeStyle = `rgba(34,211,238,0.018)`;
      ctx.lineWidth = 0.5;
      for (let row = -1; row < HUD / hexH + 1; row++) {
        for (let col = -1; col < HUD / (hexR * 1.5) + 1; col++) {
          const cx2 = col * hexR * 1.5;
          const cy2 = row * hexH + (col % 2 ? hexH / 2 : 0);
          const d2 = Math.sqrt((cx2 - Cx) ** 2 + (cy2 - Cx) ** 2);
          if (d2 < RC + 40 || d2 > R1 + 30) continue;
          ctx.beginPath();
          for (let s = 0; s < 6; s++) {
            const a = (s * 60 - 30) * Math.PI / 180;
            const hx = cx2 + hexR * 0.7 * Math.cos(a);
            const hy = cy2 + hexR * 0.7 * Math.sin(a);
            s === 0 ? ctx.moveTo(hx, hy) : ctx.lineTo(hx, hy);
          }
          ctx.closePath();
          ctx.stroke();
        }
      }

      raf = requestAnimationFrame(draw);
    };

    raf = requestAnimationFrame(draw);
    return () => { cancelAnimationFrame(raf); window.removeEventListener("resize", resize); };
  }, [telemRef, phaseRef, activatedAtRef]);

  return <canvas ref={canvasRef} className="absolute inset-0 z-[9] pointer-events-none" />;
});

function spawnParticle(): Particle {
  const angle = Math.random() * Math.PI * 2;
  const r = PARTICLE_SPAWN_R_MIN + Math.random() * (PARTICLE_SPAWN_R_MAX - PARTICLE_SPAWN_R_MIN);
  const speed = 5 + Math.random() * 20;
  const outAngle = angle + (Math.random() - 0.5) * 0.8;
  return {
    x: Cx + Math.cos(angle) * r,
    y: Cx + Math.sin(angle) * r,
    vx: Math.cos(outAngle) * speed,
    vy: Math.sin(outAngle) * speed,
    life: 1.5 + Math.random() * 3,
    maxLife: 1.5 + Math.random() * 3,
    size: 0.8 + Math.random() * 2.5,
    hue: Math.random() < 0.15 ? 30 : Math.random() < 0.3 ? 0 : 190,
    bright: 0.4 + Math.random() * 0.6,
  };
}

// ── Component ────────────────────────────────────────────────
export default function Assistant() {
  const navigate = useNavigate();
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [listening, setListening] = useState(false);
  const [loading, setLoading] = useState(false);
  const [speaking, setSpeaking] = useState(false);
  const [ttsEnabled, setTtsEnabled] = useState(true);
  const [llmOnline, setLlmOnline] = useState<boolean | null>(null);
  const [jarvisEnabled, setJarvisEnabled] = useState(true);
  const [chatExpanded, setChatExpanded] = useState(false);
  const [activated, setActivated] = useState(false);
  const [sysmon, setSysmon] = useState<SystemMonitorData | null>(null);
  const [gpu, setGpu] = useState<GpuOcStatus | null>(null);
  // PC history
  const [cpuHist, setCpuHist] = useState<number[]>([]);
  const [gpuTempHist, setGpuTempHist] = useState<number[]>([]);
  const [gpuLoadHist, setGpuLoadHist] = useState<number[]>([]);
  const [gpuVramHist, setGpuVramHist] = useState<number[]>([]);
  const [ramHist, setRamHist] = useState<number[]>([]);
  const [gpuPwrHist, setGpuPwrHist] = useState<number[]>([]);
  // Pi4 tent data
  const [tent, setTent] = useState<PiTentStatus["sensor"]>(null);
  const [tentLight, setTentLight] = useState<PiTentStatus["light"]>(null);
  const [tentTank, setTentTank] = useState<PiTentStatus["tank"]>(null);
  const [tentTempHist, setTentTempHist] = useState<number[]>([]);
  const [tentHumiHist, setTentHumiHist] = useState<number[]>([]);
  const [tentVpdHist, setTentVpdHist] = useState<number[]>([]);
  const [tentBrightHist, setTentBrightHist] = useState<number[]>([]);
  const [tentError, setTentError] = useState<string | null>(null);
  const [desktopApps, setDesktopApps] = useState<DesktopApp[]>([]);
  const [launcherHexes, setLauncherHexes] = useState<LauncherHex[]>(() => loadLauncherHexes());
  const [launcherError, setLauncherError] = useState<string | null>(null);
  const [editingHexId, setEditingHexId] = useState<string | null>(null);
  const [launcherQuery, setLauncherQuery] = useState("");
  const [launcherDraft, setLauncherDraft] = useState<LauncherDraft>({ label: "", appId: "" });

  // ── Audio telemetry (ref-driven, no re-renders) ────────────
  const [audioTelem, setAudioTelem] = useState<AudioTelemetry | null>(null);
  const [voicePhase, setVoicePhase] = useState<Phase | null>(null);
  const audioTelemRef = useRef({ ring: new Array(AUDIO_RING_LEN).fill(0), head: 0, rms: 0, peak: 0, wake: 0, state: "idle", rmsHist: [] as number[], wakeHist: [] as number[] });
  const phaseStrRef = useRef("idle");
  const activatedAtRef = useRef(0);
  // Legacy state arrays for sidebar sparklines only (updated at lower rate)
  const [rmsHist, setRmsHist] = useState<number[]>([]);
  const [wakeHist, setWakeHist] = useState<number[]>([]);
  const sparkThrottle = useRef(0);

  const chatEndRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const phase: Phase = voicePhase ?? (listening ? "listening" : loading ? "processing" : speaking ? "speaking" : "idle");
  const rc = phase === "listening" ? RD : CY;
  const rc2 = phase === "listening" ? "#b91c1c" : CY2;
  const gd = phase === "listening" ? RDD : CYD;
  const phaseClass = phase === "listening" ? "ph-listen" : phase === "processing" ? "ph-process" : phase === "speaking" ? "ph-speak" : "ph-idle";
  const phaseLabel = phase === "listening" ? "● REC" : phase === "processing" ? "◆ PROCESSING" : phase === "speaking" ? "◆ SPEAKING" : "ONLINE";

  const ticks = useMemo(() => Array.from({ length: 72 }, (_, i) => i * 5), []);
  const majorTicks = useMemo(() => Array.from({ length: 12 }, (_, i) => i * 30), []);

  useEffect(() => { chatEndRef.current?.scrollIntoView({ behavior: "smooth" }); }, [messages]);
  useEffect(() => { if (messages.length > 0) setChatExpanded(true); }, [messages]);
  useEffect(() => {
    try {
      window.localStorage.setItem(LAUNCHER_STORAGE_KEY, JSON.stringify(launcherHexes));
    } catch {
      // Ignore storage write failures.
    }
  }, [launcherHexes]);
  useEffect(() => {
    const unlisten = listen("navigate-assistant", () => { setActivated(true); setTimeout(() => setActivated(false), 2500); });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  // ── Audio telemetry event listener (ref-only, throttled state) ──
  useEffect(() => {
    const unlisten = listen<AudioTelemetry>("jarvis-audio-telemetry", (e) => {
      const t = e.payload;
      const d = audioTelemRef.current;
      d.ring[d.head % AUDIO_RING_LEN] = t.rms;
      d.head++;
      d.rms = t.rms;
      d.peak = t.peak;
      d.wake = t.wake;
      d.state = t.state;
      d.rmsHist.push(t.rms);
      if (d.rmsHist.length > HIST) d.rmsHist.shift();
      d.wakeHist.push(t.wake * 2400);
      if (d.wakeHist.length > HIST) d.wakeHist.shift();
      // Throttle React state updates to ~4 Hz for sidebar sparklines
      const now = Date.now();
      if (now - sparkThrottle.current > 250) {
        sparkThrottle.current = now;
        setAudioTelem(t);
        setRmsHist([...d.rmsHist]);
        setWakeHist([...d.wakeHist]);
      }
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);

  useEffect(() => {
    const unlisten = listen<Phase>("jarvis-voice-phase", (e: VoicePhaseEvent) => {
      setVoicePhase(e.payload);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);
  useEffect(() => {
    const unlisten = listen<Message & { source?: string }>("jarvis-chat-message", (e: ChatStreamEvent) => {
      setMessages(prev => [...prev, { role: e.payload.role, content: e.payload.content, actions: e.payload.actions }]);
      setChatExpanded(true);
    });
    return () => { unlisten.then(fn => fn()); };
  }, []);
  // Keep phase ref in sync
  useEffect(() => { phaseStrRef.current = phase; }, [phase]);
  useEffect(() => { if (activated) activatedAtRef.current = performance.now(); }, [activated]);

  useEffect(() => {
    api.listDesktopApps()
      .then(apps => {
        setDesktopApps(apps);
        setLauncherHexes(prev => hydrateLauncherHexes(prev, apps));
      })
      .catch(err => {
        setLauncherError(apiError(err));
      });
  }, []);

  const refreshStatus = useCallback(() => {
    api.assistantStatus().then(([ok]) => setLlmOnline(ok)).catch(() => setLlmOnline(false));
    api.jarvisListenerEnabled().then(e => setJarvisEnabled(e)).catch(() => setJarvisEnabled(false));
  }, []);

  const refreshSysmon = useCallback(() => {
    api.getSystemMonitor().then(d => {
      setSysmon(d);
      setCpuHist(p => [...p.slice(-(HIST - 1)), d.cpu.usage_percent]);
      setRamHist(p => [...p.slice(-(HIST - 1)), d.memory.percent]);
      if (d.gpu.temp_celsius != null) setGpuTempHist(p => [...p.slice(-(HIST - 1)), d.gpu.temp_celsius!]);
      if (d.gpu.gpu_busy_percent != null) setGpuLoadHist(p => [...p.slice(-(HIST - 1)), d.gpu.gpu_busy_percent!]);
      if (d.gpu.vram_used_mib != null) setGpuVramHist(p => [...p.slice(-(HIST - 1)), d.gpu.vram_used_mib!]);
      if (d.gpu.power_watts != null) setGpuPwrHist(p => [...p.slice(-(HIST - 1)), (d.gpu.power_watts! / 350) * 100]);
    }).catch(() => {});
    api.getGpuOcStatus().then(setGpu).catch(() => {});
  }, []);

  // Pi4 tent polling
  const refreshTent = useCallback(() => {
    api.getPiTentStatus().then(data => {
      setTent(data.sensor);
      setTentLight(data.light);
      setTentTank(data.tank);
      setTentError(data.error);
    }).catch(err => {
      setTentError(apiError(err));
    });
  }, []);

  const refreshTentHistory = useCallback(() => {
    api.getPiTentHistory().then(data => {
      setTentTempHist(data.temp_history.slice(-HIST));
      setTentHumiHist(data.humi_history.slice(-HIST));
      setTentVpdHist(data.vpd_history.slice(-HIST));
      setTentBrightHist(data.brightness_history.slice(-HIST));
      if (data.error) setTentError(data.error);
    }).catch(err => {
      setTentError(apiError(err));
    });
  }, []);

  const toggleJarvisEnabled = useCallback(async () => {
    try { const e = await api.jarvisSetListenerEnabled(!jarvisEnabled); setJarvisEnabled(e); } catch { refreshStatus(); }
  }, [jarvisEnabled, refreshStatus]);

  const launchHex = useCallback(async (hex: LauncherHex) => {
    if (!hex.appId) {
      setLauncherError("Diesem Hexagon ist noch kein Programm zugewiesen.");
      setEditingHexId(hex.id);
      setLauncherDraft({ label: hex.label, appId: hex.appId });
      return;
    }
    setLauncherError(null);
    try {
      await api.launchDesktopApp(hex.appId);
    } catch (err) {
      setLauncherError(apiError(err));
    }
  }, []);

  useEffect(() => {
    refreshStatus();
    const t1 = setTimeout(refreshSysmon, 250);
    const t2 = setTimeout(refreshTent, 550);
    const t3 = setTimeout(refreshTentHistory, 900);
    const id1 = setInterval(refreshStatus, 5000);
    const id2 = setInterval(refreshSysmon, 3000);
    const id3 = setInterval(refreshTent, 5000);
    const id4 = setInterval(refreshTentHistory, 60000);
    return () => {
      clearTimeout(t1);
      clearTimeout(t2);
      clearTimeout(t3);
      clearInterval(id1);
      clearInterval(id2);
      clearInterval(id3);
      clearInterval(id4);
    };
  }, [refreshStatus, refreshSysmon, refreshTent, refreshTentHistory]);

  const speak = useCallback((text: string) => {
    if (!ttsEnabled || !text.trim()) return;
    setSpeaking(true);
    api.jarvisSpeak(text).catch(() => {}).finally(() => { setTimeout(() => setSpeaking(false), Math.min(Math.max(text.length * 80, 2000), 15000)); });
  }, [ttsEnabled]);

  const sendMessage = useCallback(async (text: string) => {
    if (!text.trim() || loading) return;
    const userMsg: Message = { role: "user", content: text.trim() };
    const hist = [...messages, userMsg];
    setMessages(hist); setInput(""); setLoading(true);
    try {
      const resp = await api.assistantChat(hist.slice(-20).map(m => ({ role: m.role, content: m.content })));
      setMessages(p => [...p, { role: "assistant", content: resp.text, actions: resp.actions.length > 0 ? resp.actions : undefined }]);
      setLlmOnline(true); speak(resp.text);
    } catch (e) { refreshStatus(); setMessages(p => [...p, { role: "assistant", content: `⚠️ ${apiError(e)}` }]); }
    finally { setLoading(false); inputRef.current?.focus(); }
  }, [messages, loading, speak]);

  const toggleMic = useCallback(async () => {
    if (listening || loading) return;
    setListening(true);
    try { const t = await api.jarvisListen(); setListening(false); setLlmOnline(true); if (t) sendMessage(t); }
    catch { refreshStatus(); setListening(false); }
  }, [listening, loading, refreshStatus, sendMessage]);

  // ── Derived system values ──
  const cpuUsage = sysmon?.cpu.usage_percent ?? 0;
  const cpuCores = sysmon?.cpu.cores ?? 0;
  const cpuThreads = sysmon?.cpu.threads ?? 0;
  const cpuFreq = sysmon?.cpu.frequency_mhz ?? 0;
  const perCore = sysmon?.cpu.per_core_usage ?? [];
  const ramPct = sysmon?.memory.percent ?? 0;
  const ramUsed = sysmon ? (sysmon.memory.used_mib / 1024).toFixed(1) : "—";
  const ramTotal = sysmon ? (sysmon.memory.total_mib / 1024).toFixed(0) : "—";
  const swapPct = sysmon?.swap.percent ?? 0;
  const gpuTemp = gpu?.temp_edge ?? sysmon?.gpu.temp_celsius ?? 0;
  const gpuJnc = gpu?.temp_junction ?? 0;
  const gpuMem = gpu?.temp_mem ?? 0;
  const gpuLoad = gpu?.gpu_busy_percent ?? sysmon?.gpu.gpu_busy_percent ?? 0;
  const gpuPower = gpu?.power_current_w ?? sysmon?.gpu.power_watts ?? 0;
  const gpuPowerCap = gpu?.power_cap_w ?? 350;
  const gpuClock = gpu?.current_sclk_mhz ?? sysmon?.gpu.gpu_clock_mhz ?? 0;
  const gpuMclk = gpu?.current_mclk_mhz ?? 0;
  const gpuVramUsed = sysmon?.gpu.vram_used_mib ?? 0;
  const gpuVramTotal = sysmon?.gpu.vram_total_mib ?? gpu?.vram_mb ?? 24 * 1024;
  const gpuFanRpm = gpu?.fan_rpm ?? 0;
  const cpuTemp = sysmon?.cpu_sensor.temp_celsius ?? 0;
  const cpuPower = sysmon?.cpu_sensor.power_watts ?? 0;
  const uptime = sysmon?.uptime.formatted ?? "—";
  const load1 = sysmon?.load.one ?? 0;
  const load5 = sysmon?.load.five ?? 0;
  const load15 = sysmon?.load.fifteen ?? 0;
  const totalPower = cpuPower + gpuPower;
  const orbitData = useMemo<OrbitItem[]>(() => [
    { l: "GPU", v: `${gpuTemp.toFixed(0)}°C`, w: gpuTemp > 80 },
    { l: "CPU", v: `${cpuUsage.toFixed(0)}%`, w: cpuUsage > 85 },
    { l: "RAM", v: `${ramPct.toFixed(0)}%`, w: ramPct > 85 },
    { l: "PWR", v: `${gpuPower.toFixed(0)}W` },
    { l: "ZELT", v: tent ? `${tent.temp.toFixed(0)}°` : "—" },
    { l: "H₂O", v: tentTank ? `${tentTank.percent.toFixed(0)}%` : "—" },
    { l: "FAN", v: `${gpuFanRpm}` },
    { l: "LOAD", v: `${gpuLoad}%`, w: gpuLoad > 95 },
    { l: "LICHT", v: tentLight ? `${tentLight.brightness}%` : "—" },
    { l: "HUMI", v: tent ? `${tent.humi.toFixed(0)}%` : "—" },
  ], [cpuUsage, gpuFanRpm, gpuLoad, gpuPower, gpuTemp, ramPct, tent, tentLight, tentTank]);
  const appById = useMemo(() => new Map(desktopApps.map(app => [app.id, app] as const)), [desktopApps]);
  const editingHex = useMemo(() => launcherHexes.find(hex => hex.id === editingHexId) ?? null, [editingHexId, launcherHexes]);
  const filteredDesktopApps = useMemo(() => {
    const query = launcherQuery.trim().toLowerCase();
    if (!query) return desktopApps;
    return desktopApps.filter(app => `${app.name} ${app.id}`.toLowerCase().includes(query));
  }, [desktopApps, launcherQuery]);

  const openHexEditor = useCallback((hex: LauncherHex) => {
    setLauncherError(null);
    setEditingHexId(hex.id);
    setLauncherQuery("");
    setLauncherDraft({ label: hex.label, appId: hex.appId });
  }, []);

  const closeHexEditor = useCallback(() => {
    setEditingHexId(null);
    setLauncherQuery("");
  }, []);

  const saveHexEditor = useCallback(() => {
    if (!editingHexId) return;
    const linkedApp = appById.get(launcherDraft.appId);
    setLauncherHexes(prev => prev.map(hex => hex.id === editingHexId ? {
      ...hex,
      label: launcherDraft.label.trim() || linkedApp?.name || hex.label,
      appId: launcherDraft.appId,
    } : hex));
    closeHexEditor();
  }, [appById, closeHexEditor, editingHexId, launcherDraft]);

  return (
    <div className={`relative w-screen h-screen overflow-hidden ${phaseClass} ${SAFE_HUD ? "safe-hud" : ""}`} style={{ background: "#020617" }}>

      <style>{`
        @keyframes spin{to{transform:rotate(360deg)}}
        @keyframes spin-rev{to{transform:rotate(-360deg)}}
        @keyframes idle-p{0%,100%{transform:scale(1)}50%{transform:scale(1.005)}}
        @keyframes listen-p{0%,100%{transform:scale(0.95)}50%{transform:scale(1.06)}}
        @keyframes proc-p{0%,100%{transform:scale(0.97)}50%{transform:scale(1.04)}}
        @keyframes speak-p{0%,100%{transform:scale(0.97)}50%{transform:scale(1.05)}}
        @keyframes c-idle{0%,100%{opacity:.03}50%{opacity:.09}}
        @keyframes c-listen{0%,100%{opacity:.12}50%{opacity:.4}}
        @keyframes c-proc{0%,100%{opacity:.10}50%{opacity:.3}}
        @keyframes c-speak{0%,100%{opacity:.08}50%{opacity:.35}}
        @keyframes boot{from{transform:scale(0) rotateZ(-90deg);opacity:0}to{transform:scale(1) rotateZ(0deg);opacity:1}}
        @keyframes scanmove{from{top:-2px}to{top:100%}}
        @keyframes clap-flash{0%{opacity:0.7}100%{opacity:0}}
        @keyframes panel-in{from{opacity:0;transform:translateX(var(--dir,10px))}to{opacity:1;transform:translateX(0)}}
        @keyframes orbit-pulse{0%,100%{opacity:.92}50%{opacity:1}}
        @keyframes ring-breathe{0%,100%{transform:scale(0.985);opacity:.55}50%{transform:scale(1.015);opacity:.95}}
        @keyframes glyph-flicker{0%,100%{opacity:.84}45%{opacity:.55}46%{opacity:1}47%{opacity:.68}70%{opacity:.92}}
        @keyframes amber-sweep{0%{transform:translateX(-130%) skewX(-22deg);opacity:0}20%{opacity:.2}50%{opacity:.5}100%{transform:translateX(230%) skewX(-22deg);opacity:0}}
        .hud-boot{animation:boot 1.2s cubic-bezier(0.34,1.56,0.64,1) forwards}
        .contain-layout{contain:layout style}
        .will-transform{will-change:transform,opacity}
        .r1{animation:spin 55s linear infinite}.r2{animation:spin-rev 40s linear infinite}
        .r3{animation:spin 46s linear infinite}.r4{animation:spin-rev 34s linear infinite}
        .r5{animation:spin 28s linear infinite}.r6{animation:spin-rev 120s linear infinite}
        .ring-breathe{animation:ring-breathe 5s ease-in-out infinite}
        .ph-idle .pulse{animation:idle-p 6s ease-in-out infinite}
        .ph-listen .pulse{animation:listen-p 0.8s ease-in-out infinite}
        .ph-process .pulse{animation:proc-p 0.35s ease-in-out infinite}
        .ph-speak .pulse{animation:speak-p 0.6s ease-in-out infinite}
        .ph-idle .core-g{animation:c-idle 6s ease-in-out infinite}
        .ph-listen .core-g{animation:c-listen 0.8s ease-in-out infinite}
        .ph-process .core-g{animation:c-proc 0.35s ease-in-out infinite}
        .ph-speak .core-g{animation:c-speak 0.6s ease-in-out infinite}
        .scan-line{position:absolute;left:0;right:0;height:2px;pointer-events:none;background:linear-gradient(to right,transparent 5%,rgba(34,211,238,0.04) 30%,rgba(34,211,238,0.04) 70%,transparent 95%);animation:scanmove 18s linear infinite}
        .clap-flash{animation:clap-flash 2.5s ease-out forwards}
        .orbit-ring{animation:spin 300s linear infinite}
        .orbit-counter-rotate{animation:spin-rev 300s linear infinite}
        .orbit-anchor,.orbit-radius-anchor{transform-origin:center center}
        .orbit-badge-shell{position:relative; left:0; top:0; animation:orbit-pulse 4.8s ease-in-out infinite}
        .lpanel{--dir:-30px;animation:panel-in 0.8s ease-out both}
        .rpanel{--dir:30px;animation:panel-in 0.8s ease-out 0.15s both}
        .jarvis-panel{background:linear-gradient(180deg, rgba(18,26,46,0.88), rgba(7,14,28,0.74)); border:1px solid rgba(249,115,22,0.2); box-shadow: inset 0 0 0 1px rgba(34,211,238,0.06), inset 0 0 30px rgba(249,115,22,0.05), 0 0 34px rgba(34,211,238,0.03), 0 0 50px rgba(249,115,22,0.04); backdrop-filter:none; clip-path: polygon(0 14px, 14px 0, calc(100% - 14px) 0, 100% 14px, 100% calc(100% - 14px), calc(100% - 14px) 100%, 14px 100%, 0 calc(100% - 14px));}
        .jarvis-panel:before{content:""; position:absolute; inset:10px; border:1px solid rgba(249,115,22,0.09); pointer-events:none; clip-path: polygon(0 10px, 10px 0, calc(100% - 10px) 0, 100% 10px, 100% calc(100% - 10px), calc(100% - 10px) 100%, 10px 100%, 0 calc(100% - 10px));}
        .jarvis-panel:after{content:""; position:absolute; left:24px; right:24px; top:0; height:1px; background:linear-gradient(90deg, transparent, rgba(249,115,22,0.48), rgba(34,211,238,0.3), transparent); pointer-events:none}
        .hud-grid{background-image:linear-gradient(rgba(34,211,238,0.03) 1px, transparent 1px), linear-gradient(90deg, rgba(34,211,238,0.03) 1px, transparent 1px); background-size: 120px 120px; mask-image: radial-gradient(circle at center, rgba(0,0,0,1), transparent 88%);}
        .hero-card{background:linear-gradient(180deg, rgba(22,26,44,0.82), rgba(8,12,24,0.58)); border:1px solid rgba(249,115,22,0.14); box-shadow: inset 0 0 0 1px rgba(34,211,238,0.05), inset 0 0 18px rgba(249,115,22,0.04); clip-path: polygon(0 10px, 10px 0, calc(100% - 10px) 0, 100% 10px, 100% 100%, 0 100%)}
        .hero-card:after{content:""; position:absolute; left:0; right:0; top:0; height:1px; background:linear-gradient(90deg, transparent, rgba(34,211,238,0.35), transparent)}
        .hero-sweep{position:absolute; inset:-20% -40%; background:linear-gradient(100deg, transparent 35%, rgba(245,158,11,0.09) 50%, transparent 65%); animation:amber-sweep 9s linear infinite; pointer-events:none}
        .hero-corner{position:absolute; width:16px; height:16px; border-color:rgba(245,158,11,0.45); pointer-events:none}
        .hero-corner.tl{left:6px; top:6px; border-left:1px solid; border-top:1px solid}
        .hero-corner.br{right:6px; bottom:6px; border-right:1px solid; border-bottom:1px solid}
        .hud-link{position:absolute; height:1px; background:linear-gradient(90deg, rgba(249,115,22,0.0), rgba(249,115,22,0.45), rgba(34,211,238,0.35), rgba(249,115,22,0.0)); opacity:.7; pointer-events:none}
        .hud-bracket{position:absolute; width:24px; height:24px; border-color: rgba(34,211,238,0.24); pointer-events:none}
        .hud-bracket.tl{border-left:1px solid; border-top:1px solid}
        .hud-bracket.tr{border-right:1px solid; border-top:1px solid}
        .hud-bracket.bl{border-left:1px solid; border-bottom:1px solid}
        .hud-bracket.br{border-right:1px solid; border-bottom:1px solid}
        .glyph-flicker{animation:glyph-flicker 3.4s linear infinite}
        .orbit-badge-shell:before{content:""; position:absolute; left:8px; right:8px; top:14px; height:1px; background:linear-gradient(90deg, transparent, rgba(249,115,22,0.34), transparent); opacity:.7}
        .safe-hud .jarvis-panel{backdrop-filter:none; box-shadow: inset 0 0 0 1px rgba(34,211,238,0.04), 0 0 18px rgba(34,211,238,0.025)}
        .safe-hud .pulse,.safe-hud .core-g,.safe-hud .r3,.safe-hud .r4,.safe-hud .r5,.safe-hud .r6,.safe-hud .r-tick,.safe-hud .orbit-ring,.safe-hud .orbit-counter-rotate,.safe-hud .orb-float{animation:none !important}
        .safe-hud .scan-line{opacity:.35; animation-duration:24s}
        .launcher-cluster{position:absolute; left:620px; top:218px; width:300px; height:312px}
        .launcher-hex{clip-path:polygon(25% 0%,75% 0%,100% 50%,75% 100%,25% 100%,0 50%)}
        .launcher-hex-shell{position:absolute; width:120px; height:104px}
        .launcher-hex-button{width:120px; height:104px; transform:translateZ(0); transition:border-color .16s ease, background-color .16s ease, opacity .16s ease, transform .16s ease}
        .launcher-hex-button:hover{opacity:1; transform:translateY(-2px)}
        .launcher-icon-wrap{display:flex; align-items:center; justify-content:center; width:38px; height:38px; border-radius:12px; background:rgba(15,23,42,0.68); border:1px solid rgba(255,255,255,0.06)}
        .launcher-icon{width:28px; height:28px; object-fit:contain; filter:drop-shadow(0 0 10px rgba(34,211,238,0.12))}
        @media (max-width: 1500px){.launcher-cluster{left:580px; top:208px; transform:scale(.92); transform-origin:top left}}
        @media (max-width: 1320px){.launcher-cluster{left:538px; top:196px; transform:scale(.82); transform-origin:top left}}
      `}</style>

      {/* ═══ BACKGROUND ═══ */}
      <HudBackground gd={gd} activated={activated} />

      <div className="absolute left-1/2 top-5 -translate-x-1/2 z-20 w-[1220px] max-w-[76vw] jarvis-panel px-8 py-4">
        <div className="flex items-center justify-between gap-10">
          <div>
            <div className="text-[13px] tracking-[0.45em] text-orange-400/80 font-black glyph-flicker">STARK INDUSTRIES // JARVIS NEURAL COMMAND</div>
            <div className="text-[34px] font-black tracking-[0.3em] leading-none mt-1" style={{ color: rc }}>TACTICAL SYSTEM</div>
            <div className="text-[34px] font-black tracking-[0.3em] leading-none mt-1" style={{ color: rc }}>OVERVIEW</div>
          </div>
          <div className="grid grid-cols-3 gap-4 min-w-[520px]">
            <div className="hero-card px-4 py-3 text-right">
              <div className="text-[10px] tracking-[0.3em] text-zinc-500">TOTAL POWER</div>
              <div className="text-[30px] font-black font-mono leading-none mt-2" style={{ color: rc }}>{totalPower.toFixed(0)}W</div>
              <div className="text-[11px] text-zinc-500 mt-1">CPU {cpuPower.toFixed(0)}W // GPU {gpuPower.toFixed(0)}W</div>
            </div>
            <div className="hero-card px-4 py-3 text-right">
              <div className="text-[10px] tracking-[0.3em] text-zinc-500">TENT CLIMATE</div>
              <div className="text-[30px] font-black font-mono leading-none mt-2" style={{ color: AM }}>{tent ? `${tent.temp.toFixed(1)}°C` : "—"}</div>
              <div className="text-[11px] text-zinc-500 mt-1">HUMI {tent ? `${tent.humi.toFixed(0)}%` : "—"} // VPD {tent ? `${tent.vpd.toFixed(2)}` : "—"}</div>
            </div>
            <div className="hero-card px-4 py-3 text-right">
              <div className="text-[10px] tracking-[0.3em] text-zinc-500">AI CORE</div>
              <div className="text-[30px] font-black font-mono leading-none mt-2" style={{ color: llmOnline ? rc : RD }}>{llmOnline ? "READY" : "OFFLINE"}</div>
              <div className="text-[11px] text-zinc-500 mt-1">WAKE {jarvisEnabled ? "ARMED" : "OFF"} // TTS {ttsEnabled ? "ON" : "OFF"}</div>
            </div>
          </div>
        </div>
      </div>

      <div className="hud-link z-[12]" style={{ left: "445px", top: "50%", width: "170px" }} />
      <div className="hud-link z-[12]" style={{ right: "445px", top: "50%", width: "170px" }} />
      <div className="hud-link z-[12]" style={{ left: "calc(50% - 120px)", top: "110px", width: "240px" }} />

      {/* ═══ TOP CONTROLS ═══ */}
      <button onClick={() => navigate("/")} className="absolute top-4 left-5 z-30 p-2 rounded-lg text-zinc-700 hover:text-cyan-400 transition-colors"><ArrowLeft className="w-5 h-5" /></button>
      <div className="absolute top-4 right-5 z-30 flex items-center gap-2.5">
        <button onClick={toggleJarvisEnabled} className={`p-2 rounded-lg border transition-all ${jarvisEnabled?"border-cyan-500/30 text-cyan-400":"border-zinc-800 text-zinc-700"}`}><Power className="w-4 h-4"/></button>
        <button onClick={()=>setTtsEnabled(!ttsEnabled)} className={`p-2 rounded-lg border transition-all ${ttsEnabled?"border-cyan-500/30 text-cyan-400":"border-zinc-800 text-zinc-700"}`}>{ttsEnabled?<Volume2 className="w-4 h-4"/>:<VolumeX className="w-4 h-4"/>}</button>
        <button onClick={toggleMic} disabled={loading&&!listening} className={`p-2.5 rounded-lg border transition-all ${listening?"border-red-500/50 text-red-400 bg-red-500/10":"border-cyan-500/30 text-cyan-400 hover:bg-cyan-500/10"}`}><Mic className="w-5 h-5"/></button>
      </div>

      {/* ══════════════════════════════════════════════════════════
           LEFT PANEL — PC System (320px wide, fills height)
         ══════════════════════════════════════════════════════════ */}
      <div className="lpanel absolute left-5 top-24 bottom-18 z-20 flex flex-col justify-start overflow-y-auto pointer-events-none scrollbar-none jarvis-panel" style={{ width: 460 }}>
        <div className="relative pr-6 pl-6 pt-6 pb-5">
          <div className="hud-bracket tl left-3 top-3" />
          <div className="hud-bracket tr right-3 top-3" />
          <div className="hud-bracket bl left-3 bottom-3" />
          <div className="hud-bracket br right-3 bottom-3" />

          {/* TIME */}
          <ClockReadout rc={rc} uptime={uptime} />

          <div className="grid grid-cols-2 gap-3 mt-4">
            <HeroCard label="CPU LOAD" value={`${cpuUsage.toFixed(0)}%`} sub={`${cpuCores}C / ${cpuThreads}T`} color={cpuUsage > 85 ? RD : rc} />
            <HeroCard label="GPU CORE" value={`${gpuTemp.toFixed(0)}°C`} sub={`${gpuLoad.toFixed(0)}% LOAD`} color={gpuTemp > 80 ? RD : rc} />
            <HeroCard label="MEMORY" value={`${ramPct.toFixed(0)}%`} sub={`${ramUsed} / ${ramTotal} GB`} color={ramPct > 85 ? RD : rc} />
            <HeroCard label="THERMALS" value={`${cpuTemp.toFixed(0)}° / ${gpuJnc.toFixed(0)}°`} sub="CPU / JUNCTION" color={cpuTemp > 80 || gpuJnc > 95 ? RD : AM} />
          </div>

          {/* CPU */}
          <SL>CPU</SL>
          <Spark data={cpuHist} color={cpuUsage > 80 ? RD : rc} w={GW} h={GH} label="AUSLASTUNG" valNow={`${cpuUsage.toFixed(0)}%`} />
          <div className="flex gap-5 mt-1.5">
            <KV k="TEMP" v={cpuTemp.toFixed(0)} warn={cpuTemp > 80} unit="°C" />
            <KV k="PWR" v={cpuPower.toFixed(0)} unit="W" />
            <KV k="FREQ" v={cpuFreq ? cpuFreq.toFixed(0) : "—"} unit="MHz" />
          </div>
          <div className="text-[10px] text-zinc-600 mt-0.5">{cpuCores}C / {cpuThreads}T</div>

          {/* Per-core */}
          {perCore.length > 0 && (
            <div className="mt-2">
              <div className="text-[9px] tracking-[0.2em] text-zinc-600 mb-1">PER CORE USAGE</div>
              <div className="flex gap-[2px] items-end" style={{ height: 32 }}>
                {perCore.map((v, i) => (
                  <div key={i} style={{ width: Math.max(4, GW / perCore.length - 2), height: `${Math.max(3, v)}%`, backgroundColor: v > 80 ? RD : rc, opacity: 0.6, borderRadius: 2 }} />
                ))}
              </div>
            </div>
          )}

          {/* RAM */}
          <SL>SPEICHER</SL>
          <Spark data={ramHist} color={ramPct > 85 ? RD : rc} w={GW} h={GHS} label="RAM" valNow={`${ramUsed} / ${ramTotal} GB`} />
          <HBar pct={ramPct} color={ramPct > 85 ? RD : rc} w={GW} h={7} />
          <div className="flex gap-5 mt-1">
            <KV k="SWAP" v={`${swapPct.toFixed(0)}%`} warn={swapPct > 50} />
            <KV k="LOAD" v={`${load1.toFixed(1)} / ${load5.toFixed(1)} / ${load15.toFixed(1)}`} />
          </div>

          {/* GPU */}
          <SL>GPU — RX 7900 XTX</SL>
          <Spark data={gpuTempHist} color={gpuTemp > 80 ? RD : rc} w={GW} h={GH} label="TEMPERATUR" valNow={`${gpuTemp.toFixed(0)}°C`} />
          <div className="flex gap-4 mt-1">
            <span className="text-[11px] text-zinc-500">JNC <span className="font-bold font-mono text-[13px]" style={{ color: gpuJnc > 95 ? RD : rc }}>{gpuJnc.toFixed(0)}°</span></span>
            <span className="text-[11px] text-zinc-500">MEM <span className="font-bold font-mono text-[13px]" style={{ color: rc }}>{gpuMem.toFixed(0)}°</span></span>
            <span className="text-[11px] text-zinc-500">FAN <span className="font-bold font-mono text-[13px]" style={{ color: rc }}>{gpuFanRpm}</span></span>
          </div>

          <Spark data={gpuLoadHist} color={gpuLoad > 95 ? RD : rc} w={GW} h={GHS} label="GPU LOAD" valNow={`${gpuLoad}%`} />

          <Spark
            data={gpuVramHist}
            color={gpuVramUsed > gpuVramTotal * 0.9 ? RD : AM}
            w={GW}
            h={GHS}
            label="VRAM USED"
            valNow={`${Math.round(gpuVramUsed).toLocaleString()} MB`}
            subLabel={`${(gpuVramUsed / 1024).toFixed(1)} / ${(gpuVramTotal / 1024).toFixed(1)} GB`}
            maxValue={gpuVramTotal}
          />
          <HBar pct={(gpuVramUsed / Math.max(gpuVramTotal, 1)) * 100} color={gpuVramUsed > gpuVramTotal * 0.9 ? RD : AM} w={GW} h={7} />

          <Spark data={gpuPwrHist} color={rc} w={GW} h={GHS} label="POWER DRAW" valNow={`${gpuPower.toFixed(0)} / ${gpuPowerCap.toFixed(0)}W`} />
          <HBar pct={(gpuPower / gpuPowerCap) * 100} color={rc} w={GW} h={7} />

          <div className="flex gap-5 mt-1.5">
            <KV k="SCLK" v={gpuClock} unit="MHz" />
            <KV k="MCLK" v={gpuMclk} unit="MHz" />
            <KV k="VRAM" v={`${(gpuVramUsed / 1024).toFixed(1)}`} unit={`/ ${(gpuVramTotal / 1024).toFixed(0)} GB`} color={gpuVramUsed > gpuVramTotal * 0.9 ? RD : AM} />
          </div>

          {/* NVMe + storage */}
          <SL>STORAGE</SL>
          {(sysmon?.nvme_temps ?? []).map((nv, i) => (
            <KV key={i} k={nv.name.replace("nvme","NVMe ").toUpperCase()} v={nv.temp_celsius.toFixed(0)} warn={nv.temp_celsius > 65} unit="°C" />
          ))}
        </div>
      </div>

      {/* ══════════════════════════════════════════════════════════
           RIGHT PANEL — Tent/Pi4 + AI + Devices (320px wide)
         ══════════════════════════════════════════════════════════ */}
      <div className="rpanel absolute right-5 top-24 bottom-18 z-20 flex flex-col justify-start overflow-y-auto pointer-events-none scrollbar-none jarvis-panel" style={{ width: 460 }}>
        <div className="relative pl-6 pr-6 pt-6 pb-5">
          <div className="hud-bracket tl left-3 top-3" />
          <div className="hud-bracket tr right-3 top-3" />
          <div className="hud-bracket bl left-3 bottom-3" />
          <div className="hud-bracket br right-3 bottom-3" />

          <div className="grid grid-cols-2 gap-3 mb-4">
            <HeroCard label="TENT TEMP" value={tent ? `${tent.temp.toFixed(1)}°C` : "—"} sub="GROW ENVIRONMENT" color={tent && tent.temp > 35 ? RD : AM} />
            <HeroCard label="HUMIDITY" value={tent ? `${tent.humi.toFixed(0)}%` : "—"} sub={`VPD ${tent ? `${tent.vpd.toFixed(2)} kPa` : "—"}`} color={CY2} />
            <HeroCard label="LIGHT LEVEL" value={tentLight ? `${tentLight.brightness}%` : "—"} sub={tentLight?.power ? "MARSHYDRO ONLINE" : "MARSHYDRO STANDBY"} color={AM} />
            <HeroCard label="WATER TANK" value={tentTank ? `${tentTank.percent.toFixed(0)}%` : "—"} sub={tentTank ? `${tentTank.liters.toFixed(1)} L AVAILABLE` : "—"} color={tentTank && tentTank.percent < 20 ? RD : GR} />
          </div>

          {/* TENT CLIMATE */}
          <SL>ZELT — KLIMA</SL>
          <div className="flex items-center gap-2 mb-1">
            <Thermometer className="w-4 h-4" style={{ color: AM }} />
            <span className="text-[11px] tracking-wider text-zinc-500">TEMPERATUR</span>
            <span className="text-[18px] font-black font-mono ml-auto" style={{ color: tent && tent.temp > 35 ? RD : AM }}>{tent ? tent.temp.toFixed(1) : "—"}°C</span>
          </div>
          <Spark data={tentTempHist} color={tent && tent.temp > 35 ? RD : AM} w={GW} h={GH} label="" valNow="" />

          <div className="flex items-center gap-2 mt-3 mb-1">
            <Droplets className="w-4 h-4" style={{ color: CY2 }} />
            <span className="text-[11px] tracking-wider text-zinc-500">LUFTFEUCHTIGKEIT</span>
            <span className="text-[18px] font-black font-mono ml-auto" style={{ color: CY2 }}>{tent ? tent.humi.toFixed(0) : "—"}%</span>
          </div>
          <Spark data={tentHumiHist} color={CY2} w={GW} h={GH} label="" valNow="" />

          <div className="flex gap-5 mt-2">
            <KV k="VPD" v={tent ? tent.vpd.toFixed(2) : "—"} unit="kPa" />
            <KV k="BATTERIE" v={tent ? `${tent.batt}` : "—"} unit="%" />
          </div>
          <Spark data={tentVpdHist} color={GR} w={GW} h={GHS} label="VPD" valNow={tent ? `${tent.vpd.toFixed(2)} kPa` : "—"} />

          {/* TENT LIGHT */}
          <SL>MARSHYDRO LICHT</SL>
          <div className="flex items-center gap-2 mb-1">
            <Sun className="w-4 h-4" style={{ color: AM }} />
            <span className="text-[11px] tracking-wider text-zinc-500">HELLIGKEIT</span>
            <span className="text-[18px] font-black font-mono ml-auto" style={{ color: AM }}>{tentLight ? tentLight.brightness : "—"}%</span>
          </div>
          <Spark data={tentBrightHist} color={AM} w={GW} h={GHS} label="" valNow="" />
          <HBar pct={tentLight?.brightness ?? 0} color={AM} w={GW} h={7} />
          <div className="flex gap-5 mt-1">
            <KV k="POWER" v={tentLight ? (tentLight.power ? "ON" : "OFF") : "—"} warn={tentLight ? !tentLight.power : false} />
          </div>

          {/* TANK */}
          <SL>WASSERTANK</SL>
          <div className="flex items-center gap-2 mb-1">
            <Gauge className="w-4 h-4" style={{ color: GR }} />
            <span className="text-[11px] tracking-wider text-zinc-500">FÜLLSTAND</span>
            <span className="text-[18px] font-black font-mono ml-auto" style={{ color: tentTank && tentTank.percent < 20 ? RD : GR }}>{tentTank ? tentTank.percent.toFixed(0) : "—"}%</span>
          </div>
          <HBar pct={tentTank?.percent ?? 0} color={tentTank && tentTank.percent < 20 ? RD : GR} w={GW} h={10} />
          <KV k="VOLUMEN" v={tentTank ? tentTank.liters.toFixed(1) : "—"} unit="L" />
          {tentError && <div className="mt-2 text-[10px] text-red-400/80 tracking-[0.08em]">PI4: {tentError}</div>}

          {/* AI MODELS */}
          <SL>AI SYSTEME</SL>
          <KV k="STT" v="WHISPER SMALL" />
          <KV k="LLM" v="QWEN 3.5 9B" />
          <KV k="TTS" v="ORPHEUS 3B" />
          <KV k="TOOLS" v="17 AKTIV" />

          {/* AUDIO TELEMETRY */}
          <SL>AUDIO TELEMETRIE</SL>
          <div className="flex items-center gap-2 mb-1">
            <Activity className="w-4 h-4" style={{ color: AM }} />
            <span className="text-[11px] tracking-wider text-zinc-500">MIC SIGNAL</span>
            <div className="flex items-center gap-1.5 ml-auto">
              <div className={`w-2 h-2 rounded-full ${audioTelem ? (audioTelem.state === "muted" ? "bg-red-500" : audioTelem.state === "idle" ? "bg-emerald-400 shadow-[0_0_6px_rgba(34,197,94,0.6)]" : "bg-amber-400 shadow-[0_0_6px_rgba(245,158,11,0.6)]") : "bg-zinc-700"}`} />
              <span className={`text-[11px] font-bold tracking-wider font-mono ${audioTelem ? (audioTelem.state === "muted" ? "text-red-400" : audioTelem.state === "idle" ? "text-emerald-400" : "text-amber-400") : "text-zinc-600"}`}>
                {audioTelem ? audioTelem.state.toUpperCase() : "OFFLINE"}
              </span>
            </div>
          </div>
          <Spark data={rmsHist} color={AM} w={GW} h={GH} label="RMS" valNow={audioTelem ? `${audioTelem.rms}` : "—"} />
          <div className="flex gap-5 mt-1">
            <KV k="RMS" v={audioTelem ? `${audioTelem.rms}` : "—"} color={audioTelem && audioTelem.rms > 1200 ? RD : audioTelem && audioTelem.rms > 500 ? AM : undefined} />
            <KV k="PEAK" v={audioTelem ? `${audioTelem.peak}` : "—"} color={audioTelem && audioTelem.peak > 15000 ? RD : undefined} />
          </div>
          <div className="flex items-center gap-2 mt-2 mb-1">
            <Mic className="w-3.5 h-3.5" style={{ color: (audioTelem?.wake ?? 0) > WAKE_THRESHOLD ? RD : CY }} />
            <span className="text-[11px] tracking-wider text-zinc-500">WAKE CONFIDENCE</span>
            <span className="text-[14px] font-black font-mono ml-auto" style={{ color: (audioTelem?.wake ?? 0) > WAKE_THRESHOLD ? RD : (audioTelem?.wake ?? 0) > 0.2 ? AM : CY }}>
              {audioTelem ? (audioTelem.wake * 100).toFixed(1) : "—"}<span className="text-[10px] text-zinc-600">%</span>
            </span>
          </div>
          <HBar pct={Math.min((audioTelem?.wake ?? 0) * 100 / WAKE_THRESHOLD, 100)} color={(audioTelem?.wake ?? 0) > WAKE_THRESHOLD ? RD : (audioTelem?.wake ?? 0) > 0.2 ? AM : GR} w={GW} h={7} />
          <Spark data={wakeHist} color={(audioTelem?.wake ?? 0) > 0.2 ? AM : CY} w={GW} h={GHS} label="WAKE" valNow={audioTelem ? `${(audioTelem.wake * 100).toFixed(1)}%` : "—"} />
          <div className="flex gap-5 mt-1">
            <KV k="THRESHOLD" v={`${(WAKE_THRESHOLD * 100).toFixed(0)}%`} />
            <KV k="SPEECH" v="200" unit="rms" />
            <KV k="SILENCE" v="100" unit="rms" />
          </div>

          {/* STATUS */}
          <SL>STATUS</SL>
          <div className="flex items-center gap-4">
            {[
              { l: "LLM", on: llmOnline === true, off: llmOnline === false },
              { l: "WAKE", on: jarvisEnabled },
              { l: "TTS", on: ttsEnabled },
            ].map(s => (
              <div key={s.l} className="flex items-center gap-1.5">
                <div className={`w-2 h-2 rounded-full ${s.on ? "bg-emerald-400 shadow-[0_0_6px_rgba(34,197,94,0.6)]" : s.off ? "bg-red-500" : "bg-zinc-700"}`} />
                <span className={`text-[11px] font-bold tracking-wider ${s.on ? "text-emerald-400" : s.off ? "text-red-400" : "text-zinc-600"}`}>{s.l}</span>
              </div>
            ))}
          </div>

          {/* DEVICES */}
          <SL>GERÄTE</SL>
          {["CORSAIR K70 / M75 AIR", "GOVEE SMART HOME", "BEWÄSSERUNG × 6 RELAIS", "NFS MOUNT PI4", "MARSHYDRO TS 1000", "COMFEE ENTFEUCHTER"].map(d => (
            <div key={d} className="flex items-center gap-2 py-[1px]">
              <div className="w-1.5 h-1.5 rounded-full bg-cyan-500/60" />
              <span className="text-[11px] font-semibold tracking-wider text-zinc-500">{d}</span>
            </div>
          ))}

          <SL>NETZWERK</SL>
          {["GATEWAY :3100", "NGROK TUNNEL", "PI4 SSH :22", "PI5 SSH :22"].map(d => (
            <div key={d} className="flex items-center gap-2 py-[1px]">
              <div className="w-1.5 h-1.5 rounded-full bg-cyan-500/40" />
              <span className="text-[11px] font-semibold tracking-wider text-zinc-500">{d}</span>
            </div>
          ))}
        </div>
      </div>

      {/* ═══ ARC REACTOR — CENTERED ═══ */}
      <ReactorCore rc={rc} rc2={rc2} gd={gd} phaseLabel={phaseLabel} phaseAccent={phase === "listening" ? RD : AM} ticks={ticks} majorTicks={majorTicks} />
      <AudioCanvas telemRef={audioTelemRef} phaseRef={phaseStrRef} activatedAtRef={activatedAtRef} />
      <OrbitBadges orbitData={orbitData} rc={rc} gd={gd} />

      <div className="absolute inset-0 z-[14] pointer-events-none">
        <div className="launcher-cluster">
          {launcherHexes.map(hex => {
            const linkedApp = appById.get(hex.appId);
            return (
              <div
                key={hex.id}
                className="launcher-hex-shell pointer-events-auto"
                style={{ left: hex.x, top: hex.y }}
              >
                <div
                  role="button"
                  tabIndex={0}
                  onClick={() => { void launchHex(hex); }}
                  onKeyDown={event => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      void launchHex(hex);
                    }
                  }}
                  className="launcher-hex launcher-hex-button group relative flex flex-col items-center justify-center overflow-hidden border"
                  style={{
                    background: "rgba(5, 16, 34, 0.76)",
                    borderColor: `${hex.accent}3d`,
                    boxShadow: "none",
                    opacity: 0.96,
                  }}
                >
                  <div className="absolute inset-[1px] launcher-hex" style={{ background: "rgba(2, 10, 24, 0.86)" }} />
                  <div className="absolute inset-x-4 top-2 h-px" style={{ background: `linear-gradient(90deg, transparent, ${hex.accent}70, transparent)` }} />
                  <button
                    type="button"
                    data-launcher-edit="true"
                    onClick={event => {
                      event.stopPropagation();
                      openHexEditor(hex);
                    }}
                    className="absolute right-2 top-2 z-10 rounded border px-1 py-0.5 text-[8px] font-black tracking-[0.16em] text-zinc-300 transition hover:text-white"
                    style={{ borderColor: `${hex.accent}35`, background: "rgba(2,6,23,0.68)" }}
                  >
                    E
                  </button>
                  <div className="relative z-[1] flex flex-col items-center px-2 text-center">
                    <div className="launcher-icon-wrap">
                      {desktopIconSrc(linkedApp) ? (
                        <img
                          src={desktopIconSrc(linkedApp) ?? undefined}
                          alt={linkedApp?.name ?? hex.label}
                          className="launcher-icon"
                          loading="lazy"
                        />
                      ) : (
                        <span className="text-[16px] font-black uppercase" style={{ color: hex.accent }}>
                          {hex.label.slice(0, 1)}
                        </span>
                      )}
                    </div>
                    <div className="mt-2 text-[13px] font-black leading-tight text-zinc-100">{hex.label}</div>
                    <div className="mt-1 text-[8px] uppercase tracking-[0.16em] text-zinc-500">
                      {linkedApp?.name ?? "Programm wählen"}
                    </div>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {launcherError && (
        <div className="fixed bottom-[68px] left-1/2 z-50 -translate-x-1/2 rounded-xl border px-4 py-2 text-[11px] font-medium text-red-200"
          style={{ background: "rgba(127,29,29,0.82)", borderColor: "rgba(248,113,113,0.35)", boxShadow: "0 0 18px rgba(248,113,113,0.18)" }}>
          {launcherError}
        </div>
      )}

      {editingHex && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-slate-950/72 backdrop-blur-sm">
          <div className="w-[720px] max-w-[92vw] rounded-2xl border border-cyan-400/20 bg-slate-950/92 p-6 shadow-[0_0_50px_rgba(34,211,238,0.12)]">
            <div className="flex items-start justify-between gap-6">
              <div>
                <div className="text-[11px] font-black uppercase tracking-[0.3em] text-cyan-300">Launcher Hex</div>
                <h2 className="mt-2 text-2xl font-black text-zinc-100">Programm verknüpfen</h2>
                <p className="mt-2 text-sm text-zinc-500">Name ändern, App auswählen und das Hexagon danach frei verschieben.</p>
              </div>
              <button type="button" onClick={closeHexEditor} className="rounded-lg border border-zinc-800 px-3 py-2 text-xs font-bold uppercase tracking-[0.2em] text-zinc-400 transition hover:border-zinc-700 hover:text-zinc-200">Schließen</button>
            </div>

            <div className="mt-6 grid gap-4 md:grid-cols-[220px,1fr]">
              <div className="space-y-3">
                <label className="block text-[11px] font-black uppercase tracking-[0.22em] text-zinc-500">
                  Titel
                  <input
                    type="text"
                    value={launcherDraft.label}
                    onChange={event => setLauncherDraft(prev => ({ ...prev, label: event.target.value }))}
                    className="mt-2 w-full rounded-xl border border-zinc-800 bg-slate-900 px-3 py-2 text-sm text-zinc-200 outline-none transition focus:border-cyan-400/40"
                    placeholder="Hexagon Name"
                  />
                </label>
                <label className="block text-[11px] font-black uppercase tracking-[0.22em] text-zinc-500">
                  Suche
                  <input
                    type="text"
                    value={launcherQuery}
                    onChange={event => setLauncherQuery(event.target.value)}
                    className="mt-2 w-full rounded-xl border border-zinc-800 bg-slate-900 px-3 py-2 text-sm text-zinc-200 outline-none transition focus:border-cyan-400/40"
                    placeholder="Firefox, Steam, Code ..."
                  />
                </label>
                <div className="rounded-xl border border-zinc-900 bg-slate-900/70 px-3 py-3 text-xs text-zinc-500">
                  Aktuell: <span className="font-semibold text-zinc-300">{appById.get(launcherDraft.appId)?.name ?? "Kein Programm"}</span>
                </div>
              </div>

              <div className="rounded-2xl border border-zinc-900 bg-slate-900/70 p-3">
                <div className="max-h-[360px] overflow-y-auto pr-1">
                  <div className="grid gap-2">
                    {filteredDesktopApps.map(app => {
                      const selected = launcherDraft.appId === app.id;
                      return (
                        <button
                          key={app.id}
                          type="button"
                          onClick={() => setLauncherDraft(prev => ({ ...prev, appId: app.id, label: prev.label || app.name }))}
                          className="rounded-xl border px-4 py-3 text-left transition"
                          style={{
                            borderColor: selected ? "rgba(34,211,238,0.45)" : "rgba(39,39,42,0.9)",
                            background: selected ? "rgba(8,47,73,0.55)" : "rgba(15,23,42,0.6)",
                            boxShadow: selected ? "0 0 18px rgba(34,211,238,0.12)" : "none",
                          }}
                        >
                          <div className="text-sm font-semibold text-zinc-100">{app.name}</div>
                          <div className="mt-1 text-xs font-mono text-zinc-500">{app.id}</div>
                          <div className="mt-1 text-[11px] text-zinc-600">{app.exec}</div>
                        </button>
                      );
                    })}
                    {filteredDesktopApps.length === 0 && (
                      <div className="rounded-xl border border-dashed border-zinc-800 px-4 py-6 text-center text-sm text-zinc-500">
                        Keine passende Anwendung gefunden.
                      </div>
                    )}
                  </div>
                </div>
              </div>
            </div>

            <div className="mt-6 flex justify-end gap-3">
              <button type="button" onClick={closeHexEditor} className="rounded-xl border border-zinc-800 px-4 py-2 text-sm font-semibold text-zinc-300 transition hover:border-zinc-700 hover:text-zinc-100">Abbrechen</button>
              <button type="button" onClick={saveHexEditor} className="rounded-xl border border-cyan-400/40 bg-cyan-400/10 px-4 py-2 text-sm font-semibold text-cyan-200 transition hover:bg-cyan-400/15">Speichern</button>
            </div>
          </div>
        </div>
      )}

      {/* ═══ CHAT + INPUT — pinned to bottom, chat grows upward ═══ */}

      {/* ── Input Bar — ALWAYS fixed at very bottom ── */}
      <div className="fixed bottom-0 left-1/2 -translate-x-1/2 w-full max-w-[800px] z-40 pointer-events-none">
        <div className="pointer-events-auto px-4 pb-3 pt-2">
          <div
            className="flex items-center gap-3 px-4 py-2.5 rounded-xl transition-all duration-300"
            style={{
              border: `1px solid ${rc}18`,
              backgroundColor: "rgba(2,6,23,0.94)",
              boxShadow: `0 0 20px ${rc}06, inset 0 0 20px ${rc}03`,
              backdropFilter: SAFE_HUD ? "none" : "blur(8px)",
            }}
          >
            <div className="w-1 h-4 rounded-full shrink-0" style={{ backgroundColor: `${rc}40` }} />
            <input
              ref={inputRef}
              type="text"
              value={input}
              onChange={e => setInput(e.target.value)}
              onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); sendMessage(input); } }}
              placeholder="Befehl eingeben..."
              className="flex-1 bg-transparent border-none outline-none text-sm text-zinc-300 placeholder:text-zinc-700 font-mono"
              disabled={loading}
            />
            <button
              onClick={() => sendMessage(input)}
              disabled={!input.trim() || loading}
              className="p-2 rounded-lg transition-all duration-200"
              style={{
                backgroundColor: input.trim() ? `${rc}18` : "transparent",
                boxShadow: input.trim() ? `0 0 10px ${rc}15` : "none",
              }}
            >
              <Send className="w-4 h-4" style={{ color: input.trim() ? rc : "#27272a" }} />
            </button>
          </div>
        </div>
      </div>

      {/* ── Chat panel — grows upward from above the input bar ── */}
      <div
        className="fixed left-1/2 -translate-x-1/2 w-full max-w-[800px] pointer-events-none transition-all duration-400 ease-in-out"
        style={{ bottom: 76, zIndex: 35 }}
      >
        {/* Toggle */}
        {messages.length > 0 && (
          <div className="flex justify-center">
            <button
              onClick={() => setChatExpanded(!chatExpanded)}
              className="pointer-events-auto px-5 py-1 rounded-t-lg text-[10px] font-black tracking-[0.25em] uppercase transition-all duration-300 hover:brightness-125"
              style={{
                backgroundColor: `${rc}0c`,
                color: rc,
                border: `1px solid ${rc}25`,
                borderBottom: "none",
                boxShadow: chatExpanded ? `0 -4px 20px ${rc}10` : "none",
                textShadow: `0 0 8px ${rc}40`,
              }}
            >
              <ChevronUp className={`w-3 h-3 inline-block mr-1.5 transition-transform duration-300 ${chatExpanded ? "" : "rotate-180"}`} />
              {messages.length} NACHRICHTEN
            </button>
          </div>
        )}

        {/* Messages container */}
        <div
          className="pointer-events-auto overflow-hidden transition-all duration-400 ease-in-out"
          style={{
            maxHeight: chatExpanded ? 340 : 0,
            opacity: chatExpanded ? 1 : 0,
          }}
        >
          <div
            className="overflow-y-auto px-6 py-3 scrollbar-thin scrollbar-thumb-zinc-800 flex flex-col-reverse"
            style={{
              maxHeight: 340,
              background: `linear-gradient(180deg, ${rc}04 0%, rgba(2,6,23,0.97) 8%, rgba(2,6,23,0.98) 100%)`,
              borderTop: `1px solid ${rc}20`,
              borderLeft: `1px solid ${rc}10`,
              borderRight: `1px solid ${rc}10`,
              borderBottom: `1px solid ${rc}10`,
              boxShadow: `inset 0 1px 30px ${rc}06, 0 -8px 30px rgba(0,0,0,0.5)`,
            }}
          >
            <div className="space-y-2.5">
              {messages.map((msg, i) => (
                <div key={i} className={`flex ${msg.role === "user" ? "justify-end" : "justify-start"}`}>
                  <div
                    className="max-w-[75%] rounded-lg px-3.5 py-2 flex gap-2.5 backdrop-blur-sm"
                    style={{
                      backgroundColor: msg.role === "user" ? "rgba(96,165,250,0.06)" : `${rc}08`,
                      border: `1px solid ${msg.role === "user" ? "rgba(96,165,250,0.12)" : `${rc}12`}`,
                      boxShadow: msg.role === "user" ? "0 0 12px rgba(96,165,250,0.04)" : `0 0 12px ${rc}04`,
                    }}
                  >
                    <div className="w-0.5 self-stretch rounded-full shrink-0" style={{ backgroundColor: msg.role === "user" ? "#60a5fa" : rc, boxShadow: `0 0 6px ${msg.role === "user" ? "#60a5fa40" : `${rc}40`}` }} />
                    <div className="min-w-0">
                      <span className="text-[9px] font-black tracking-[0.2em]" style={{ color: msg.role === "user" ? "#60a5fa80" : `${rc}80` }}>{msg.role === "user" ? "DU" : "JARVIS"}</span>
                      <p className="text-[13px] text-zinc-400 whitespace-pre-wrap leading-relaxed mt-0.5">{msg.content}</p>
                      {msg.actions?.map((a, j) => (
                        <div key={j} className="flex items-center gap-1.5 text-[10px] mt-1.5">
                          <div className="w-1 h-1 rounded-full" style={{ backgroundColor: a.success ? "#34d399" : "#f87171", boxShadow: `0 0 4px ${a.success ? "#34d39960" : "#f8717160"}` }} />
                          <Zap className={`w-2.5 h-2.5 ${a.success ? "text-emerald-400" : "text-red-400"}`} />
                          <span className="text-zinc-600 font-mono">{a.message}</span>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              ))}
              {loading && (
                <div className="flex justify-start">
                  <div className="rounded-lg px-3.5 py-2 flex gap-2.5" style={{ backgroundColor: `${rc}08`, border: `1px solid ${rc}12` }}>
                    <div className="w-0.5 self-stretch rounded-full" style={{ backgroundColor: rc, boxShadow: `0 0 6px ${rc}40` }} />
                    <div>
                      <span className="text-[9px] font-black tracking-[0.2em]" style={{ color: `${rc}80` }}>JARVIS</span>
                      <div className="flex items-center gap-1.5 mt-1">
                        <div className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ backgroundColor: rc }} />
                        <div className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ backgroundColor: rc, animationDelay: "150ms" }} />
                        <div className="w-1.5 h-1.5 rounded-full animate-pulse" style={{ backgroundColor: rc, animationDelay: "300ms" }} />
                      </div>
                    </div>
                  </div>
                </div>
              )}
              <div ref={chatEndRef} />
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

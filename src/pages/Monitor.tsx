import { useEffect, useState, useRef } from "react";
import { Cpu, Fan, MemoryStick, Thermometer, Zap, Monitor as MonitorIcon } from "lucide-react";
import { api, SystemMonitorData, CcxtStatus } from "../api";
import { Card, Badge, PageHeader, Loading } from "../components/ui";

// ─── Helpers ──────────────────────────────────────────────────

function tempColor(t: number) { return t >= 80 ? "text-red-400" : t >= 65 ? "text-amber-400" : "text-emerald-400"; }
function tempBg(t: number) { return t >= 80 ? "bg-red-500" : t >= 65 ? "bg-amber-500" : "bg-emerald-500"; }
function usageColor(p: number) { return p >= 90 ? "red" : p >= 70 ? "amber" : "cyan"; }

const barColors: Record<string, string> = {
  cyan: "bg-cyan-500", emerald: "bg-emerald-500", amber: "bg-amber-500",
  red: "bg-red-500", purple: "bg-purple-500", blue: "bg-blue-500",
};

function Bar({ value, max = 100, color = "cyan", h = "h-2" }: {
  value: number; max?: number; color?: string; h?: string;
}) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  return (
    <div className={`w-full bg-zinc-800 rounded-full ${h} overflow-hidden`}>
      <div className={`${barColors[color] || "bg-cyan-500"} ${h} rounded-full transition-all duration-500`}
        style={{ width: `${pct}%` }} />
    </div>
  );
}

function RingGauge({ value, max = 100, size = 56, stroke = 5, color = "cyan", children }: {
  value: number; max?: number; size?: number; stroke?: number; color?: string; children?: React.ReactNode;
}) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  const r = (size - stroke) / 2;
  const circ = 2 * Math.PI * r;
  const offset = circ - (pct / 100) * circ;
  const sc: Record<string, string> = {
    cyan: "stroke-cyan-500", emerald: "stroke-emerald-500", amber: "stroke-amber-500",
    red: "stroke-red-500", purple: "stroke-purple-500", blue: "stroke-blue-500",
  };
  return (
    <div className="relative inline-flex items-center justify-center" style={{ width: size, height: size }}>
      <svg width={size} height={size} className="-rotate-90">
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={stroke} className="stroke-zinc-800" />
        <circle cx={size / 2} cy={size / 2} r={r} fill="none" strokeWidth={stroke}
          className={`${sc[color] || "stroke-cyan-500"} transition-all duration-700`}
          strokeDasharray={circ} strokeDashoffset={offset} strokeLinecap="round" />
      </svg>
      <div className="absolute inset-0 flex items-center justify-center">{children}</div>
    </div>
  );
}

function SectionTitle({ icon: Icon, children }: { icon: React.ElementType; children: React.ReactNode }) {
  return (
    <h3 className="text-xs font-bold uppercase tracking-wider text-zinc-500 mb-3 flex items-center gap-2">
      <Icon className="w-3.5 h-3.5" /> {children}
    </h3>
  );
}

function SpecPill({ label, value, highlight = false }: { label: string; value: string; highlight?: boolean }) {
  return (
    <div className={`rounded-lg px-3 py-2 text-center ${highlight ? "bg-amber-500/5 border border-amber-500/20" : "bg-zinc-900/60 border border-zinc-800/50"}`}>
      <div className="text-[10px] text-zinc-500 mb-0.5">{label}</div>
      <div className={`text-xs font-bold font-mono ${highlight ? "text-amber-400" : "text-zinc-200"}`}>{value}</div>
    </div>
  );
}

function ThermalRow({ label, temp, watts }: { label: string; temp: number; watts: number | null }) {
  return (
    <div className="bg-zinc-900/40 rounded-lg p-3">
      <div className="flex items-center justify-between mb-2">
        <span className="text-xs font-semibold text-zinc-400">{label}</span>
        <div className="flex items-center gap-3">
          {watts !== null && (
            <span className="text-xs font-mono text-zinc-400">{watts.toFixed(1)}<span className="text-zinc-600"> W</span></span>
          )}
          <span className={`text-sm font-black tabular-nums ${tempColor(temp)}`}>{temp.toFixed(0)}°C</span>
        </div>
      </div>
      <div className="w-full bg-zinc-800 rounded-full h-1.5 overflow-hidden">
        <div className={`h-1.5 rounded-full transition-all duration-500 ${tempBg(temp)}`}
          style={{ width: `${Math.min((temp / 100) * 100, 100)}%` }} />
      </div>
    </div>
  );
}

function PowerRow({ label, watts }: { label: string; watts: number }) {
  return (
    <div className="bg-zinc-900/40 rounded-lg p-3 flex items-center justify-between">
      <span className="text-xs font-semibold text-zinc-400">{label}</span>
      <span className="text-sm font-black tabular-nums text-cyan-400">{watts.toFixed(1)}<span className="text-xs text-zinc-500 ml-1">W</span></span>
    </div>
  );
}

function TopCard({ label, icon: Icon, value, unit, sub, color, percent }: {
  label: string; icon: React.ElementType; value: string; unit?: string; sub: string;
  color: string; percent: number;
}) {
  return (
    <Card className="relative overflow-hidden">
      <div className="flex items-start justify-between mb-3">
        <div>
          <span className="text-[10px] font-bold uppercase tracking-wider text-zinc-500">{label}</span>
          <div className="flex items-baseline gap-1 mt-1">
            <span className="text-2xl font-black tabular-nums">{value}</span>
            {unit && <span className="text-sm text-zinc-500">{unit}</span>}
          </div>
        </div>
        <RingGauge value={percent} size={44} stroke={4} color={color}>
          <Icon className="w-4 h-4 text-zinc-400" />
        </RingGauge>
      </div>
      <span className="text-[11px] text-zinc-500">{sub}</span>
    </Card>
  );
}

// ─── Main Component ───────────────────────────────────────────

export default function SystemMonitor() {
  const [data, setData] = useState<SystemMonitorData | null>(null);
  const [fans, setFans] = useState<CcxtStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState("");
  const intervalRef = useRef<number | null>(null);
  const fetchingRef = useRef(false);

  useEffect(() => {
    const refresh = async () => {
      if (fetchingRef.current) return;
      fetchingRef.current = true;
      try {
        const [sysData, fanData] = await Promise.all([
          api.getSystemMonitor(),
          api.corsairCcxtPoll().catch(() => null),
        ]);
        setData(sysData);
        setFans(fanData);
        setError("");
      } catch (e) {
        console.error(e);
        setError(String(e));
      }
      setLoading(false);
      fetchingRef.current = false;
    };
    refresh();
    intervalRef.current = window.setInterval(refresh, 3000);
    return () => { if (intervalRef.current) clearInterval(intervalRef.current); };
  }, []);

  if (loading) return <div className="p-8"><Loading /></div>;
  if (error && !data) return (
    <div className="p-8">
      <PageHeader title="System Monitor" description="Fehler beim Laden" />
      <Card className="p-6 text-red-400 text-sm">{error}</Card>
    </div>
  );
  if (!data) return <div className="p-8"><Loading /></div>;

  const { cpu, memory, swap, cpu_sensor, gpu, uptime, extra_power, nvme_temps } = data;
  const hasGpu = gpu.name !== "" || gpu.temp_celsius !== null || gpu.power_watts !== null;
  const totalPower = extra_power.total_system_watts ?? ((cpu_sensor.power_watts || 0) + (gpu.power_watts || 0) + (extra_power.dram_watts || 0));

  return (
    <div className="p-8 max-w-5xl mx-auto">
      <PageHeader
        title="System Monitor"
        description={`${cpu.model.replace(/\(TM\)|\(R\)/g, "")} — Uptime: ${uptime.formatted}`}
      />

      {/* ─── Overview Cards ─── */}
      <div className={`grid ${hasGpu ? "grid-cols-5" : "grid-cols-4"} gap-3 mb-6`}>
        <TopCard label="CPU" icon={Cpu}
          value={cpu.usage_percent.toFixed(1)} unit="%"
          sub={`${cpu.cores}C/${cpu.threads}T${cpu.frequency_mhz ? ` · ${(cpu.frequency_mhz / 1000).toFixed(2)} GHz` : ""}`}
          color={usageColor(cpu.usage_percent)} percent={cpu.usage_percent} />

        {hasGpu && (
          <TopCard label="GPU" icon={MonitorIcon}
            value={gpu.gpu_busy_percent !== null ? `${gpu.gpu_busy_percent}` : "—"} unit={gpu.gpu_busy_percent !== null ? "%" : ""}
            sub={[gpu.name ? gpu.name.replace(/\[|\]/g, "").replace(/\(.*?\)/, "").trim() : "GPU", gpu.gpu_clock_mhz ? `${gpu.gpu_clock_mhz} MHz` : null].filter(Boolean).join(" · ")}
            color="purple" percent={gpu.gpu_busy_percent || 0} />
        )}

        <TopCard label="RAM" icon={MemoryStick}
          value={(memory.used_mib / 1024).toFixed(1)} unit="GiB"
          sub={`von ${(memory.total_mib / 1024).toFixed(1)} GiB · ${memory.percent.toFixed(0)}%`}
          color={usageColor(memory.percent)} percent={memory.percent} />

        {(() => {
          const probeTemp = fans?.connected
            ? fans.temps.find(t => t.connected)?.temp ?? null
            : null;
          const heroTemp = probeTemp ?? cpu_sensor.temp_celsius;
          return (
            <TopCard label="Wasser" icon={Thermometer}
              value={heroTemp !== null ? heroTemp.toFixed(0) : "—"} unit="°C"
              sub={[
                cpu_sensor.temp_celsius !== null ? `CPU ${cpu_sensor.temp_celsius.toFixed(0)}°` : null,
                gpu.temp_celsius !== null ? `GPU ${gpu.temp_celsius.toFixed(0)}°` : null,
              ].filter(Boolean).join(" · ") || "N/A"}
              color={heroTemp !== null && heroTemp >= 65 ? "amber" : "emerald"}
              percent={heroTemp ? Math.min(heroTemp, 100) : 0} />
          );
        })()}

        <TopCard label="Energie" icon={Zap}
          value={totalPower > 0 ? totalPower.toFixed(0) : "—"} unit="W"
          sub={[
            cpu_sensor.power_watts ? `CPU ${cpu_sensor.power_watts.toFixed(0)}W` : null,
            gpu.power_watts ? `GPU ${gpu.power_watts.toFixed(0)}W` : null,
            extra_power.dram_watts ? `RAM ${extra_power.dram_watts.toFixed(0)}W` : null,
          ].filter(Boolean).join(" · ") || "N/A"}
          color={totalPower > 200 ? "red" : totalPower > 100 ? "amber" : "cyan"}
          percent={Math.min((totalPower / 400) * 100, 100)} />
      </div>

      {/* ─── CPU Detail + Thermal ─── */}
      <div className="grid grid-cols-3 gap-4 mb-6">
        <Card className="col-span-2">
          <SectionTitle icon={Cpu}>Prozessor</SectionTitle>

          <div className="mb-4">
            <div className="flex justify-between text-[11px] text-zinc-500 mb-1.5">
              <span>Gesamtauslastung</span>
              <span className="font-mono font-bold text-zinc-300">{cpu.usage_percent.toFixed(1)}%</span>
            </div>
            <Bar value={cpu.usage_percent} color={usageColor(cpu.usage_percent)} />
          </div>

          {cpu.per_core_usage.length > 0 && (
            <div className="grid grid-cols-2 gap-x-6 gap-y-2 mb-4">
              {cpu.per_core_usage.map((pct, i) => (
                <div key={i} className="flex items-center gap-2">
                  <span className="text-[10px] text-zinc-600 w-6 text-right font-mono">C{i}</span>
                  <div className="flex-1"><Bar value={pct} color={usageColor(pct)} h="h-1" /></div>
                  <span className={`text-[10px] w-8 text-right font-mono tabular-nums ${pct > 70 ? "text-amber-400" : "text-zinc-500"}`}>
                    {pct.toFixed(0)}%
                  </span>
                </div>
              ))}
            </div>
          )}

          <div className="grid grid-cols-4 gap-3 pt-3 border-t border-zinc-800/50">
            <SpecPill label="Kerne" value={`${cpu.cores}`} />
            <SpecPill label="Threads" value={`${cpu.threads}`} />
            <SpecPill label="Frequenz" value={cpu.frequency_mhz ? `${(cpu.frequency_mhz / 1000).toFixed(2)} GHz` : "—"} />
            <SpecPill label="Architektur" value={cpu.architecture || "—"} />
          </div>
        </Card>

        <Card>
          <SectionTitle icon={Thermometer}>Thermik & Energie</SectionTitle>
          <div className="space-y-4">
            {cpu_sensor.temp_celsius !== null && (
              <ThermalRow label="CPU" temp={cpu_sensor.temp_celsius} watts={cpu_sensor.power_watts} />
            )}
            {gpu.temp_celsius !== null && (
              <ThermalRow label="GPU" temp={gpu.temp_celsius} watts={gpu.power_watts} />
            )}
            {extra_power.dram_watts !== null && (
              <PowerRow label="RAM (DRAM)" watts={extra_power.dram_watts} />
            )}
            {extra_power.platform_watts !== null && (
              <PowerRow label="Platform (PSys)" watts={extra_power.platform_watts} />
            )}
            {data.battery && (
              <PowerRow label="Batterie" watts={data.battery.power_watts} />
            )}
            {nvme_temps.length > 0 && nvme_temps.map((nvme, i) => (
              <div key={i} className="bg-zinc-900/40 rounded-lg p-3">
                <div className="flex items-center justify-between mb-2">
                  <span className="text-xs font-semibold text-zinc-400 truncate" title={nvme.name}>
                    {nvme.name.length > 22 ? nvme.name.slice(0, 22) + "…" : nvme.name}
                  </span>
                  <span className={`text-sm font-black tabular-nums ${tempColor(nvme.temp_celsius)}`}>
                    {nvme.temp_celsius.toFixed(0)}°C
                  </span>
                </div>
                <div className="w-full bg-zinc-800 rounded-full h-1.5 overflow-hidden">
                  <div className={`h-1.5 rounded-full transition-all duration-500 ${tempBg(nvme.temp_celsius)}`}
                    style={{ width: `${Math.min((nvme.temp_celsius / 80) * 100, 100)}%` }} />
                </div>
              </div>
            ))}
            {cpu_sensor.power_no_permission && (
              <div className="text-[10px] text-amber-500/80 bg-amber-500/5 rounded-lg px-3 py-2">
                ⚡ RAPL: Keine Berechtigung (root nötig für exakte CPU-Power)
              </div>
            )}

          </div>
        </Card>
      </div>

      {/* ─── Corsair Lüfter ─── */}
      {fans && fans.connected && fans.fans.some(f => f.connected) && (
        <Card className="mb-6">
          <SectionTitle icon={Fan}>Lüfter — {fans.product}</SectionTitle>
          <div className="grid grid-cols-3 gap-3">
            {fans.fans.filter(f => f.connected).map((f) => {
              const rpmPct = Math.min((f.rpm / 2000) * 100, 100);
              const color = f.rpm > 1600 ? "red" : f.rpm > 1200 ? "amber" : "cyan";
              return (
                <div key={f.channel} className="bg-zinc-900/50 rounded-lg p-3 border border-zinc-800/50">
                  <div className="flex items-center justify-between mb-2">
                    <div className="flex items-center gap-2">
                      <RingGauge value={rpmPct} size={36} stroke={3} color={color}>
                        <Fan className="w-3.5 h-3.5 text-zinc-400" />
                      </RingGauge>
                      <span className="text-xs font-semibold text-zinc-400">Fan {f.channel + 1}</span>
                    </div>
                    <span className="text-sm font-black tabular-nums text-zinc-200">
                      {f.rpm}<span className="text-[10px] text-zinc-500 ml-0.5">RPM</span>
                    </span>
                  </div>
                  <Bar value={rpmPct} color={color} h="h-1" />
                  {f.duty > 0 && (
                    <div className="text-[10px] text-zinc-600 mt-1 text-right font-mono">{f.duty}%</div>
                  )}
                </div>
              );
            })}
          </div>

        </Card>
      )}

      {/* ─── Memory + GPU ─── */}
      <div className={`grid ${hasGpu ? "grid-cols-2" : "grid-cols-1"} gap-4`}>
        <Card>
          <SectionTitle icon={MemoryStick}>Arbeitsspeicher</SectionTitle>
          <div className="grid grid-cols-2 gap-4 mb-4">
            <div className="flex items-center gap-4">
              <RingGauge value={memory.percent} size={72} stroke={6} color={usageColor(memory.percent)}>
                <span className="text-sm font-black tabular-nums">{memory.percent.toFixed(0)}%</span>
              </RingGauge>
              <div>
                <div className="text-xs text-zinc-500">RAM</div>
                <div className="text-sm font-bold">{(memory.used_mib / 1024).toFixed(1)} <span className="text-zinc-500 font-normal">GiB</span></div>
                <div className="text-[10px] text-zinc-600">von {(memory.total_mib / 1024).toFixed(1)} GiB</div>
              </div>
            </div>
            {swap.total_mib > 0 && (
              <div className="flex items-center gap-4">
                <RingGauge value={swap.percent} size={72} stroke={6} color="amber">
                  <span className="text-sm font-black tabular-nums">{swap.percent.toFixed(0)}%</span>
                </RingGauge>
                <div>
                  <div className="text-xs text-zinc-500">Swap</div>
                  <div className="text-sm font-bold">{(swap.used_mib / 1024).toFixed(1)} <span className="text-zinc-500 font-normal">GiB</span></div>
                  <div className="text-[10px] text-zinc-600">von {(swap.total_mib / 1024).toFixed(1)} GiB</div>
                </div>
              </div>
            )}
          </div>
          <div className="grid grid-cols-3 gap-3 pt-3 border-t border-zinc-800/50">
            <SpecPill label="Gesamt" value={`${(memory.total_mib / 1024).toFixed(1)} GiB`} />
            <SpecPill label="Verfügbar" value={`${(memory.available_mib / 1024).toFixed(1)} GiB`} />
            <SpecPill label="Swap" value={swap.total_mib > 0 ? `${(swap.total_mib / 1024).toFixed(1)} GiB` : "—"} />
          </div>
        </Card>

        {hasGpu && (
          <Card>
            <SectionTitle icon={MonitorIcon}>GPU — {gpu.name ? gpu.name.replace(/\[|\]/g, "").replace(/\(.*?\)/, "").trim() : "Grafikkarte"}</SectionTitle>
            <div className="grid grid-cols-2 gap-4 mb-4">
              <div className="flex items-center gap-4">
                <RingGauge value={gpu.gpu_busy_percent || 0} size={72} stroke={6} color="purple">
                  <span className="text-sm font-black tabular-nums">{gpu.gpu_busy_percent ?? 0}%</span>
                </RingGauge>
                <div>
                  <div className="text-xs text-zinc-500">Auslastung</div>
                  <div className="text-sm font-bold">{gpu.gpu_busy_percent !== null ? `${gpu.gpu_busy_percent}%` : "—"}</div>
                  <div className="text-[10px] text-zinc-600">GPU Core</div>
                </div>
              </div>
              {gpu.vram_total_mib != null && gpu.vram_total_mib > 0 && gpu.vram_used_mib !== null && (
                <div className="flex items-center gap-4">
                  <RingGauge value={gpu.vram_used_mib} max={gpu.vram_total_mib} size={72} stroke={6} color="blue">
                    <span className="text-sm font-black tabular-nums">
                      {Math.round((gpu.vram_used_mib / gpu.vram_total_mib) * 100)}%
                    </span>
                  </RingGauge>
                  <div>
                    <div className="text-xs text-zinc-500">VRAM</div>
                    <div className="text-sm font-bold">{(gpu.vram_used_mib / 1024).toFixed(1)} <span className="text-zinc-500 font-normal">GiB</span></div>
                    <div className="text-[10px] text-zinc-600">von {(gpu.vram_total_mib / 1024).toFixed(0)} GiB</div>
                  </div>
                </div>
              )}
            </div>
            <div className="grid grid-cols-3 gap-3 pt-3 border-t border-zinc-800/50">
              <SpecPill label="Temperatur" value={gpu.temp_celsius !== null ? `${gpu.temp_celsius.toFixed(0)}°C` : "—"}
                highlight={gpu.temp_celsius !== null && gpu.temp_celsius >= 65} />
              <SpecPill label="Verbrauch" value={gpu.power_watts !== null ? `${gpu.power_watts.toFixed(0)} W` : "—"} />
              <SpecPill label="GPU-Takt" value={gpu.gpu_clock_mhz ? `${gpu.gpu_clock_mhz} MHz` : "—"} />
            </div>
          </Card>
        )}
      </div>
    </div>
  );
}

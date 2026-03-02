import { useEffect, useState, useRef } from "react";
import { Cpu, MemoryStick, Thermometer, Zap, HardDrive, Activity } from "lucide-react";
import { api, SystemMonitorData } from "../api";

// ─── Helpers ──────────────────────────────────────────────────

function tempColor(t: number) {
  return t >= 80 ? "text-red-400" : t >= 65 ? "text-amber-400" : "text-emerald-400";
}

function usageColor(p: number) {
  return p >= 90 ? "text-red-400" : p >= 70 ? "text-amber-400" : "text-cyan-400";
}

function usageBg(p: number) {
  return p >= 90 ? "bg-red-500" : p >= 70 ? "bg-amber-500" : "bg-cyan-500";
}

function MiniBar({ value, max = 100, color }: { value: number; max?: number; color?: string }) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  const bg = color || usageBg(pct);
  return (
    <div className="w-full bg-zinc-800/80 rounded-full h-1.5 overflow-hidden">
      <div className={`${bg} h-1.5 rounded-full transition-all duration-700`} style={{ width: `${pct}%` }} />
    </div>
  );
}

function StatRow({ icon: Icon, label, value, sub, color }: {
  icon: typeof Cpu; label: string; value: string; sub?: string; color?: string;
}) {
  return (
    <div className="flex items-center gap-3 py-1.5">
      <Icon className={`w-4 h-4 shrink-0 ${color || "text-cyan-400"}`} />
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline justify-between">
          <span className="text-[11px] text-zinc-400 font-medium">{label}</span>
          <span className="text-sm font-bold tabular-nums text-zinc-100">{value}</span>
        </div>
        {sub && <span className="text-[10px] text-zinc-600 block truncate">{sub}</span>}
      </div>
    </div>
  );
}

// ─── Widget ───────────────────────────────────────────────────

export default function Widget() {
  const [data, setData] = useState<SystemMonitorData | null>(null);
  const intervalRef = useRef<ReturnType<typeof setInterval> | undefined>(undefined);

  useEffect(() => {
    const fetch = () => api.getSystemMonitor().then(setData).catch(() => {});
    fetch();
    intervalRef.current = setInterval(fetch, 2000);
    return () => clearInterval(intervalRef.current);
  }, []);

  if (!data) {
    return (
      <div className="w-full h-full flex items-center justify-center bg-zinc-950/80 rounded-2xl" data-tauri-drag-region>
        <div className="animate-pulse text-zinc-600 text-sm">Laden…</div>
      </div>
    );
  }

  const { cpu, memory, swap, cpu_sensor, gpu, load, extra_power, nvme_temps } = data;

  const totalPower = extra_power.total_system_watts
    ?? ((cpu_sensor.power_watts ?? 0) + (gpu.power_watts ?? 0) + (extra_power.dram_watts ?? 0));

  const memPercent = memory.percent;
  const swapPercent = swap.total_mib > 0 ? swap.percent : 0;

  return (
    <div
      className="w-full h-full bg-zinc-950/75 backdrop-blur-xl rounded-2xl border border-zinc-800/50 shadow-2xl overflow-hidden select-none"
      data-tauri-drag-region
    >
      {/* Header — draggable */}
      <div className="px-4 pt-3 pb-2 border-b border-zinc-800/50 flex items-center gap-2" data-tauri-drag-region>
        <Activity className="w-4 h-4 text-cyan-400" />
        <span className="text-xs font-bold tracking-wide text-zinc-300" data-tauri-drag-region>BACKSNAP</span>
        <span className="ml-auto text-[10px] text-zinc-600 tabular-nums">{data.uptime.formatted}</span>
      </div>

      {/* Content */}
      <div className="px-4 py-2 space-y-1">
        {/* CPU */}
        <StatRow
          icon={Cpu}
          label="CPU"
          value={`${cpu.usage_percent.toFixed(0)}%`}
          sub={cpu.model.length > 30 ? cpu.model.slice(0, 30) + "…" : cpu.model}
          color={usageColor(cpu.usage_percent)}
        />
        <MiniBar value={cpu.usage_percent} />

        {/* RAM */}
        <StatRow
          icon={MemoryStick}
          label="RAM"
          value={`${(memory.used_mib / 1024).toFixed(1)} / ${(memory.total_mib / 1024).toFixed(0)} GB`}
          sub={`${memPercent.toFixed(0)}% belegt`}
          color={usageColor(memPercent)}
        />
        <MiniBar value={memPercent} />

        {/* Swap */}
        {swap.total_mib > 0 && (
          <>
            <StatRow
              icon={HardDrive}
              label="Swap"
              value={`${(swap.used_mib / 1024).toFixed(1)} / ${(swap.total_mib / 1024).toFixed(1)} GB`}
              sub={`${swapPercent.toFixed(0)}%`}
              color={usageColor(swapPercent)}
            />
            <MiniBar value={swapPercent} />
          </>
        )}

        {/* Temps */}
        <div className="border-t border-zinc-800/40 mt-2 pt-2">
          {cpu_sensor.temp_celsius !== null && (
            <StatRow
              icon={Thermometer}
              label="CPU Temp"
              value={`${cpu_sensor.temp_celsius.toFixed(0)}°C`}
              sub={cpu_sensor.power_watts !== null ? `${cpu_sensor.power_watts.toFixed(1)} W` : undefined}
              color={tempColor(cpu_sensor.temp_celsius)}
            />
          )}
          {gpu.temp_celsius !== null && (
            <StatRow
              icon={Thermometer}
              label={gpu.name.length > 14 ? gpu.name.slice(0, 14) + "…" : gpu.name}
              value={`${gpu.temp_celsius.toFixed(0)}°C`}
              sub={[
                gpu.power_watts !== null ? `${gpu.power_watts.toFixed(0)} W` : null,
                gpu.gpu_busy_percent !== null ? `${gpu.gpu_busy_percent.toFixed(0)}% Last` : null,
              ].filter(Boolean).join(" · ") || undefined}
              color={tempColor(gpu.temp_celsius)}
            />
          )}
          {nvme_temps.length > 0 && nvme_temps.slice(0, 3).map((nvme, i) => (
            <StatRow
              key={i}
              icon={HardDrive}
              label={nvme.name.length > 14 ? nvme.name.slice(0, 14) + "…" : nvme.name}
              value={`${nvme.temp_celsius.toFixed(0)}°C`}
              color={tempColor(nvme.temp_celsius)}
            />
          ))}
        </div>

        {/* Power */}
        {totalPower > 0 && (
          <div className="border-t border-zinc-800/40 mt-1 pt-2">
            <StatRow
              icon={Zap}
              label="Gesamt"
              value={`${totalPower.toFixed(0)} W`}
              sub={[
                extra_power.dram_watts !== null ? `DRAM ${extra_power.dram_watts.toFixed(1)}W` : null,
                extra_power.platform_watts !== null ? `PSys ${extra_power.platform_watts.toFixed(1)}W` : null,
              ].filter(Boolean).join(" · ") || undefined}
              color="text-amber-400"
            />
          </div>
        )}

        {/* Load */}
        <div className="border-t border-zinc-800/40 mt-1 pt-2 flex items-center gap-3">
          <span className="text-[10px] text-zinc-600 font-bold">LOAD</span>
          {[
            { l: "1m", v: load.one },
            { l: "5m", v: load.five },
            { l: "15m", v: load.fifteen },
          ].map(({ l, v }) => {
            const ratio = v / cpu.threads;
            const c = ratio >= 1 ? "text-red-400" : ratio >= 0.7 ? "text-amber-400" : "text-emerald-400";
            return (
              <div key={l} className="text-center flex-1">
                <div className={`text-xs font-bold tabular-nums ${c}`}>{v.toFixed(2)}</div>
                <div className="text-[9px] text-zinc-600">{l}</div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

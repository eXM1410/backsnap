import { useEffect, useState, useRef } from "react";
import { Cpu, MemoryStick, Gauge, Thermometer, Zap, Monitor as MonitorIcon } from "lucide-react";
import { api, SystemMonitorData } from "../api";
import { Card, Badge, PageHeader, Loading } from "../components/ui";

function ProgressBar({
  value,
  max = 100,
  color = "cyan",
  size = "md",
}: {
  value: number;
  max?: number;
  color?: string;
  size?: "sm" | "md";
}) {
  const pct = max > 0 ? Math.min((value / max) * 100, 100) : 0;
  const colors: Record<string, string> = {
    cyan: "bg-cyan-500",
    emerald: "bg-emerald-500",
    amber: "bg-amber-500",
    red: "bg-red-500",
    purple: "bg-purple-500",
    blue: "bg-blue-500",
  };
  const h = size === "sm" ? "h-1.5" : "h-2.5";
  return (
    <div className={`w-full bg-zinc-800 rounded-full ${h}`}>
      <div
        className={`${colors[color] || "bg-cyan-500"} ${h} rounded-full transition-all duration-500`}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

function StatValue({ label, value, unit }: { label: string; value: string | number; unit?: string }) {
  return (
    <div className="flex items-center justify-between">
      <span className="text-sm text-zinc-500">{label}</span>
      <span className="text-sm font-mono">
        {value}
        {unit && <span className="text-zinc-500 ml-1">{unit}</span>}
      </span>
    </div>
  );
}

function tempColor(temp: number): string {
  if (temp >= 80) return "text-red-400";
  if (temp >= 65) return "text-amber-400";
  return "text-emerald-400";
}

function usageColor(pct: number): string {
  if (pct >= 90) return "red";
  if (pct >= 70) return "amber";
  return "cyan";
}

export default function SystemMonitor() {
  const [data, setData] = useState<SystemMonitorData | null>(null);
  const [loading, setLoading] = useState(true);
  const intervalRef = useRef<number | null>(null);

  const refresh = async () => {
    try {
      const d = await api.getSystemMonitor();
      setData(d);
    } catch (e) {
      console.error("Monitor error:", e);
    }
    setLoading(false);
  };

  useEffect(() => {
    refresh();
    intervalRef.current = window.setInterval(refresh, 1500);
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  if (loading || !data) return <div className="p-8"><Loading /></div>;

  const { cpu, memory, swap, cpu_sensor, gpu, load, uptime, battery } = data;
  const hasGpu = gpu.name !== "" || gpu.temp_celsius !== null || gpu.power_watts !== null;

  return (
    <div className="p-8">
      <PageHeader
        title="System Monitor"
        description={`${cpu.model} — Uptime: ${uptime.formatted}`}
      />

      {/* Top Cards */}
      <div className="grid grid-cols-4 gap-4 mb-6">
        <Card>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-zinc-400 uppercase">CPU</span>
            <Cpu className="w-4 h-4 text-cyan-400" />
          </div>
          <div className="text-2xl font-bold">{cpu.usage_percent.toFixed(1)}%</div>
          <div className="text-xs text-zinc-500 mt-1">
            {cpu.cores}C / {cpu.threads}T
            {cpu.frequency_mhz && ` — ${(cpu.frequency_mhz / 1000).toFixed(2)} GHz`}
          </div>
        </Card>

        <Card>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-zinc-400 uppercase">RAM</span>
            <MemoryStick className="w-4 h-4 text-emerald-400" />
          </div>
          <div className="text-2xl font-bold">{memory.percent.toFixed(1)}%</div>
          <div className="text-xs text-zinc-500 mt-1">
            {(memory.used_mib / 1024).toFixed(1)} / {(memory.total_mib / 1024).toFixed(1)} GiB
          </div>
        </Card>

        <Card>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-zinc-400 uppercase">
              {hasGpu ? "GPU" : "Swap"}
            </span>
            {hasGpu ? (
              <MonitorIcon className="w-4 h-4 text-purple-400" />
            ) : (
              <Gauge className="w-4 h-4 text-amber-400" />
            )}
          </div>
          {hasGpu ? (
            <>
              <div className="text-2xl font-bold">
                {gpu.gpu_busy_percent !== null ? `${gpu.gpu_busy_percent}%` : "—"}
              </div>
              <div className="text-xs text-zinc-500 mt-1 truncate">{gpu.name || "GPU"}</div>
            </>
          ) : (
            <>
              <div className="text-2xl font-bold">{swap.percent.toFixed(1)}%</div>
              <div className="text-xs text-zinc-500 mt-1">
                {(swap.used_mib / 1024).toFixed(1)} / {(swap.total_mib / 1024).toFixed(1)} GiB
              </div>
            </>
          )}
        </Card>

        <Card>
          <div className="flex items-center justify-between mb-2">
            <span className="text-xs font-semibold text-zinc-400 uppercase">Temperatur</span>
            <Thermometer className="w-4 h-4 text-red-400" />
          </div>
          <div className="flex items-baseline gap-3">
            {cpu_sensor.temp_celsius !== null && (
              <div>
                <span className={`text-2xl font-bold ${tempColor(cpu_sensor.temp_celsius)}`}>
                  {cpu_sensor.temp_celsius.toFixed(0)}°
                </span>
                <span className="text-xs text-zinc-500 ml-1">CPU</span>
              </div>
            )}
            {gpu.temp_celsius !== null && (
              <div>
                <span className={`text-2xl font-bold ${tempColor(gpu.temp_celsius)}`}>
                  {gpu.temp_celsius.toFixed(0)}°
                </span>
                <span className="text-xs text-zinc-500 ml-1">GPU</span>
              </div>
            )}
          </div>
        </Card>
      </div>

      {/* CPU Detail + Memory */}
      <div className="grid grid-cols-2 gap-4 mb-6">
        {/* CPU */}
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
            <Cpu className="w-4 h-4" /> CPU — {cpu.model.replace(/\(TM\)|\(R\)/g, "").trim()}
          </h3>

          <div className="mb-4">
            <div className="flex justify-between text-xs text-zinc-500 mb-1">
              <span>Gesamt</span>
              <span>{cpu.usage_percent.toFixed(1)}%</span>
            </div>
            <ProgressBar value={cpu.usage_percent} color={usageColor(cpu.usage_percent)} />
          </div>

          {/* Per-core bars */}
          {cpu.per_core_usage.length > 0 && (
            <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 mb-4">
              {cpu.per_core_usage.map((pct, i) => (
                <div key={i} className="flex items-center gap-2">
                  <span className="text-[10px] text-zinc-600 w-5 text-right">{i}</span>
                  <div className="flex-1">
                    <ProgressBar value={pct} color={usageColor(pct)} size="sm" />
                  </div>
                  <span className="text-[10px] text-zinc-500 w-8 text-right">
                    {pct.toFixed(0)}%
                  </span>
                </div>
              ))}
            </div>
          )}

          <div className="space-y-2 border-t border-zinc-800 pt-3">
            {cpu_sensor.temp_celsius !== null && (
              <StatValue
                label="Temperatur"
                value={cpu_sensor.temp_celsius.toFixed(1)}
                unit="°C"
              />
            )}
            {cpu_sensor.power_watts !== null && (
              <StatValue
                label="Verbrauch"
                value={cpu_sensor.power_watts.toFixed(1)}
                unit="W"
              />
            )}
            {cpu_sensor.power_no_permission && (
              <div className="text-xs text-amber-500">RAPL: Keine Berechtigung (root nötig)</div>
            )}
            {cpu.frequency_mhz && (
              <StatValue
                label="Frequenz"
                value={(cpu.frequency_mhz / 1000).toFixed(2)}
                unit="GHz"
              />
            )}
            <StatValue label="Kerne / Threads" value={`${cpu.cores} / ${cpu.threads}`} />
          </div>
        </Card>

        {/* Memory */}
        <Card>
          <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
            <MemoryStick className="w-4 h-4" /> Arbeitsspeicher
          </h3>

          <div className="mb-4">
            <div className="flex justify-between text-xs text-zinc-500 mb-1">
              <span>RAM</span>
              <span>
                {(memory.used_mib / 1024).toFixed(1)} / {(memory.total_mib / 1024).toFixed(1)} GiB
              </span>
            </div>
            <ProgressBar value={memory.percent} color={usageColor(memory.percent)} />
          </div>

          {swap.total_mib > 0 && (
            <div className="mb-4">
              <div className="flex justify-between text-xs text-zinc-500 mb-1">
                <span>Swap</span>
                <span>
                  {(swap.used_mib / 1024).toFixed(1)} / {(swap.total_mib / 1024).toFixed(1)} GiB
                </span>
              </div>
              <ProgressBar value={swap.percent} color="amber" />
            </div>
          )}

          <div className="space-y-2 border-t border-zinc-800 pt-3">
            <StatValue label="Gesamt" value={(memory.total_mib / 1024).toFixed(1)} unit="GiB" />
            <StatValue label="Belegt" value={(memory.used_mib / 1024).toFixed(1)} unit="GiB" />
            <StatValue label="Verfügbar" value={(memory.available_mib / 1024).toFixed(1)} unit="GiB" />
            {swap.total_mib > 0 && (
              <StatValue label="Swap belegt" value={(swap.used_mib / 1024).toFixed(1)} unit="GiB" />
            )}
          </div>

          {/* Load Average */}
          <div className="mt-4 pt-3 border-t border-zinc-800">
            <h4 className="text-xs font-semibold text-zinc-500 mb-2">Load Average</h4>
            <div className="flex gap-4">
              <LoadBadge label="1m" value={load.one} cores={cpu.threads} />
              <LoadBadge label="5m" value={load.five} cores={cpu.threads} />
              <LoadBadge label="15m" value={load.fifteen} cores={cpu.threads} />
            </div>
          </div>
        </Card>
      </div>

      {/* GPU + Power */}
      {hasGpu && (
        <div className="grid grid-cols-2 gap-4 mb-6">
          <Card>
            <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
              <MonitorIcon className="w-4 h-4" /> GPU — {gpu.name || "AMD GPU"}
            </h3>

            {gpu.gpu_busy_percent !== null && (
              <div className="mb-4">
                <div className="flex justify-between text-xs text-zinc-500 mb-1">
                  <span>GPU-Auslastung</span>
                  <span>{gpu.gpu_busy_percent}%</span>
                </div>
                <ProgressBar value={gpu.gpu_busy_percent} color="purple" />
              </div>
            )}

            {gpu.vram_total_mib && gpu.vram_used_mib !== null && (
              <div className="mb-4">
                <div className="flex justify-between text-xs text-zinc-500 mb-1">
                  <span>VRAM</span>
                  <span>
                    {(gpu.vram_used_mib / 1024).toFixed(1)} / {(gpu.vram_total_mib / 1024).toFixed(1)} GiB
                  </span>
                </div>
                <ProgressBar
                  value={gpu.vram_used_mib}
                  max={gpu.vram_total_mib}
                  color="blue"
                />
              </div>
            )}

            <div className="space-y-2 border-t border-zinc-800 pt-3">
              {gpu.temp_celsius !== null && (
                <StatValue label="Temperatur" value={gpu.temp_celsius.toFixed(1)} unit="°C" />
              )}
              {gpu.power_watts !== null && (
                <StatValue label="Verbrauch" value={gpu.power_watts.toFixed(1)} unit="W" />
              )}
              {gpu.vram_total_mib && (
                <StatValue label="VRAM Gesamt" value={(gpu.vram_total_mib / 1024).toFixed(0)} unit="GiB" />
              )}
            </div>
          </Card>

          {/* Power Summary */}
          <Card>
            <h3 className="text-sm font-semibold text-zinc-400 mb-4 flex items-center gap-2">
              <Zap className="w-4 h-4" /> Energieverbrauch
            </h3>

            <div className="space-y-4">
              {cpu_sensor.power_watts !== null && (
                <PowerMeter label="CPU" watts={cpu_sensor.power_watts} max={200} color="cyan" />
              )}
              {gpu.power_watts !== null && (
                <PowerMeter label="GPU" watts={gpu.power_watts} max={350} color="purple" />
              )}
              {cpu_sensor.power_watts !== null && gpu.power_watts !== null && (
                <div className="border-t border-zinc-800 pt-3">
                  <StatValue
                    label="Gesamt (CPU+GPU)"
                    value={(cpu_sensor.power_watts + gpu.power_watts).toFixed(0)}
                    unit="W"
                  />
                </div>
              )}
              {battery && (
                <div className="border-t border-zinc-800 pt-3">
                  <StatValue label="Akku" value={battery.power_watts.toFixed(1)} unit="W" />
                </div>
              )}
            </div>
          </Card>
        </div>
      )}
    </div>
  );
}

function LoadBadge({ label, value, cores }: { label: string; value: number; cores: number }) {
  const ratio = value / cores;
  const color = ratio >= 1.0 ? "red" : ratio >= 0.7 ? "yellow" : "green";
  return (
    <div className="text-center">
      <Badge color={color}>
        <span className="font-mono">{value.toFixed(2)}</span>
      </Badge>
      <div className="text-[10px] text-zinc-600 mt-1">{label}</div>
    </div>
  );
}

function PowerMeter({
  label,
  watts,
  max,
  color,
}: {
  label: string;
  watts: number;
  max: number;
  color: string;
}) {
  return (
    <div>
      <div className="flex justify-between text-xs text-zinc-500 mb-1">
        <span>{label}</span>
        <span className="font-mono">{watts.toFixed(1)} W</span>
      </div>
      <ProgressBar value={watts} max={max} color={color} />
    </div>
  );
}

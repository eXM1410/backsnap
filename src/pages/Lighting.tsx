import { useState, useRef, useCallback } from "react";
import { Lightbulb, Power, PowerOff, Loader2, Sun, Cpu, Lamp } from "lucide-react";
import { PageHeader, Button } from "../components/ui";
import { api, apiError } from "../api";
import CorsairSection from "./Corsair";
import OpenRGBSection from "./OpenRGB";

export default function Lighting() {
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [globalBrightness, setGlobalBrightness] = useState(100);
  const [goveeBri, setGoveeBri] = useState(100);
  const [rgbBri, setRgbBri] = useState(100);
  const globalBriTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const goveeBriTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);
  const rgbBriTimeout = useRef<ReturnType<typeof setTimeout> | null>(null);

  const flash = (msg: string, ms = 3000) => {
    setToast(msg);
    setTimeout(() => setToast(null), ms);
  };

  const sendGlobalBri = useCallback((val: number) => {
    if (globalBriTimeout.current) clearTimeout(globalBriTimeout.current);
    globalBriTimeout.current = setTimeout(async () => {
      try {
        const r = await api.lightingMasterBrightness(val);
        const parts: string[] = [];
        parts.push(r.corsairOk ? "Corsair ✓" : "Corsair ✗");
        parts.push(r.openrgbOk ? "OpenRGB ✓" : "OpenRGB ✗");
        parts.push(r.goveeOk ? "Govee ✓" : "Govee ✗");
        flash(`${val}%: ${parts.join(" · ")}`, 2000);
      } catch (e) {
        flash("Fehler: " + apiError(e));
      }
    }, 300);
  }, []);

  const sendGoveeBri = useCallback((val: number) => {
    if (goveeBriTimeout.current) clearTimeout(goveeBriTimeout.current);
    goveeBriTimeout.current = setTimeout(async () => {
      try {
        const [ok] = await api.goveeMasterBrightness(val);
        flash(ok ? `Govee ${val}%` : "Govee nicht erreichbar", 2000);
      } catch (e) {
        flash("Fehler: " + apiError(e));
      }
    }, 300);
  }, []);

  const sendRgbBri = useCallback((val: number) => {
    if (rgbBriTimeout.current) clearTimeout(rgbBriTimeout.current);
    rgbBriTimeout.current = setTimeout(async () => {
      try {
        const [ok, msg] = await api.rgbMasterBrightness(val);
        flash(ok ? `PC RGB ${msg}` : `Fehler: ${msg}`, 2000);
      } catch (e) {
        flash("Fehler: " + apiError(e));
      }
    }, 300);
  }, []);

  const masterPower = async (on: boolean) => {
    setBusy("all");
    try {
      const r = await api.lightingMasterPower(on);
      const parts: string[] = [];
      parts.push(r.corsairOk ? "Corsair ✓" : "Corsair ✗");
      parts.push(r.openrgbOk ? "OpenRGB ✓" : "OpenRGB ✗");
      parts.push(r.goveeOk ? "Govee ✓" : "Govee ✗");
      flash(`Alles ${on ? "AN" : "AUS"}: ${parts.join(" · ")}`);
    } catch (e) {
      flash("Fehler: " + apiError(e));
    } finally {
      setBusy(null);
    }
  };

  const goveePower = async (on: boolean) => {
    setBusy("govee");
    try {
      const [ok] = await api.goveeMasterPower(on);
      flash(ok ? `Govee ${on ? "AN" : "AUS"}` : "Govee nicht erreichbar");
    } catch (e) {
      flash("Fehler: " + apiError(e));
    } finally {
      setBusy(null);
    }
  };

  const rgbPower = async (on: boolean) => {
    setBusy("rgb");
    try {
      const [ok, msg] = await api.rgbMasterPower(on);
      flash(ok ? `PC RGB ${on ? "AN" : "AUS"}: ${msg}` : `Fehler: ${msg}`);
    } catch (e) {
      flash("Fehler: " + apiError(e));
    } finally {
      setBusy(null);
    }
  };

  const Spin = () => <Loader2 className="w-3 h-3 animate-spin" />;

  return (
    <div className="p-6 max-w-5xl mx-auto space-y-8">
      <PageHeader title="Lighting" description="RGB-Steuerung für alle Geräte" />

      {/* ── Control Bar ─────────────────────────────────── */}
      <div className="flex flex-wrap items-stretch gap-3">
        {/* Global */}
        <div className="flex items-center gap-2 bg-zinc-800/60 border border-zinc-700/50 rounded-xl px-4 py-2.5">
          <span className="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 mr-1">Global</span>
          <Button onClick={() => masterPower(true)} disabled={!!busy} size="sm">
            {busy === "all" ? <Spin /> : <Power className="w-3.5 h-3.5" />}
          </Button>
          <Button onClick={() => masterPower(false)} disabled={!!busy} variant="secondary" size="sm">
            <PowerOff className="w-3.5 h-3.5" />
          </Button>
          <div className="w-px h-5 bg-zinc-700 mx-1" />
          <Sun className="w-3.5 h-3.5 text-yellow-400 shrink-0" />
          <input
            type="range"
            min={1}
            max={100}
            value={globalBrightness}
            onChange={(e) => {
              const v = Number(e.target.value);
              setGlobalBrightness(v);
              sendGlobalBri(v);
            }}
            className="w-24 h-1.5 accent-yellow-400 cursor-pointer"
          />
          <span className="text-xs text-zinc-400 tabular-nums w-7 text-right">{globalBrightness}%</span>
        </div>

        {/* Govee */}
        <div className="flex items-center gap-2 bg-zinc-800/60 border border-zinc-700/50 rounded-xl px-4 py-2.5">
          <Lamp className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 mr-1">Govee</span>
          <Button onClick={() => goveePower(true)} disabled={!!busy} size="sm">
            {busy === "govee" ? <Spin /> : <Power className="w-3.5 h-3.5" />}
          </Button>
          <Button onClick={() => goveePower(false)} disabled={!!busy} variant="secondary" size="sm">
            <PowerOff className="w-3.5 h-3.5" />
          </Button>
          <div className="w-px h-5 bg-zinc-700 mx-1" />
          <Sun className="w-3.5 h-3.5 text-cyan-400 shrink-0" />
          <input
            type="range"
            min={1}
            max={100}
            value={goveeBri}
            onChange={(e) => {
              const v = Number(e.target.value);
              setGoveeBri(v);
              sendGoveeBri(v);
            }}
            className="w-24 h-1.5 accent-cyan-400 cursor-pointer"
          />
          <span className="text-xs text-zinc-400 tabular-nums w-7 text-right">{goveeBri}%</span>
        </div>

        {/* PC RGB */}
        <div className="flex items-center gap-2 bg-zinc-800/60 border border-zinc-700/50 rounded-xl px-4 py-2.5">
          <Cpu className="w-3.5 h-3.5 text-purple-400 shrink-0" />
          <span className="text-[11px] font-semibold uppercase tracking-wider text-zinc-500 mr-1">PC RGB</span>
          <Button onClick={() => rgbPower(true)} disabled={!!busy} size="sm">
            {busy === "rgb" ? <Spin /> : <Power className="w-3.5 h-3.5" />}
          </Button>
          <Button onClick={() => rgbPower(false)} disabled={!!busy} variant="secondary" size="sm">
            <PowerOff className="w-3.5 h-3.5" />
          </Button>
          <div className="w-px h-5 bg-zinc-700 mx-1" />
          <Sun className="w-3.5 h-3.5 text-purple-400 shrink-0" />
          <input
            type="range"
            min={1}
            max={100}
            value={rgbBri}
            onChange={(e) => {
              const v = Number(e.target.value);
              setRgbBri(v);
              sendRgbBri(v);
            }}
            className="w-24 h-1.5 accent-purple-400 cursor-pointer"
          />
          <span className="text-xs text-zinc-400 tabular-nums w-7 text-right">{rgbBri}%</span>
        </div>
      </div>

      {toast && (
        <div className="fixed bottom-6 right-6 z-50 bg-zinc-800 border border-zinc-700 rounded-xl px-4 py-3 shadow-2xl text-sm animate-in slide-in-from-bottom-4 flex items-center gap-2">
          <Lightbulb className="w-4 h-4 text-cyan-400 shrink-0" />
          {toast}
        </div>
      )}

      <CorsairSection />
      <OpenRGBSection />
    </div>
  );
}

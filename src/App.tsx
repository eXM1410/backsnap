import { useState, useEffect } from "react";
import { Routes, Route, NavLink, useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  LayoutDashboard,
  Camera,
  RefreshCw,
  Settings,
  HardDrive,
  Terminal,
  Activity,
  Shield,
  ShieldCheck,
  Trash2,
  Gauge,
  PanelLeftClose,
  PanelLeftOpen,
  Wifi,
  Lightbulb,
  Bot,
} from "lucide-react";
import Dashboard from "./pages/Dashboard";
import Snapshots from "./pages/Snapshots";
import Sync from "./pages/Sync";
import Disks from "./pages/Disks";
import Logs from "./pages/Logs";
import SettingsPage from "./pages/Settings";
import Monitor from "./pages/Monitor";
import Cleanup from "./pages/Cleanup";
import Tuning from "./pages/Tuning";
import PiRemote from "./pages/PiRemote";
import Lighting from "./pages/Lighting";
import BootGuard from "./pages/BootGuard";
import Assistant from "./pages/Assistant";
import Widget from "./pages/Widget";
import { ErrorBoundary } from "./components/ErrorBoundary";
import SetupWizard from "./components/SetupWizard";
import { api } from "./api";

const nav = [
  { to: "/", icon: Activity, label: "Monitor" },
  { to: "/dashboard", icon: LayoutDashboard, label: "Dashboard" },
  { to: "/snapshots", icon: Camera, label: "Snapshots" },
  { to: "/sync", icon: RefreshCw, label: "NVMe Sync" },
  { to: "/cleanup", icon: Trash2, label: "Aufräumen" },
  { to: "/tuning", icon: Gauge, label: "Tuning" },
  { to: "/pi", icon: Wifi, label: "Pi Remote" },
  { to: "/lighting", icon: Lightbulb, label: "Lighting" },
  { to: "/assistant", icon: Bot, label: "Assistant" },
  { to: "/boot-guard", icon: ShieldCheck, label: "Boot Guard" },
  { to: "/disks", icon: HardDrive, label: "Disks" },
  { to: "/logs", icon: Terminal, label: "Logs" },
  { to: "/settings", icon: Settings, label: "Einstellungen" },
];

export default function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const [collapsed, setCollapsed] = useState(true);
  const [showWizard, setShowWizard] = useState(false);

  // Listen for Rust "navigate-assistant" events (clap detection)
  useEffect(() => {
    const unlisten = listen("navigate-assistant", () => {
      navigate("/assistant");
    });
    return () => { unlisten.then(fn => fn()); };
  }, [navigate]);

  useEffect(() => {
    api.getConfig().then((cfg) => {
      if (!cfg.disks.primary_uuid) {
        setShowWizard(true);
      }
    }).catch(() => {
      // If config can't be loaded, show wizard anyway
      setShowWizard(true);
    });
  }, []);

  // Widget mode — standalone transparent window, no sidebar
  if (location.pathname === "/widget") {
    return (
      <div className="w-screen h-screen bg-transparent">
        <Widget />
      </div>
    );
  }

  // Assistant mode — fullscreen, no sidebar (Iron Man HUD)
  const isAssistant = location.pathname === "/assistant";

  // Toggle true fullscreen when entering/leaving assistant
  useEffect(() => {
    const win = getCurrentWindow();
    if (isAssistant) {
      win.setFullscreen(true);
    } else {
      win.setFullscreen(false);
    }
  }, [isAssistant]);

  return (
    <div className="flex h-screen overflow-hidden">
      {showWizard && <SetupWizard onComplete={() => setShowWizard(false)} />}
      {/* Sidebar — hidden on assistant page */}
      {!isAssistant && (
      <aside
        className={`${
          collapsed ? "w-16" : "w-56"
        } shrink-0 bg-zinc-900/50 border-r border-zinc-800 flex flex-col transition-all duration-200`}
      >
        {/* Logo */}
        <div className={`flex items-center ${collapsed ? "justify-center px-2" : "gap-2.5 px-5"} py-5 border-b border-zinc-800`}>
          <Shield className="w-7 h-7 text-cyan-400 shrink-0" />
          {!collapsed && <span className="text-lg font-bold tracking-tight">Arclight</span>}
        </div>

        {/* Navigation */}
        <nav className={`flex-1 py-3 ${collapsed ? "px-2" : "px-3"} space-y-0.5`}>
          {nav.map((item) => {
            const isActive =
              item.to === "/"
                ? location.pathname === "/"
                : location.pathname.startsWith(item.to);
            return (
              <NavLink
                key={item.to}
                to={item.to}
                title={collapsed ? item.label : undefined}
                className={`flex items-center ${
                  collapsed ? "justify-center px-0 py-2.5" : "gap-3 px-3 py-2.5"
                } rounded-lg text-sm font-medium transition-all duration-150 ${
                  isActive
                    ? "bg-cyan-500/10 text-cyan-400"
                    : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
                }`}
              >
                <item.icon className="w-[18px] h-[18px] shrink-0" />
                {!collapsed && item.label}
              </NavLink>
            );
          })}
        </nav>

        {/* Collapse Toggle + Footer */}
        <div className={`border-t border-zinc-800 ${collapsed ? "px-2" : "px-3"} py-2`}>
          <button
            onClick={() => setCollapsed(!collapsed)}
            className="flex items-center justify-center w-full py-2 rounded-lg text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800/50 transition-colors"
            title={collapsed ? "Menü aufklappen" : "Menü einklappen"}
          >
            {collapsed ? (
              <PanelLeftOpen className="w-[18px] h-[18px]" />
            ) : (
              <>
                <PanelLeftClose className="w-[18px] h-[18px] mr-2" />
                <span className="text-xs">Einklappen</span>
              </>
            )}
          </button>
        </div>
        {!collapsed && (
          <div className="px-4 py-2 border-t border-zinc-800 text-xs text-zinc-600">
            arclight v0.1.0
          </div>
        )}
      </aside>
      )}

      {/* Main Content */}
      <main className="flex-1 overflow-y-auto">
        <ErrorBoundary>
          <Routes>
            <Route path="/" element={<Monitor />} />
            <Route path="/dashboard" element={<Dashboard />} />
            <Route path="/snapshots" element={<Snapshots />} />
            <Route path="/sync" element={<Sync />} />
            <Route path="/cleanup" element={<Cleanup />} />
            <Route path="/tuning" element={<Tuning />} />
            <Route path="/pi" element={<PiRemote />} />
            <Route path="/lighting" element={<Lighting />} />
            <Route path="/assistant" element={<Assistant />} />
            <Route path="/boot-guard" element={<BootGuard />} />
            <Route path="/disks" element={<Disks />} />
            <Route path="/logs" element={<Logs />} />
            <Route path="/settings" element={<SettingsPage />} />
          </Routes>
        </ErrorBoundary>
      </main>
    </div>
  );
}

import { useState, useEffect } from "react";
import { Routes, Route, NavLink, Navigate, useLocation, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  Camera,
  Activity,
  Shield,
  Gauge,
  PanelLeftClose,
  PanelLeftOpen,
  HardDrive,
  Terminal,
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
import SplitView from "./components/SplitView";
import { ErrorBoundary } from "./components/ErrorBoundary";
import SetupWizard from "./components/SetupWizard";
import { api } from "./api";

const nav = [
  { to: "/", icon: Bot, label: "Assistant" },
  { to: "/system", icon: Activity, label: "System" },
  { to: "/snapshots", icon: Camera, label: "Snapshots" },
  { to: "/storage", icon: HardDrive, label: "Storage" },
  { to: "/smarthome", icon: Lightbulb, label: "Smart Home" },
  { to: "/config", icon: Gauge, label: "Config" },
  { to: "/utilities", icon: Terminal, label: "Utilities" },
];

export default function App() {
  const location = useLocation();
  const navigate = useNavigate();
  const [collapsed, setCollapsed] = useState(true);
  const [showWizard, setShowWizard] = useState(false);

  // Listen for Rust "navigate-assistant" events (clap detection)
  useEffect(() => {
    const unlisten = listen("navigate-assistant", () => {
      navigate("/");
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
  const isAssistant = location.pathname === "/";

  // Always run fullscreen
  useEffect(() => {
    getCurrentWindow().setFullscreen(true);
  }, []);

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
            const isActive = location.pathname === item.to || (item.to !== "/" && location.pathname.startsWith(item.to));
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
      <main className={`flex-1 ${isAssistant ? "overflow-hidden" : "overflow-y-auto"}`}>
        <ErrorBoundary>
          <Routes>
            <Route path="/" element={<Assistant />} />
            <Route path="/system" element={<SplitView left={<Monitor />} right={<Dashboard />} />} />
            <Route path="/snapshots" element={<SplitView left={<Snapshots />} right={<Cleanup />} />} />
            <Route path="/storage" element={<SplitView left={<Sync />} right={<Disks />} />} />
            <Route path="/smarthome" element={<SplitView left={<PiRemote />} right={<Lighting />} />} />
            <Route path="/config" element={<SplitView left={<Tuning />} right={<BootGuard />} />} />
            <Route path="/utilities" element={<SplitView left={<Logs />} right={<SettingsPage />} />} />
          </Routes>
        </ErrorBoundary>
      </main>
    </div>
  );
}

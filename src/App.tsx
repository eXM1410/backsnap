import { useState } from "react";
import { Routes, Route, NavLink, useLocation } from "react-router-dom";
import {
  LayoutDashboard,
  Camera,
  RefreshCw,
  Clock,
  Settings,
  HardDrive,
  Terminal,
  Activity,
  Shield,
  PanelLeftClose,
  PanelLeftOpen,
} from "lucide-react";
import Dashboard from "./pages/Dashboard";
import Snapshots from "./pages/Snapshots";
import Sync from "./pages/Sync";
import Schedule from "./pages/Schedule";
import Disks from "./pages/Disks";
import Logs from "./pages/Logs";
import SettingsPage from "./pages/Settings";
import Monitor from "./pages/Monitor";

const nav = [
  { to: "/", icon: LayoutDashboard, label: "Dashboard" },
  { to: "/snapshots", icon: Camera, label: "Snapshots" },
  { to: "/sync", icon: RefreshCw, label: "NVMe Sync" },
  { to: "/schedule", icon: Clock, label: "Zeitplan" },
  { to: "/monitor", icon: Activity, label: "Monitor" },
  { to: "/disks", icon: HardDrive, label: "Disks" },
  { to: "/logs", icon: Terminal, label: "Logs" },
  { to: "/settings", icon: Settings, label: "Einstellungen" },
];

export default function App() {
  const location = useLocation();
  const [collapsed, setCollapsed] = useState(false);

  return (
    <div className="flex h-screen overflow-hidden">
      {/* Sidebar */}
      <aside
        className={`${
          collapsed ? "w-16" : "w-56"
        } shrink-0 bg-zinc-900/50 border-r border-zinc-800 flex flex-col transition-all duration-200`}
      >
        {/* Logo */}
        <div className={`flex items-center ${collapsed ? "justify-center px-2" : "gap-2.5 px-5"} py-5 border-b border-zinc-800`}>
          <Shield className="w-7 h-7 text-cyan-400 shrink-0" />
          {!collapsed && <span className="text-lg font-bold tracking-tight">backsnap</span>}
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
                <item.icon className="w-4.5 h-4.5 shrink-0" />
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
              <PanelLeftOpen className="w-4.5 h-4.5" />
            ) : (
              <>
                <PanelLeftClose className="w-4.5 h-4.5 mr-2" />
                <span className="text-xs">Einklappen</span>
              </>
            )}
          </button>
        </div>
        {!collapsed && (
          <div className="px-4 py-2 border-t border-zinc-800 text-xs text-zinc-600">
            backsnap v0.1.0
          </div>
        )}
      </aside>

      {/* Main Content */}
      <main className="flex-1 overflow-y-auto">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/snapshots" element={<Snapshots />} />
          <Route path="/sync" element={<Sync />} />
          <Route path="/schedule" element={<Schedule />} />
          <Route path="/monitor" element={<Monitor />} />
          <Route path="/disks" element={<Disks />} />
          <Route path="/logs" element={<Logs />} />
          <Route path="/settings" element={<SettingsPage />} />
        </Routes>
      </main>
    </div>
  );
}

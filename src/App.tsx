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

  return (
    <div className="flex h-screen overflow-hidden">
      {/* Sidebar */}
      <aside className="w-56 shrink-0 bg-zinc-900/50 border-r border-zinc-800 flex flex-col">
        {/* Logo */}
        <div className="flex items-center gap-2.5 px-5 py-5 border-b border-zinc-800">
          <Shield className="w-7 h-7 text-cyan-400" />
          <span className="text-lg font-bold tracking-tight">backsnap</span>
        </div>

        {/* Navigation */}
        <nav className="flex-1 py-3 px-3 space-y-0.5">
          {nav.map((item) => {
            const isActive =
              item.to === "/"
                ? location.pathname === "/"
                : location.pathname.startsWith(item.to);
            return (
              <NavLink
                key={item.to}
                to={item.to}
                className={`flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm font-medium transition-all duration-150 ${
                  isActive
                    ? "bg-cyan-500/10 text-cyan-400"
                    : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
                }`}
              >
                <item.icon className="w-4.5 h-4.5" />
                {item.label}
              </NavLink>
            );
          })}
        </nav>

        {/* Footer */}
        <div className="px-4 py-3 border-t border-zinc-800 text-xs text-zinc-600">
          backsnap v0.1.0
        </div>
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

import { ReactNode, ComponentType } from "react";
import { Loader2 } from "lucide-react";

// ─── Shared Icon Type ─────────────────────────────────────────

type IconComponent = ComponentType<{ className?: string }>;

// ─── Card ─────────────────────────────────────────────────────

export function Card({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={`bg-zinc-900/60 border border-zinc-800 rounded-xl p-5 ${className}`}
    >
      {children}
    </div>
  );
}

// ─── Stat Card ────────────────────────────────────────────────

export function StatCard({
  label,
  value,
  icon: Icon,
  color = "text-cyan-400",
  sub,
}: {
  label: string;
  value: string | number;
  icon: IconComponent;
  color?: string;
  sub?: string;
}) {
  return (
    <Card>
      <div className="flex items-start justify-between">
        <div>
          <p className="text-xs text-zinc-500 uppercase tracking-wider mb-1">
            {label}
          </p>
          <p className="text-2xl font-bold">{value}</p>
          {sub && <p className="text-xs text-zinc-500 mt-1">{sub}</p>}
        </div>
        <div className={`p-2.5 rounded-lg bg-zinc-800/50 ${color}`}>
          <Icon className="w-5 h-5" />
        </div>
      </div>
    </Card>
  );
}

// ─── Button ───────────────────────────────────────────────────

export function Button({
  children,
  onClick,
  variant = "primary",
  size = "md",
  disabled = false,
  loading = false,
  className = "",
}: {
  children: ReactNode;
  onClick?: () => void;
  variant?: "primary" | "secondary" | "danger" | "ghost";
  size?: "sm" | "md" | "lg";
  disabled?: boolean;
  loading?: boolean;
  className?: string;
}) {
  const variants = {
    primary:
      "bg-cyan-500 hover:bg-cyan-400 text-zinc-950 font-semibold shadow-lg shadow-cyan-500/20",
    secondary:
      "bg-zinc-800 hover:bg-zinc-700 text-zinc-200 border border-zinc-700",
    danger:
      "bg-red-500/10 hover:bg-red-500/20 text-red-400 border border-red-500/30",
    ghost: "hover:bg-zinc-800 text-zinc-400 hover:text-zinc-200",
  };

  const sizes = {
    sm: "px-3 py-1.5 text-xs",
    md: "px-4 py-2 text-sm",
    lg: "px-6 py-3 text-base",
  };

  return (
    <button
      onClick={onClick}
      disabled={disabled || loading}
      className={`inline-flex items-center justify-center gap-2 rounded-lg transition-all duration-150 disabled:opacity-50 disabled:cursor-not-allowed ${variants[variant]} ${sizes[size]} ${className}`}
    >
      {loading && <Loader2 className="w-4 h-4 animate-spin" />}
      {children}
    </button>
  );
}

// ─── Badge ────────────────────────────────────────────────────

export function Badge({
  children,
  color = "cyan",
}: {
  children: ReactNode;
  color?: "cyan" | "green" | "red" | "yellow" | "zinc" | "amber" | "purple" | "blue" | "emerald";
}) {
  const colors: Record<string, string> = {
    cyan: "bg-cyan-500/10 text-cyan-400 border-cyan-500/20",
    green: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
    emerald: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
    red: "bg-red-500/10 text-red-400 border-red-500/20",
    yellow: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    amber: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    purple: "bg-purple-500/10 text-purple-400 border-purple-500/20",
    blue: "bg-blue-500/10 text-blue-400 border-blue-500/20",
    zinc: "bg-zinc-800 text-zinc-400 border-zinc-700",
  };

  return (
    <span
      className={`inline-flex items-center px-2 py-0.5 text-xs font-medium rounded-md border ${colors[color]}`}
    >
      {children}
    </span>
  );
}

// ─── Page Header ──────────────────────────────────────────────

export function PageHeader({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between mb-6">
      <div>
        <h1 className="text-2xl font-bold">{title}</h1>
        {description && (
          <p className="text-sm text-zinc-500 mt-1">{description}</p>
        )}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  );
}

// ─── Loading ──────────────────────────────────────────────────

export function Loading({ text = "Laden..." }: { text?: string }) {
  return (
    <div className="flex items-center justify-center py-12 text-zinc-500">
      <Loader2 className="w-5 h-5 animate-spin mr-2" />
      {text}
    </div>
  );
}

// ─── Empty State ──────────────────────────────────────────────

export function EmptyState({
  icon: Icon,
  title,
  description,
}: {
  icon: IconComponent;
  title: string;
  description?: string;
}) {
  return (
    <div className="flex flex-col items-center justify-center py-16 text-zinc-500">
      <Icon className="w-12 h-12 mb-3 opacity-30" />
      <p className="font-medium">{title}</p>
      {description && <p className="text-sm mt-1">{description}</p>}
    </div>
  );
}

import { type ReactNode } from "react";

interface SplitViewProps {
  left: ReactNode;
  right: ReactNode;
  /** Optional ratio for left panel (default 50 = 50/50 split) */
  leftPercent?: number;
}

export default function SplitView({ left, right, leftPercent = 50 }: SplitViewProps) {
  return (
    <div className="flex h-full w-full overflow-hidden">
      <div
        className="h-full overflow-y-auto border-r border-zinc-800"
        style={{ width: `${leftPercent}%` }}
      >
        {left}
      </div>
      <div
        className="h-full overflow-y-auto"
        style={{ width: `${100 - leftPercent}%` }}
      >
        {right}
      </div>
    </div>
  );
}

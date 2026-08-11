import type { ReactNode } from "react";

export function DrawerHeader({
  title,
  actions,
  collapsed = false,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  collapsed?: boolean;
}) {
  return (
    <div className={`xc-drawer-header ${collapsed ? "xc-drawer-header-compact" : ""}`}>
      {!collapsed && <span className="xc-panel-title min-w-0 flex-1">{title}</span>}
      {actions ? <div className="flex shrink-0 items-center gap-1">{actions}</div> : null}
    </div>
  );
}

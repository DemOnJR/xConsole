import {
  BotIcon,
  PanelBottomIcon,
  PanelLeftIcon,
  PanelRightIcon,
  SettingsIcon,
} from "./icons";
import { useUiStore } from "../stores/uiStore";

function ToggleBtn({
  active,
  title,
  onClick,
  children,
}: {
  active: boolean;
  title: string;
  onClick: () => void;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      title={title}
      onClick={onClick}
      className={`rounded-md p-1.5 transition ${
        active
          ? "bg-[#1f2737] text-gray-100"
          : "text-gray-500 hover:bg-[#151b26] hover:text-gray-300"
      }`}
    >
      {children}
    </button>
  );
}

export function AppToolbar() {
  const leftOpen = useUiStore((s) => s.leftOpen);
  const rightOpen = useUiStore((s) => s.rightOpen);
  const bottomOpen = useUiStore((s) => s.bottomOpen);
  const agentOpen = useUiStore((s) => s.agentOpen);
  const toggleLeft = useUiStore((s) => s.toggleLeft);
  const toggleRight = useUiStore((s) => s.toggleRight);
  const toggleBottom = useUiStore((s) => s.toggleBottom);
  const toggleAgent = useUiStore((s) => s.toggleAgent);
  const openSettings = useUiStore((s) => s.openSettings);

  return (
    <header className="flex h-9 shrink-0 items-center border-b border-[#1f2737] bg-[#0d121b] px-2">
      <div className="flex items-center gap-2 text-xs font-semibold tracking-wide text-gray-300">
        xConsole
      </div>

      <div className="ml-auto flex items-center gap-0.5">
        <ToggleBtn
          active={leftOpen}
          title={leftOpen ? "Hide workspaces" : "Show workspaces"}
          onClick={toggleLeft}
        >
          <PanelLeftIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={bottomOpen}
          title={bottomOpen ? "Hide console" : "Show console"}
          onClick={toggleBottom}
        >
          <PanelBottomIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={agentOpen}
          title={agentOpen ? "Hide agent" : "Show agent"}
          onClick={toggleAgent}
        >
          <BotIcon size={16} />
        </ToggleBtn>
        <ToggleBtn
          active={rightOpen}
          title={rightOpen ? "Hide servers" : "Show servers"}
          onClick={toggleRight}
        >
          <PanelRightIcon size={16} />
        </ToggleBtn>

        <div className="mx-1 h-4 w-px bg-[#1f2737]" />

        <ToggleBtn active={false} title="Settings" onClick={() => openSettings()}>
          <SettingsIcon size={16} />
        </ToggleBtn>
      </div>
    </header>
  );
}

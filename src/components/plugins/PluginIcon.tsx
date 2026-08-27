import {
  ChartIcon,
  BotIcon,
  FolderIcon,
  DatabaseIcon,
  CloudIcon,
  CpuIcon,
  ServerIcon,
  ContainerIcon,
  GlobeIcon,
  ZapIcon,
  LayersIcon,
  PuzzleIcon,
  ShieldIcon,
  TerminalIcon,
  SettingsIcon,
} from "../icons";

interface PluginIconProps {
  icon?: string;
  pluginId?: string;
  size?: number;
  className?: string;
}

/**
 * Resolves a plugin icon name, id, or category to a clean, crisp SVG component.
 * Guaranteed to NEVER render emojis or raw symbol identifiers.
 */
export function PluginIcon({
  icon,
  pluginId = "",
  size = 18,
  className = "",
}: PluginIconProps) {
  const normIcon = (icon || "").trim().toLowerCase();
  const normId = (pluginId || "").trim().toLowerCase();

  // 1. Check icon name match
  if (
    normIcon === "charticon" ||
    normIcon === "chart" ||
    normIcon === "analytics" ||
    normIcon === "telemetry" ||
    normIcon === "metrics"
  ) {
    return <ChartIcon size={size} className={className} />;
  }

  if (
    normIcon === "boticon" ||
    normIcon === "bot" ||
    normIcon === "agent" ||
    normIcon === "ai" ||
    normIcon === "robot"
  ) {
    return <BotIcon size={size} className={className} />;
  }

  if (
    normIcon === "foldericon" ||
    normIcon === "folder" ||
    normIcon === "sftp" ||
    normIcon === "files" ||
    normIcon === "file"
  ) {
    return <FolderIcon size={size} className={className} />;
  }

  if (
    normIcon === "databaseicon" ||
    normIcon === "database" ||
    normIcon === "db" ||
    normIcon === "sql" ||
    normIcon === "mysql" ||
    normIcon === "sqlite" ||
    normIcon === "postgres" ||
    normIcon === "cache" ||
    normIcon === "redis"
  ) {
    return <DatabaseIcon size={size} className={className} />;
  }

  if (
    normIcon === "cloudicon" ||
    normIcon === "cloud" ||
    normIcon === "cloudflare" ||
    normIcon === "tunnel" ||
    normIcon === "dns"
  ) {
    return <CloudIcon size={size} className={className} />;
  }

  if (
    normIcon === "cpuicon" ||
    normIcon === "cpu" ||
    normIcon === "chip" ||
    normIcon === "system" ||
    normIcon === "hardware"
  ) {
    return <CpuIcon size={size} className={className} />;
  }

  if (
    normIcon === "servericon" ||
    normIcon === "server" ||
    normIcon === "vps" ||
    normIcon === "node" ||
    normIcon === "infra"
  ) {
    return <ServerIcon size={size} className={className} />;
  }

  if (
    normIcon === "containericon" ||
    normIcon === "container" ||
    normIcon === "docker" ||
    normIcon === "k8s" ||
    normIcon === "pod"
  ) {
    return <ContainerIcon size={size} className={className} />;
  }

  if (
    normIcon === "globeicon" ||
    normIcon === "globe" ||
    normIcon === "nginx" ||
    normIcon === "network" ||
    normIcon === "web" ||
    normIcon === "ssl"
  ) {
    return <GlobeIcon size={size} className={className} />;
  }

  if (
    normIcon === "zapicon" ||
    normIcon === "zap" ||
    normIcon === "bolt" ||
    normIcon === "lightning" ||
    normIcon === "fast"
  ) {
    return <ZapIcon size={size} className={className} />;
  }

  if (
    normIcon === "shieldicon" ||
    normIcon === "shield" ||
    normIcon === "security" ||
    normIcon === "auth" ||
    normIcon === "lock"
  ) {
    return <ShieldIcon size={size} className={className} />;
  }

  if (
    normIcon === "terminalicon" ||
    normIcon === "terminal" ||
    normIcon === "cli" ||
    normIcon === "shell" ||
    normIcon === "cmd"
  ) {
    return <TerminalIcon size={size} className={className} />;
  }

  if (
    normIcon === "settingsicon" ||
    normIcon === "settings" ||
    normIcon === "config" ||
    normIcon === "preferences"
  ) {
    return <SettingsIcon size={size} className={className} />;
  }

  if (
    normIcon === "layersicon" ||
    normIcon === "layers" ||
    normIcon === "extension"
  ) {
    return <LayersIcon size={size} className={className} />;
  }

  // 2. Fallback to plugin ID heuristics
  if (normId.includes("analytics") || normId.includes("telemetry") || normId.includes("metric")) {
    return <ChartIcon size={size} className={className} />;
  }
  if (normId.includes("agent") || normId.includes("ai") || normId.includes("bot")) {
    return <BotIcon size={size} className={className} />;
  }
  if (normId.includes("sftp") || normId.includes("file")) {
    return <FolderIcon size={size} className={className} />;
  }
  if (normId.includes("database") || normId.includes("db") || normId.includes("redis") || normId.includes("sql")) {
    return <DatabaseIcon size={size} className={className} />;
  }
  if (normId.includes("cloudflare") || normId.includes("tunnel") || normId.includes("dns")) {
    return <CloudIcon size={size} className={className} />;
  }
  if (normId.includes("docker") || normId.includes("container")) {
    return <ContainerIcon size={size} className={className} />;
  }
  if (normId.includes("nginx") || normId.includes("proxy") || normId.includes("ssl")) {
    return <GlobeIcon size={size} className={className} />;
  }

  // 3. Ultimate clean fallback: Puzzle icon
  return <PuzzleIcon size={size} className={className} />;
}

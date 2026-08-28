import { invoke } from "@tauri-apps/api/core";
import type { AgentAnalytics, ResourceSnapshot, Vps } from "./types";

async function tauriInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (err) {
    const win = typeof window !== "undefined" ? (window as any) : {};
    if (win.__TAURI_INTERNALS__?.invoke) {
      return await win.__TAURI_INTERNALS__.invoke(cmd, args);
    }
    if (win.__TAURI__?.core?.invoke) {
      return await win.__TAURI__.core.invoke(cmd, args);
    }
    throw err;
  }
}

export const api = {
  agentAnalytics: () => tauriInvoke<AgentAnalytics>("agent_analytics"),
  appResourceSnapshot: () => tauriInvoke<ResourceSnapshot>("app_resource_snapshot"),
  listVps: () => tauriInvoke<Vps[]>("list_vps_cmd"),
};

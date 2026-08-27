import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import type {
  CloudAccount,
  CloudflareAuditLog,
  CloudflareDnsRecord,
  CloudflareDnsRecordInput,
  CloudflareSecuritySettings,
  CloudflareTunnel,
  CloudflareTunnelConfig,
  CloudflareZone,
} from "./types";

interface CloudflareState {
  accounts: CloudAccount[];
  selectedAccountId: string | null;
  zones: CloudflareZone[];
  selectedZoneId: string | null;
  tunnels: CloudflareTunnel[];
  selectedTunnel: CloudflareTunnel | null;
  tunnelConfig: CloudflareTunnelConfig | null;
  tunnelToken: string | null;
  dnsRecords: CloudflareDnsRecord[];
  securitySettings: CloudflareSecuritySettings | null;
  history: CloudflareAuditLog[];
  loading: boolean;
  error: string | null;

  // Actions
  loadAccounts: () => Promise<void>;
  selectAccount: (accountId: string) => Promise<void>;
  selectZone: (zoneId: string) => Promise<void>;
  loadZones: (accountId?: string) => Promise<void>;
  loadTunnels: (accountId?: string) => Promise<void>;
  selectTunnel: (tunnel: CloudflareTunnel | null) => Promise<void>;
  createTunnel: (name: string) => Promise<CloudflareTunnel>;
  deleteTunnel: (tunnelId: string) => Promise<void>;
  saveTunnelConfig: (config: CloudflareTunnelConfig) => Promise<void>;
  loadDnsRecords: (zoneId?: string) => Promise<void>;
  upsertDnsRecord: (record: CloudflareDnsRecordInput) => Promise<void>;
  deleteDnsRecord: (recordId: string) => Promise<void>;
  loadSecuritySettings: (zoneId?: string) => Promise<void>;
  setSecurityLevel: (level: string) => Promise<void>;
  toggleUnderAttackMode: () => Promise<void>;
  loadHistory: (accountId?: string) => Promise<void>;
  revertAction: (logId: string) => Promise<string>;
}

export const useCloudflareStore = create<CloudflareState>((set, get) => ({
  accounts: [],
  selectedAccountId: null,
  zones: [],
  selectedZoneId: null,
  tunnels: [],
  selectedTunnel: null,
  tunnelConfig: null,
  tunnelToken: null,
  dnsRecords: [],
  securitySettings: null,
  history: [],
  loading: false,
  error: null,

  loadAccounts: async () => {
    try {
      const allAccounts = await invoke<CloudAccount[]>("list_cloud_accounts");
      const cfAccounts = allAccounts.filter((a) => a.kind === "cloudflare");
      set({ accounts: cfAccounts });
      if (cfAccounts.length > 0 && !get().selectedAccountId) {
        await get().selectAccount(cfAccounts[0].id);
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  selectAccount: async (accountId: string) => {
    set({ selectedAccountId: accountId, selectedTunnel: null, tunnelConfig: null });
    await Promise.all([
      get().loadZones(accountId),
      get().loadTunnels(accountId),
      get().loadHistory(accountId),
    ]);
  },

  selectZone: async (zoneId: string) => {
    set({ selectedZoneId: zoneId });
    await Promise.all([get().loadDnsRecords(zoneId), get().loadSecuritySettings(zoneId)]);
  },

  loadZones: async (accountId?: string) => {
    const accId = accountId || get().selectedAccountId;
    if (!accId) return;
    set({ loading: true, error: null });
    try {
      const zones = await invoke<CloudflareZone[]>("list_cloudflare_zones", { accountId: accId });
      set({ zones, loading: false });
      if (zones.length > 0 && !get().selectedZoneId) {
        await get().selectZone(zones[0].id);
      }
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  loadTunnels: async (accountId?: string) => {
    const accId = accountId || get().selectedAccountId;
    if (!accId) return;
    try {
      const tunnels = await invoke<CloudflareTunnel[]>("list_cloudflare_tunnels", { accountId: accId });
      set({ tunnels });
      if (tunnels.length > 0 && !get().selectedTunnel) {
        await get().selectTunnel(tunnels[0]);
      }
    } catch (e) {
      set({ error: String(e) });
    }
  },

  selectTunnel: async (tunnel: CloudflareTunnel | null) => {
    set({ selectedTunnel: tunnel, tunnelConfig: null, tunnelToken: null });
    if (!tunnel) return;
    const accId = get().selectedAccountId;
    if (!accId) return;
    try {
      const [config, token] = await Promise.all([
        invoke<CloudflareTunnelConfig>("get_cloudflare_tunnel_config", { accountId: accId, tunnelId: tunnel.id }),
        invoke<string>("get_cloudflare_tunnel_token", { accountId: accId, tunnelId: tunnel.id }).catch(() => null),
      ]);
      set({ tunnelConfig: config, tunnelToken: token });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  createTunnel: async (name: string) => {
    const accId = get().selectedAccountId;
    if (!accId) throw new Error("Niciun cont Cloudflare selectat");
    const tunnel = await invoke<CloudflareTunnel>("create_cloudflare_tunnel", { accountId: accId, name });
    await get().loadTunnels(accId);
    await get().selectTunnel(tunnel);
    return tunnel;
  },

  deleteTunnel: async (tunnelId: string) => {
    const accId = get().selectedAccountId;
    if (!accId) return;
    await invoke<void>("delete_cloudflare_tunnel", { accountId: accId, tunnelId });
    if (get().selectedTunnel?.id === tunnelId) {
      set({ selectedTunnel: null, tunnelConfig: null });
    }
    await get().loadTunnels(accId);
  },

  saveTunnelConfig: async (config: CloudflareTunnelConfig) => {
    const accId = get().selectedAccountId;
    const tunnel = get().selectedTunnel;
    if (!accId || !tunnel) return;
    await invoke<void>("save_cloudflare_tunnel_config", { accountId: accId, tunnelId: tunnel.id, config });
    set({ tunnelConfig: config });
  },

  loadDnsRecords: async (zoneId?: string) => {
    const accId = get().selectedAccountId;
    const zId = zoneId || get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      const dnsRecords = await invoke<CloudflareDnsRecord[]>("list_cloudflare_dns_records", { accountId: accId, zoneId: zId });
      set({ dnsRecords });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  upsertDnsRecord: async (record: CloudflareDnsRecordInput) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) return;
    await invoke<CloudflareDnsRecord>("upsert_cloudflare_dns_record", { accountId: accId, zoneId: zId, record });
    await get().loadDnsRecords(zId);
  },

  deleteDnsRecord: async (recordId: string) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) return;
    await invoke<void>("delete_cloudflare_dns_record", { accountId: accId, zoneId: zId, recordId });
    await get().loadDnsRecords(zId);
  },

  loadSecuritySettings: async (zoneId?: string) => {
    const accId = get().selectedAccountId;
    const zId = zoneId || get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      const securitySettings = await invoke<CloudflareSecuritySettings>("get_cloudflare_security_settings", { accountId: accId, zoneId: zId });
      set({ securitySettings });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setSecurityLevel: async (level: string) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) return;
    await invoke<string>("set_cloudflare_security_level", { accountId: accId, zoneId: zId, level });
    await get().loadSecuritySettings(zId);
  },

  toggleUnderAttackMode: async () => {
    const current = get().securitySettings?.under_attack_mode ?? false;
    const nextLevel = current ? "medium" : "under_attack";
    await get().setSecurityLevel(nextLevel);
  },

  loadHistory: async (accountId?: string) => {
    const accId = accountId || get().selectedAccountId;
    if (!accId) return;
    try {
      const history = await invoke<CloudflareAuditLog[]>("list_cloudflare_history", { accountId: accId });
      set({ history });
    } catch (e) {
      console.warn("Failed to load Cloudflare audit log history", e);
    }
  },

  revertAction: async (logId: string) => {
    const accId = get().selectedAccountId;
    if (!accId) throw new Error("No active Cloudflare account");
    const result = await invoke<string>("revert_cloudflare_action", { accountId: accId, logId });
    await Promise.all([
      get().loadHistory(accId),
      get().loadDnsRecords(),
      get().loadSecuritySettings(),
    ]);
    return result;
  },
}));

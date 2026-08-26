import { create } from "zustand";
import {
  api,
  type CloudAccount,
  type CloudflareDnsRecord,
  type CloudflareDnsRecordInput,
  type CloudflareSecuritySettings,
  type CloudflareTunnel,
  type CloudflareTunnelConfig,
  type CloudflareZone,
} from "../lib/tauri";

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
  loading: false,
  error: null,

  loadAccounts: async () => {
    try {
      const allAccounts = await api.listCloudAccounts();
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
    await Promise.all([get().loadZones(accountId), get().loadTunnels(accountId)]);
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
      const zones = await api.listCloudflareZones(accId);
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
    set({ loading: true, error: null });
    try {
      const tunnels = await api.listCloudflareTunnels(accId);
      set({ tunnels, loading: false });
    } catch (e) {
      set({ error: String(e), loading: false });
    }
  },

  selectTunnel: async (tunnel: CloudflareTunnel | null) => {
    set({ selectedTunnel: tunnel, tunnelConfig: null, tunnelToken: null });
    const accId = get().selectedAccountId;
    if (!tunnel || !accId) return;
    try {
      const [config, token] = await Promise.all([
        api.getCloudflareTunnelConfig(accId, tunnel.id).catch(() => ({ ingress: [] })),
        api.getCloudflareTunnelToken(accId, tunnel.id).catch(() => ""),
      ]);
      set({ tunnelConfig: config, tunnelToken: token });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  createTunnel: async (name: string) => {
    const accId = get().selectedAccountId;
    if (!accId) throw new Error("No active Cloudflare account selected");
    set({ loading: true, error: null });
    try {
      const created = await api.createCloudflareTunnel(accId, name);
      await get().loadTunnels(accId);
      await get().selectTunnel(created);
      set({ loading: false });
      return created;
    } catch (e) {
      set({ error: String(e), loading: false });
      throw e;
    }
  },

  deleteTunnel: async (tunnelId: string) => {
    const accId = get().selectedAccountId;
    if (!accId) return;
    try {
      await api.deleteCloudflareTunnel(accId, tunnelId);
      if (get().selectedTunnel?.id === tunnelId) {
        set({ selectedTunnel: null, tunnelConfig: null, tunnelToken: null });
      }
      await get().loadTunnels(accId);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  saveTunnelConfig: async (config: CloudflareTunnelConfig) => {
    const accId = get().selectedAccountId;
    const tunnel = get().selectedTunnel;
    if (!accId || !tunnel) return;
    try {
      const saved = await api.saveCloudflareTunnelConfig(accId, tunnel.id, config);
      set({ tunnelConfig: saved });
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  loadDnsRecords: async (zoneId?: string) => {
    const accId = get().selectedAccountId;
    const zId = zoneId || get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      const dnsRecords = await api.listCloudflareDnsRecords(accId, zId);
      set({ dnsRecords });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  upsertDnsRecord: async (record: CloudflareDnsRecordInput) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) throw new Error("No active zone selected");
    try {
      await api.upsertCloudflareDnsRecord(accId, zId, record);
      await get().loadDnsRecords(zId);
    } catch (e) {
      set({ error: String(e) });
      throw e;
    }
  },

  deleteDnsRecord: async (recordId: string) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      await api.deleteCloudflareDnsRecord(accId, zId, recordId);
      await get().loadDnsRecords(zId);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  loadSecuritySettings: async (zoneId?: string) => {
    const accId = get().selectedAccountId;
    const zId = zoneId || get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      const securitySettings = await api.getCloudflareSecuritySettings(accId, zId);
      set({ securitySettings });
    } catch (e) {
      set({ error: String(e) });
    }
  },

  setSecurityLevel: async (level: string) => {
    const accId = get().selectedAccountId;
    const zId = get().selectedZoneId;
    if (!accId || !zId) return;
    try {
      await api.setCloudflareSecurityLevel(accId, zId, level);
      await get().loadSecuritySettings(zId);
    } catch (e) {
      set({ error: String(e) });
    }
  },

  toggleUnderAttackMode: async () => {
    const current = get().securitySettings;
    const nextLevel = current?.attack_mode ? "medium" : "under_attack";
    await get().setSecurityLevel(nextLevel);
  },
}));

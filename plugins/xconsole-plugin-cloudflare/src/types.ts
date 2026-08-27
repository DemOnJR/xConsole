export interface CloudAccount {
  id: string;
  kind: string;
  name: string;
  region?: string | null;
  project_id?: string | null;
  created_at: string;
}

export interface CloudflareZone {
  id: string;
  name: string;
  status: string;
  paused: boolean;
  name_servers?: string[];
}

export interface CloudflareTunnel {
  id: string;
  name: string;
  created_at?: string;
  status?: string;
}

export interface CloudflareIngressRule {
  hostname?: string;
  path?: string;
  service: string;
}

export interface CloudflareTunnelConfig {
  ingress: CloudflareIngressRule[];
}

export interface CloudflareDnsRecord {
  id: string;
  zone_id: string;
  name: string;
  type: string;
  content: string;
  proxiable: boolean;
  proxied: boolean;
  ttl: number;
}

export interface CloudflareDnsRecordInput {
  id?: string;
  name: string;
  type: string;
  content: string;
  proxied: boolean;
  ttl?: number;
}

export interface CloudflareSecuritySettings {
  security_level: string;
  waf_status?: string;
  under_attack_mode: boolean;
}

export interface CloudflareAuditLog {
  id: string;
  account_id: string;
  action_type: string;
  target_id?: string | null;
  target_name?: string | null;
  summary: string;
  actor: string;
  session_id?: string | null;
  before_state?: string | null;
  after_state?: string | null;
  reverted: boolean;
  created_at: string;
}

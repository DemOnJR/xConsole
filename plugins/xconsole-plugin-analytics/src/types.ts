export interface ResourceSnapshot {
  ts: string;
  cpu_pct: number;
  ram_mb: number;
  ram_total_mb: number;
  process_ram_mb: number;
  gpu_pct: number | null;
  gpu_mem_mb: number | null;
  gpu_mem_total_mb: number | null;
  gpu_name: string | null;
}

export interface CachePoint {
  ts: string;
  session: string;
  prompt: number;
  hit: number;
  miss: number;
  /** Tokens written to the provider cache. Billed at 1.25x (5m) / 2.0x (1h) of input. */
  written: number;
  pct: number;
  model: string;
  provider: string;
  /** "5m" or "1h". */
  ttl: string;
}

export interface ToolCount {
  name: string;
  count: number;
}

export interface ConversationStat {
  id: string;
  title: string;
  updated_at: string;
  user_turns: number;
  tool_calls: number;
  tools: ToolCount[];
}

export interface AgentAnalytics {
  cache: CachePoint[];
  /** Token-weighted: sum(hit) / sum(hit + miss + written). */
  cache_avg_pct: number;
  cache_hit_tokens: number;
  cache_miss_tokens: number;
  cache_written_tokens: number;
  conversations: ConversationStat[];
  tools_all: ToolCount[];
  resource: ResourceSnapshot;
}

export interface Vps {
  id: string;
  name: string;
  host: string;
  port: number;
  username: string;
  auth_type?: string;
  key_path?: string | null;
  tags?: string | null;
  created_at?: string | null;
}

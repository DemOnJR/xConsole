import type { ProviderKind } from "./tauri";

/**
 * Curated AI provider catalog (deduped union of rick-cli/rick, opencode/models.dev,
 * pi.dev, and Command Code), sorted A-Z for the picker.
 *
 * Most entries are OpenAI-compatible and map to the existing `openai` kind with a
 * per-provider base URL (exactly how rick/opencode wire them) — no new backend kind
 * needed. A few are Anthropic-format, local, or CLI tools.
 */

export interface CatalogProvider {
  /** Stable slug (also the id used when adding). */
  id: string;
  /** Display name, e.g. "DeepSeek". */
  name: string;
  /** Backend kind this provider maps to. */
  kind: ProviderKind;
  /** Wire protocol for the live /models probe. */
  flavor: "openai" | "anthropic" | "local" | "cli";
  /** Default API base URL (or bin path for CLI). */
  baseUrl: string;
  /** Fallback default model. */
  defaultModel: string;
  /** Curated model set — used when the live probe fails or has no endpoint. */
  models: string[];
  /** Whether an API key is required. */
  needsKey: boolean;
  /** CLI binary name for CLI kinds. */
  binPath?: string;
  /** Group label for the A-Z picker (first letter unless custom). */
  group: string;
}

/** Match a saved provider to its catalog row (kind, id, or name). */
export function catalogForProvider(p: { id?: string; kind: string; name: string }): CatalogProvider | undefined {
  const name = p.name.toLowerCase();
  return PROVIDER_CATALOG.find(
    (c) =>
      c.kind === p.kind ||
      c.id === p.id ||
      c.id === p.kind ||
      c.name.toLowerCase() === name,
  );
}

export const PROVIDER_CATALOG: CatalogProvider[] = [
  // A
  { id: "alibaba", name: "Alibaba (Qwen)", kind: "openai", flavor: "openai", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", defaultModel: "qwen-plus", models: ["qwen-plus", "qwen-max", "qwen-turbo", "qwen3-max", "qwen3-flash"], needsKey: true, group: "A" },
  { id: "amazon-bedrock", name: "Amazon Bedrock", kind: "openai", flavor: "openai", baseUrl: "https://bedrock-runtime.us-east-1.amazonaws.com", defaultModel: "anthropic.claude-sonnet-4-5", models: ["anthropic.claude-sonnet-4-5", "anthropic.claude-opus-4-5", "meta.llama3-70b"], needsKey: true, group: "A" },
  { id: "anthropic", name: "Anthropic (Claude)", kind: "anthropic", flavor: "anthropic", baseUrl: "https://api.anthropic.com", defaultModel: "claude-sonnet-4-5", models: ["claude-sonnet-4-5", "claude-opus-4-5", "claude-haiku-4-5", "claude-3-5-haiku"], needsKey: true, group: "A" },
  { id: "antigravity", name: "Antigravity CLI (agy)", kind: "antigravity_cli", flavor: "cli", baseUrl: "agy", defaultModel: "gemini-3.7-flash-high", models: ["gemini-3.7-flash-high", "gemini-3.7-flash-medium", "gemini-3.7-flash-low", "gemini-3.6-flash-high", "gemini-3.6-flash-medium", "gemini-3.6-flash-low", "gemini-3.5-flash-high", "gemini-3.5-flash-medium", "gemini-3.5-flash-low", "gemini-3.1-pro-high", "gemini-3.1-pro-low", "claude-sonnet-4-6", "claude-opus-4-6-thinking", "gpt-oss-120b-medium"], needsKey: false, binPath: "agy", group: "A" },
  { id: "arcee", name: "Arcee AI", kind: "openai", flavor: "openai", baseUrl: "https://api.arcee.ai/v1", defaultModel: "arcee-lite", models: ["arcee-lite", "arcee-mini"], needsKey: true, group: "A" },
  { id: "azure", name: "Azure OpenAI", kind: "openai", flavor: "openai", baseUrl: "https://YOUR_RESOURCE.openai.azure.com/openai/v1", defaultModel: "gpt-4o", models: ["gpt-4o", "gpt-4o-mini", "gpt-5"], needsKey: true, group: "A" },

  // B
  { id: "baseten", name: "Baseten", kind: "openai", flavor: "openai", baseUrl: "https://bridge.baseten.co/v1", defaultModel: "llama-3-70b", models: ["llama-3-70b", "mistral-7b"], needsKey: true, group: "B" },

  // C
  { id: "cerebras", name: "Cerebras", kind: "openai", flavor: "openai", baseUrl: "https://api.cerebras.ai/v1", defaultModel: "llama-3.3-70b", models: ["llama-3.3-70b", "llama-3.1-8b"], needsKey: true, group: "C" },
  { id: "codex", name: "Codex CLI", kind: "codex_cli", flavor: "cli", baseUrl: "codex", defaultModel: "", models: [], needsKey: false, binPath: "codex", group: "C" },
  { id: "cohere", name: "Cohere", kind: "openai", flavor: "openai", baseUrl: "https://api.cohere.ai/compatibility/v1", defaultModel: "command-r-plus", models: ["command-r-plus", "command-r", "command-a"], needsKey: true, group: "C" },
  {
    id: "command-code",
    name: "Command Code",
    kind: "openai",
    flavor: "openai",
    baseUrl: "https://api.commandcode.ai/provider/v1",
    defaultModel: "deepseek/deepseek-v4-flash",
    models: [
      "stealth/ox-alpha",
      "deepseek/deepseek-v4-flash",
      "deepseek/deepseek-v4-pro",
      "claude-sonnet-5",
      "claude-sonnet-4-6",
      "claude-opus-5",
      "claude-opus-4-8",
      "claude-opus-4-7",
      "claude-haiku-4-5-20251001",
      "gpt-5.6-sol",
      "gpt-5.6-terra",
      "gpt-5.6-luna",
      "gpt-5.5",
      "gpt-5.4",
      "gpt-5.3-codex",
      "google/gemini-3.7-flash",
      "google/gemini-3.6-flash",
      "xai/grok-4.6",
      "xai/grok-4.5",
      "moonshotai/Kimi-K3",
      "moonshotai/Kimi-K2.7-Code",
      "Qwen/Qwen3.8-Max",
      "Qwen/Qwen3.7-Plus",
      "zai-org/GLM-5.3",
      "MiniMaxAI/MiniMax-M3",
    ],
    needsKey: true,
    group: "C",
  },
  { id: "cursor", name: "Cursor (Agent CLI)", kind: "cursor", flavor: "cli", baseUrl: "agent", defaultModel: "auto", models: [], needsKey: false, binPath: "agent", group: "C" },

  // D
  { id: "deepseek", name: "DeepSeek", kind: "openai", flavor: "openai", baseUrl: "https://api.deepseek.com/v1", defaultModel: "deepseek-chat", models: ["deepseek-chat", "deepseek-reasoner", "deepseek-v4-flash", "deepseek-v4-pro"], needsKey: true, group: "D" },

  // F
  { id: "fireworks", name: "Fireworks AI", kind: "openai", flavor: "openai", baseUrl: "https://api.fireworks.ai/inference/v1", defaultModel: "accounts/fireworks/models/llama-v3p1-70b-instruct", models: ["accounts/fireworks/models/llama-v3p1-70b-instruct", "accounts/fireworks/models/deepseek-v3"], needsKey: true, group: "F" },

  // G
  { id: "gemini", name: "Google Gemini", kind: "openai", flavor: "openai", baseUrl: "https://generativelanguage.googleapis.com/v1beta/openai", defaultModel: "gemini-2.5-pro", models: ["gemini-2.5-pro", "gemini-2.5-flash", "gemini-3-pro", "gemini-3-flash"], needsKey: true, group: "G" },
  { id: "github-copilot", name: "GitHub Copilot", kind: "openai", flavor: "openai", baseUrl: "https://api.githubcopilot.com/v1", defaultModel: "gpt-5", models: ["gpt-5", "claude-sonnet-4-5"], needsKey: true, group: "G" },
  { id: "gitlab-duo", name: "GitLab Duo", kind: "openai", flavor: "openai", baseUrl: "https://gitlab.com/api/ai", defaultModel: "claude-sonnet-4-5", models: ["claude-sonnet-4-5"], needsKey: true, group: "G" },
  { id: "groq", name: "Groq", kind: "openai", flavor: "openai", baseUrl: "https://api.groq.com/openai/v1", defaultModel: "llama-3.3-70b-versatile", models: ["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768", "deepseek-r1-distill-llama-70b"], needsKey: true, group: "G" },

  // H
  { id: "huggingface", name: "Hugging Face", kind: "openai", flavor: "openai", baseUrl: "https://router.huggingface.co/v1", defaultModel: "meta-llama/Llama-3.3-70B-Instruct", models: ["meta-llama/Llama-3.3-70B-Instruct", "deepseek-ai/DeepSeek-V3"], needsKey: true, group: "H" },

  // K
  {
    id: "kilo",
    name: "Kilo AI",
    kind: "openai",
    flavor: "openai",
    baseUrl: "https://api.kilo.ai/api/gateway",
    defaultModel: "anthropic/claude-sonnet-4.5",
    models: [
      "anthropic/claude-sonnet-4.5",
      "anthropic/claude-opus-4.5",
      "anthropic/claude-haiku-4.5",
      "openai/gpt-5",
      "openai/gpt-4o",
      "deepseek/deepseek-v4",
      "deepseek/deepseek-chat",
      "mistralai/codestral-2508",
      "minimax/minimax-m2.1:free",
      "z-ai/glm-5:free",
    ],
    needsKey: true,
    group: "K",
  },
  { id: "kimi", name: "Kimi (Moonshot)", kind: "openai", flavor: "openai", baseUrl: "https://api.moonshot.cn/v1", defaultModel: "moonshot-v1-8k", models: ["moonshot-v1-8k", "moonshot-v1-32k", "kimi-k2", "kimi-k2.5"], needsKey: true, group: "K" },

  // L
  { id: "lmstudio", name: "LM Studio (local)", kind: "openai", flavor: "local", baseUrl: "http://localhost:1234/v1", defaultModel: "local-model", models: [], needsKey: false, group: "L" },

  // M
  { id: "minimax", name: "MiniMax", kind: "anthropic", flavor: "anthropic", baseUrl: "https://api.minimax.io/anthropic", defaultModel: "MiniMax-M2", models: ["MiniMax-M2", "MiniMax-M2.5", "MiniMax-M3"], needsKey: true, group: "M" },
  { id: "mistral", name: "Mistral", kind: "openai", flavor: "openai", baseUrl: "https://api.mistral.ai/v1", defaultModel: "mistral-large-latest", models: ["mistral-large-latest", "mistral-small-latest", "codestral-latest", "ministral-8b"], needsKey: true, group: "M" },

  // N
  { id: "nous", name: "Nous Research", kind: "openai", flavor: "openai", baseUrl: "https://api.nousresearch.com/v1", defaultModel: "hermes-4", models: ["hermes-4"], needsKey: true, group: "N" },
  { id: "nvidia", name: "NVIDIA NIM", kind: "openai", flavor: "openai", baseUrl: "https://integrate.api.nvidia.com/v1", defaultModel: "meta/llama-3.3-70b-instruct", models: ["meta/llama-3.3-70b-instruct", "deepseek-ai/deepseek-r1"], needsKey: true, group: "N" },

  // O
  { id: "ollama", name: "Ollama (local)", kind: "ollama", flavor: "local", baseUrl: "http://localhost:11434", defaultModel: "qwen3.5:9b", models: [], needsKey: false, group: "O" },
  { id: "opencode", name: "OpenCode CLI", kind: "opencode_cli", flavor: "cli", baseUrl: "opencode", defaultModel: "", models: [], needsKey: false, binPath: "opencode", group: "O" },
  { id: "opencode-go", name: "OpenCode Go", kind: "openai", flavor: "openai", baseUrl: "https://opencode.ai/go", defaultModel: "deepseek-v4-flash", models: ["deepseek-v4-flash", "deepseek-v4-flash-free"], needsKey: true, group: "O" },
  { id: "openrouter", name: "OpenRouter", kind: "openai", flavor: "openai", baseUrl: "https://openrouter.ai/api/v1", defaultModel: "stealth/ox-alpha", models: ["stealth/ox-alpha", "anthropic/claude-sonnet-4.5", "openai/gpt-5", "deepseek/deepseek-v4", "meta-llama/llama-3.3-70b-instruct"], needsKey: true, group: "O" },
  { id: "openai", name: "OpenAI", kind: "openai", flavor: "openai", baseUrl: "https://api.openai.com/v1", defaultModel: "gpt-5", models: ["gpt-5", "gpt-5-mini", "gpt-4o", "gpt-4o-mini", "gpt-4.1", "o3"], needsKey: true, group: "O" },

  // P
  { id: "perplexity", name: "Perplexity", kind: "openai", flavor: "openai", baseUrl: "https://api.perplexity.ai", defaultModel: "sonar-pro", models: ["sonar-pro", "sonar", "sonar-reasoning"], needsKey: true, group: "P" },
  { id: "poolside", name: "Poolside", kind: "openai", flavor: "openai", baseUrl: "https://api.poolside.ai/v1", defaultModel: "laguna-s-2.1", models: ["laguna-s-2.1"], needsKey: true, group: "P" },

  // Q
  { id: "qwen", name: "Qwen (Alibaba Cloud)", kind: "openai", flavor: "openai", baseUrl: "https://dashscope.aliyuncs.com/compatible-mode/v1", defaultModel: "qwen-max", models: ["qwen-max", "qwen-plus", "qwen3-max", "qwen3-flash"], needsKey: true, group: "Q" },

  // S
  { id: "sakana", name: "Sakana AI", kind: "openai", flavor: "openai", baseUrl: "https://api.sakana.ai/v1", defaultModel: "fugu-ultra", models: ["fugu-ultra", "fugu-small"], needsKey: true, group: "S" },
  { id: "siliconflow", name: "SiliconFlow", kind: "openai", flavor: "openai", baseUrl: "https://api.siliconflow.cn/v1", defaultModel: "Qwen/Qwen3-32B", models: ["Qwen/Qwen3-32B", "deepseek-ai/DeepSeek-V3"], needsKey: true, group: "S" },
  { id: "stepfun", name: "StepFun", kind: "openai", flavor: "openai", baseUrl: "https://api.stepfun.com/v1", defaultModel: "step-2-16k", models: ["step-2-16k", "step-1-8k"], needsKey: true, group: "S" },

  // T
  { id: "tencent", name: "Tencent (Hunyuan)", kind: "openai", flavor: "openai", baseUrl: "https://api.hunyuan.cloud.tencent.com/v1", defaultModel: "hunyuan-turbo", models: ["hunyuan-turbo", "hunyuan-pro", "hy3"], needsKey: true, group: "T" },
  { id: "thinkingmachines", name: "Thinking Machines", kind: "anthropic", flavor: "anthropic", baseUrl: "https://api.thinkingmachines.ai", defaultModel: "inkling", models: ["inkling", "inkling-small"], needsKey: true, group: "T" },
  { id: "together", name: "Together AI", kind: "openai", flavor: "openai", baseUrl: "https://api.together.xyz/v1", defaultModel: "meta-llama/Llama-3.3-70B-Instruct-Turbo", models: ["meta-llama/Llama-3.3-70B-Instruct-Turbo", "deepseek-ai/DeepSeek-V3", "Qwen/Qwen3-32B"], needsKey: true, group: "T" },

  // X
  { id: "xai", name: "xAI (Grok)", kind: "openai", flavor: "openai", baseUrl: "https://api.x.ai/v1", defaultModel: "grok-4", models: ["grok-4", "grok-4-fast", "grok-3", "grok-3-mini"], needsKey: true, group: "X" },
  { id: "xiaomi", name: "Xiaomi MiMo", kind: "openai", flavor: "openai", baseUrl: "https://api.xiaomi.com/v1", defaultModel: "mimo-v2.5-pro", models: ["mimo-v2.5-pro", "mimo-v2.5"], needsKey: true, group: "X" },

  // Z
  { id: "zai", name: "Z.AI (GLM)", kind: "openai", flavor: "openai", baseUrl: "https://api.z.ai/api/paas/v4", defaultModel: "glm-5", models: ["glm-5", "glm-5.2", "glm-4.5", "glm-4.6"], needsKey: true, group: "Z" },
  { id: "zhipu", name: "Zhipu AI", kind: "openai", flavor: "openai", baseUrl: "https://open.bigmodel.cn/api/paas/v4", defaultModel: "glm-4.5", models: ["glm-4.5", "glm-4", "glm-4-flash"], needsKey: true, group: "Z" },
];

/** Catalog grouped alphabetically by first letter (A-Z). */
export function catalogGroups(): { letter: string; providers: CatalogProvider[] }[] {
  const map = new Map<string, CatalogProvider[]>();
  for (const p of PROVIDER_CATALOG) {
    const list = map.get(p.group) ?? [];
    list.push(p);
    map.set(p.group, list);
  }
  return [...map.entries()]
    .sort((a, b) => a[0].localeCompare(b[0]))
    .map(([letter, providers]) => ({ letter, providers }));
}

/** Approximate per-1M token pricing for display in model pickers and settings. */
export function priceForModel(modelId: string): { input: number; output: number; cacheRead: number } {
  const m = modelId.toLowerCase();
  if (m.includes(":free")) return { input: 0.0, output: 0.0, cacheRead: 0.0 };
  if (m.includes("ox-alpha") || m.includes("0x-alpha") || m.includes("stealth")) return { input: 0.0, output: 0.0, cacheRead: 0.0 };
  if (m.includes("opus")) return { input: 15.0, output: 75.0, cacheRead: 1.5 };
  if (m.includes("sonnet") || m.includes("3-7-sonnet") || m.includes("3.7-sonnet")) return { input: 3.0, output: 15.0, cacheRead: 0.3 };
  if (m.includes("haiku")) return { input: 0.8, output: 4.0, cacheRead: 0.08 };
  if (m.includes("gpt-5.6") || m.includes("gpt-5")) return { input: 1.25, output: 10.0, cacheRead: 0.125 };
  if (m.includes("gpt-4o-mini")) return { input: 0.15, output: 0.6, cacheRead: 0.075 };
  if (m.includes("gpt-4o") || m.includes("gpt-4")) return { input: 2.5, output: 10.0, cacheRead: 1.25 };
  if (m.includes("o3")) return { input: 2.0, output: 8.0, cacheRead: 0.2 };
  if (m.includes("v4-flash") || m.includes("flash") || m.includes("deepseek-chat")) return { input: 0.14, output: 0.28, cacheRead: 0.0028 };
  if (m.includes("v4-pro")) return { input: 0.435, output: 0.87, cacheRead: 0.003625 };
  if (m.includes("reasoner") || m.includes("r1")) return { input: 0.55, output: 2.19, cacheRead: 0.14 };
  if (m.includes("codestral") || m.includes("mistral")) return { input: 0.3, output: 0.9, cacheRead: 0.03 };
  if (m.includes("gemini") && m.includes("pro")) return { input: 1.25, output: 5.0, cacheRead: 0.3125 };
  if (m.includes("gemini")) return { input: 0.15, output: 0.6, cacheRead: 0.0375 };
  if (m.includes("grok")) return { input: 2.0, output: 10.0, cacheRead: 0.5 };
  if (m.includes("qwen") && (m.includes("max") || m.includes("3.8"))) return { input: 0.4, output: 1.2, cacheRead: 0.1 };
  if (m.includes("qwen")) return { input: 0.2, output: 0.6, cacheRead: 0.05 };
  if (m.includes("kimi") || m.includes("moonshot")) return { input: 0.4, output: 1.6, cacheRead: 0.1 };
  if (m.includes("minimax")) return { input: 0.2, output: 0.8, cacheRead: 0.05 };
  if (m.includes("glm")) return { input: 0.5, output: 1.5, cacheRead: 0.1 };
  return { input: 2.0, output: 10.0, cacheRead: 0.2 };
}

/** Fuzzy search over the catalog (name + id), simple subsequence match. */
export function searchCatalog(query: string): CatalogProvider[] {
  const q = query.trim().toLowerCase();
  if (!q) return PROVIDER_CATALOG;
  return PROVIDER_CATALOG.filter((p) => {
    const hay = `${p.name} ${p.id}`.toLowerCase();
    // Subsequence match: all query chars appear in order.
    let i = 0;
    for (const ch of q) {
      const found = hay.indexOf(ch, i);
      if (found === -1) return false;
      i = found + 1;
    }
    return true;
  });
}


import type {
  WorkKeyStateV1,
  WorkModelEntry,
  WorkModelProvider,
  WorkModelRole,
  WorkModelsV1,
} from "$shared/ipc/bindings";

const WIRE = {
  anthropic: "anthropic_messages",
  open_ai: "open_ai_responses",
  google: "gemini",
  deep_seek: "chat_completions",
  open_router: "chat_completions",
  compatible: "chat_completions",
  cloud: "anthropic_messages",
} as const;

const SLUG = {
  anthropic: "anthropic",
  open_ai: "openai",
  google: "google",
  deep_seek: "deepseek",
  open_router: "openrouter",
  compatible: "compatible",
  cloud: "zephium",
} as const;

export function entry(
  provider: WorkModelProvider,
  model: string,
  name: string,
  recommended: boolean,
  roles: WorkModelRole[] = ["lead", "page", "light"],
): WorkModelEntry {
  return {
    id: `${SLUG[provider]}/${model}`,
    model: { provider, wire: WIRE[provider], model },
    display_name: name,
    roles,
    recommended,
    context_window: 1_000_000,
    max_output: 128_000,
    supports: {
      tools: true,
      vision: true,
      prompt_cache: true,
      reasoning: true,
      native_search: true,
    },
    price: { input: 2_000_000, cached_input: 200_000, output: 10_000_000 },
  };
}

const builtin: WorkModelEntry[] = [
  entry("anthropic", "claude-opus-5-5", "Claude Opus 5.5", true, ["lead"]),
  entry("anthropic", "claude-sonnet-5-5", "Claude Sonnet 5.5", true, ["lead", "page"]),
  entry("anthropic", "claude-haiku-4-5-20251001", "Claude Haiku 4.5", true, ["page", "light"]),
  entry("anthropic", "claude-fable-5-1", "Claude Fable 5.1", false, ["lead"]),
  entry("open_ai", "gpt-6-luna", "GPT-6 Luna", true, ["lead", "page", "light"]),
  entry("open_ai", "gpt-6-sol", "GPT-6 Sol", true, ["lead", "page"]),
  entry("open_ai", "gpt-6-astra", "GPT-6 Astra", false, ["lead"]),
  entry("google", "gemini-3.1-pro-preview", "Gemini 3.1 Pro", true, ["lead"]),
  entry("google", "gemini-3.8-flash", "Gemini 3.8 Flash", true, ["lead", "page"]),
  entry("deep_seek", "deepseek-v4-pro", "DeepSeek V4 Pro", true, ["lead"]),
  entry("open_router", "anthropic/claude-sonnet-5.5", "Claude Sonnet 5.5", true, ["lead", "page"]),
];

export const PROFILE = "00000000000000000000000001";

export function models(options: {
  keys?: Partial<Record<WorkModelProvider, WorkKeyStateV1>>;
  cloud?: boolean;
  base?: string;
  lead?: string | null;
  chosen?: string | null;
}): WorkModelsV1 {
  const cloud = options.cloud
    ? [entry("cloud", "claude-sonnet-5-5", "Claude Sonnet 5.5", true, ["lead", "page"])]
    : [];
  return {
    version: 1,
    profile: PROFILE,
    entries: [...cloud, ...builtin],
    chosen: { lead: options.chosen ?? null, page: null, light: null },
    effective: { lead: options.lead ?? null, page: null, light: null },
    providers: (
      ["anthropic", "open_ai", "google", "deep_seek", "open_router", "compatible"] as const
    ).map((provider) => ({
      provider,
      key: options.keys?.[provider] ?? "missing",
      base: provider === "compatible" ? (options.base ?? null) : null,
    })),
    cloud: { signed_in: !!options.cloud, plan: options.cloud ? "Pro" : null },
    fault: null,
  };
}

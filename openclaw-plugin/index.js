import { appendFileSync } from "node:fs";
import { definePluginEntry } from "openclaw/plugin-sdk/plugin-entry";

const DEFAULTS = {
  apiUrl: "http://127.0.0.1:3000/v1/systemone",
  model: "kev-latest",
  tierMinConfidence: 0.75,
  timeoutMs: 2500,
  debugFile: "/tmp/jev-router.log",
};

function log(cfg, line) {
  try {
    appendFileSync(cfg.debugFile, `${new Date().toISOString()} [jev-router] ${line}\n`);
  } catch {
    /* logging must never break a turn */
  }
}

function normaliseTiers(raw) {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return [];
  const out = [];
  for (const [name, value] of Object.entries(raw)) {
    const spec = typeof value === "object" && value !== null ? value : {};
    const model = typeof value === "string" ? value : typeof spec.model === "string" ? spec.model : undefined;
    if (!model) continue;
    const provider = typeof spec.provider === "string" ? spec.provider : undefined;
    const description =
      typeof spec.description === "string" && spec.description.trim()
        ? spec.description
        : `Use the ${name} model`;
    out.push({ name, model, provider, description: description.slice(0, 200) });
  }
  return out;
}

export default definePluginEntry({
  id: "jev-router",
  name: "Jev Router",
  description: "Model-tier routing for agent turns using a local Jev decision server.",
  register(api) {
    const readConfig = () => {
      const raw = api.pluginConfig && typeof api.pluginConfig === "object" ? api.pluginConfig : {};
      const cfg = { ...DEFAULTS, ...raw };
      cfg.tiers = normaliseTiers(raw.tiers);
      return cfg;
    };

    api.on(
      "before_model_resolve",
      async (event) => {
        const cfg = readConfig();
        if (cfg.tiers.length === 0) return undefined;
        const prompt = typeof event?.prompt === "string" ? event.prompt : "";
        if (prompt.trim().length < 24) return undefined;

        const criteria = Object.fromEntries(cfg.tiers.map((tier) => [tier.name, tier.description]));
        const body = JSON.stringify({
          state: prompt.slice(0, 4000),
          model: cfg.model,
          questions: {
            model_tier: {
              type: "choice",
              instructions:
                "Which model tier should execute this step? Pick the cheapest tier that can handle it.",
              criteria,
            },
          },
        });

        try {
          const res = await fetch(cfg.apiUrl, {
            method: "POST",
            headers: { "content-type": "application/json", authorization: "Bearer local" },
            body,
            signal: AbortSignal.timeout(cfg.timeoutMs),
          });
          if (!res.ok) {
            log(cfg, `http ${res.status} — leaving model unchanged`);
            return undefined;
          }
          const data = await res.json();
          const answer = data?.answers?.model_tier;
          if (!answer || answer.type !== "choice" || typeof answer.choice !== "string") {
            log(cfg, "malformed answer — leaving model unchanged");
            return undefined;
          }
          const tier = cfg.tiers.find((candidate) => candidate.name === answer.choice);
          const confidence = Number(answer.confidence);
          if (!tier || !Number.isFinite(confidence)) return undefined;
          if (confidence < cfg.tierMinConfidence) {
            log(cfg, `bypass: ${tier.name}@${confidence.toFixed(3)} < ${cfg.tierMinConfidence}`);
            return undefined;
          }
          log(cfg, `apply: ${tier.name}@${confidence.toFixed(3)} -> ${tier.model}`);
          return {
            modelOverride: tier.model,
            ...(tier.provider ? { providerOverride: tier.provider } : {}),
          };
        } catch (error) {
          log(cfg, `error: ${error?.name ?? "Error"} — leaving model unchanged`);
          return undefined;
        }
      },
      { priority: 50 },
    );
  },
});

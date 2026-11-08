# Sage

**A tier ladder for every model on your machine — so a cheap decision model can pick how much intelligence a step deserves.**

Sage inventories every model you can actually reach (hosted subscription models, free stealth models, local Ollama weights), ranks them by price and context, and emits a ready-made ladder for two routers:

- **Jeff/`jev`** — the OpenCode plugin that decides *which tool* runs next and can rewrite the model per request.
- **`jev-router`** — the OpenClaw plugin that decides *which model* runs a turn, via `before_model_resolve`.

The point is not to use the biggest model everywhere. It is to let a ~150 ms classifier answer "how hard is this step?" and pay accordingly:

```
tier 0 local      → qwen3:8b            $0
tier 1 free       → ox-alpha-free       $0
tier 2 cheap      → glm-5.3-flash       $0.15/M in
tier 3 standard   → gpt-5.6-luna        $0.20/M in
tier 4 strong     → deepseek-v4-pro     $0.66/M in
tier 5 frontier   → qwen3.8-max         $2.00/M in
```

Easy steps stay free or near-free; hard steps climb the ladder automatically.

---

## Why a ladder instead of one "best" model

Cost and capability are not linear. On a real workload most steps are mechanical: read a file, grep a symbol, apply a known edit. Paying frontier prices for those is waste. Occasionally a step is genuinely hard — a multi-file refactor, an ambiguous spec, a debugging dead-end — and a cheap model will flail, costing more in retries than the upgrade would have.

A router that can name the difficulty of a step turns that into a pricing decision instead of a vibe.

## How the tiers are built

Sage derives tiers from the model catalog's own numbers, not from marketing names:

| rule | tier |
|---|---|
| local Ollama weights | 0 `local` |
| hosted, $0 in **and** $0 out | 1 `free` |
| ≤ $0.20/M in **and** ≤ $0.60/M out | 2 `cheap` |
| ≤ $1.00/M in | 3 `standard` |
| < $2.00/M in | 4 `strong` |
| everything above | 5 `frontier` |

Within a tier, models are ordered by `3 × input + output` price — input-heavy work (long context) is weighted accordingly.

Tier descriptions are written for the *router*, not for humans: they describe the work a tier should handle ("multi-file refactors, architecture, ambiguous specs"), because that is what the decision model reads.

## The ranking

Generated from the live catalog on this machine. Prices are USD per million tokens; `ctx` is the context window.

| tier | model | in $/M | out $/M | ctx | tags |
|---|---|---|---|---|---|
| 0 local | `ollama/glm-4.7-flash:latest` | 0 | 0 | — | reasoning, tools, local |
| 0 local | `ollama/qwen2.5-coder:7b` | 0 | 0 | — | reasoning, tools, local |
| 0 local | `ollama/qwen3-coder:latest` | 0 | 0 | — | reasoning, tools, local |
| 0 local | `ollama/qwen3:8b` | 0 | 0 | — | reasoning, tools, local |
| 1 free | `opencode-go/ox-alpha-free` | 0.0 | 0.0 | 1,000,000 | reasoning, tools, vision |
| 2 cheap | `opencode-go/muse-spark-1.2-contributor` | 0.1 | 0.2 | 1,048,576 | reasoning, tools, vision |
| 2 cheap | `opencode-go/muse-spark-1.3-contributor` | 0.1 | 0.2 | 1,048,576 | reasoning, tools, vision |
| 2 cheap | `opencode-go/mimo-v2.5` | 0.14 | 0.28 | 1,000,000 | reasoning, tools, vision |
| 2 cheap | `opencode-go/qwen3.8-flash` | 0.15 | 0.47 | 1,000,000 | reasoning, tools, vision |
| 2 cheap | `opencode-go/glm-5.3-flash` | 0.15 | 0.5 | 1,000,000 | reasoning, tools, vision |
| 2 cheap | `opencode-go/hy3` | 0.14 | 0.58 | 256,000 | reasoning, tools |
| 2 cheap | `opencode-go/deepseek-v4-flash` | 0.15 | 0.6 | 1,000,000 | reasoning, tools |
| 2 cheap | `opencode-go/deepseek-v4-flash-vision-exp` | 0.15 | 0.6 | 1,000,000 | reasoning, tools, vision |
| 2 cheap | `opencode-go/deepseek-v4.1-flash` | 0.15 | 0.6 | 1,000,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/omen-alpha` | 0.2 | 0.66 | 500,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/gpt-5.6-luna` | 0.2 | 1.2 | 1,050,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/qwen3.5-plus` | 0.2 | 1.2 | 262,144 | reasoning, tools, vision |
| 3 standard | `opencode-go/longcat-2.0` | 0.3 | 1.2 | 1,000,000 | reasoning, tools |
| 3 standard | `opencode-go/minimax-m2.5` | 0.3 | 1.2 | 204,800 | reasoning, tools |
| 3 standard | `opencode-go/minimax-m2.7` | 0.3 | 1.2 | 204,800 | reasoning, tools |
| 3 standard | `opencode-go/minimax-m3` | 0.3 | 1.2 | 1,000,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/mimo-v2.5-pro` | 0.435 | 0.87 | 1,048,576 | reasoning, tools |
| 3 standard | `opencode-go/qwen3.7-plus` | 0.4 | 1.6 | 1,000,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/mimo-v2-omni` | 0.4 | 2.0 | 262,144 | reasoning, tools, vision |
| 3 standard | `opencode-go/deepseek-v4-pro` | 0.66 | 1.98 | 1,000,000 | reasoning, tools |
| 3 standard | `opencode-go/qwen3.6-plus` | 0.5 | 3.0 | 1,000,000 | reasoning, tools, vision |
| 3 standard | `opencode-go/kimi-k2.5` | 0.6 | 3.0 | 262,144 | reasoning, tools, vision |
| 3 standard | `opencode-go/hy4-preview` | 0.834 | 2.501 | 1,024,000 | reasoning, tools |
| 3 standard | `opencode-go/mimo-v2-pro` | 1.0 | 3.0 | 1,048,576 | reasoning, tools |
| 3 standard | `opencode-go/glm-5` | 1.0 | 3.2 | 202,752 | reasoning, tools |
| 3 standard | `opencode-go/kimi-k2.6` | 0.95 | 4.0 | 262,144 | reasoning, tools, vision |
| 3 standard | `opencode-go/kimi-k2.7-code` | 0.95 | 4.0 | 262,144 | reasoning, tools, vision |
| 4 strong | `opencode-go/glm-5.1` | 1.4 | 4.4 | 202,752 | reasoning, tools |
| 4 strong | `opencode-go/glm-5.2` | 1.4 | 4.4 | 1,000,000 | reasoning, tools |
| 4 strong | `opencode-go/glm-5.3` | 1.4 | 4.4 | 1,000,000 | reasoning, tools |
| 5 frontier | `opencode-go/grok-4.5` | 2.0 | 6.0 | 500,000 | reasoning, tools, vision |
| 5 frontier | `opencode-go/grok-4.6` | 2.0 | 6.0 | 500,000 | reasoning, tools, vision |
| 5 frontier | `opencode-go/qwen3.8-max` | 2.0 | 6.0 | 1,000,000 | reasoning, tools, vision |
| 5 frontier | `opencode-go/qwen3.7-max` | 2.5 | 7.5 | 1,000,000 | reasoning, tools |
| 5 frontier | `opencode-go/kimi-k3` | 3.0 | 15.0 | 1,048,576 | reasoning, tools, vision |

## Install

```sh
git clone https://github.com/dazemc/sage
cd sage
python3 sage.py            # writes models.json, RANKING.md, tiers.yaml, tiers.openclaw.json
python3 sage.py --table    # just print the ranking
```

No dependencies. It reads `~/.cache/opencode/models.json` (the OpenCode provider cache) and `ollama list`.

## Wire it into a router

### OpenCode (`jev` plugin)

```yaml
# ~/.config/jev/config.yaml
apiKey: local-openjev
apiUrl: http://127.0.0.1:3000/v1/systemone
model: openjev-flash-9b
minConfidence: 0.6        # tool trimming
tierMinConfidence: 0.75   # model switching
tiers:                    # paste from tiers.yaml
  free:
    model: ox-alpha-free
    description: "Simple single-tool work: read one file, answer a short question"
  # ...
```

The OpenCode plugin can only rewrite the `model` field inside one provider's request, so its ladder starts at tier 1. Local Ollama models live on a different provider and cannot be swapped in mid-session.

### OpenClaw (`jev-router` plugin)

```json5
// openclaw.json → plugins.entries.jev-router.config
{
  "apiUrl": "http://127.0.0.1:3000/v1/systemone",
  "tierMinConfidence": 0.65,
  "tiers": { /* paste tiers.openclaw.json → tiers */ }
}
```

`before_model_resolve` may return `providerOverride` as well, so the OpenClaw ladder can start at tier 0 (local Ollama) and cross providers.

## Architecture

```
                    ┌──────────────────────────┐
  opencode ────────▶│ jev plugin  (per request)│──┐
                    │ tool trim + model tier   │  │
                    └──────────────────────────┘  │
                                                  ▼
                    ┌──────────────────────────┐  ┌─────────────────────┐
  OpenClaw ────────▶│ jev-router (per turn)    │─▶│ Jev decision server │
                    │ model tier               │  │ /v1/systemone       │
                    └──────────────────────────┘  └──────────┬──────────┘
                                                             ▼
                                                   OpenJev-Flash-9B (local)
```

One classifier, two consumers, one ladder. The classifier never generates text — it answers a typed choice question ("which tier?") with calibrated probabilities, in ~150 ms on a 4070 Ti.

## Measured behaviour

From the tuning log of the reference setup:

| decision | outcome |
|---|---|
| file read, one tool | `read@0.99`, trimmed to `read` |
| monorepo refactor prompt | tier `pro@0.78`, promoted to the strong model |
| simple question | tier `flash@0.98`, stayed cheap |
| ambiguous edit | `edit@0.48` below gate → full tool list, no trim |
| warm latency | 47–50 ms per decision |

The gate is deliberately asymmetric: a wrong tool trim wastes a turn; a wrong tier promotion bills the expensive model.

## Limitations, stated plainly

- **Prices come from the local OpenCode cache.** They are a snapshot, not a live quote. Run Sage after your client refreshes its catalog.
- **Tiers are price-derived, not benchmarked.** A model can be mispriced relative to its ability; the ladder is a cost heuristic, and the router still needs a confidence gate before trusting it.
- **Free tiers change.** `ox-alpha-free` and friends are promotional stealth models; when they disappear, regenerate and the ladder updates.
- **Local tier is small.** `qwen3:8b` is fine for mechanical steps and bad for reasoning; keep its description narrow so the classifier doesn't send real work there.
- **Provider boundaries are hard.** A router that only rewrites `model` cannot cross providers. Only `providerOverride` (OpenClaw) can.

## License

MIT. The ranking data is derived from your own machine's catalog.

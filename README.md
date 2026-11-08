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

## What's in the box

Sage is one tool with four parts that used to live in separate repos:

| path | role |
|---|---|
| `rust/` | the CLI: ranking, ladders, `apply`, `stats`, `doctor`, `spawn`, completions |
| `serve/` | the local decision server: `front.py` (model discovery + strict response shaping), `serve.sh`, keepalive units |
| `opencode-plugin/` | the OpenCode fork: per-request tool trim + model tier |
| `openclaw-plugin/` | `jev-router`: per-turn model tier via `before_model_resolve` |

One command surface:

```sh
sage                  # regenerate models.json, RANKING.md, tiers.*, policy.json
sage apply            # write the ladder into the live configs (idempotent)
sage stats            # confidence histograms + apply rate per consumer
sage doctor           # judge / ollama / agent server health
sage spawn --auto ... # fan out across tiers
sage completions zsh --install
```

`apply` is the piece that closes the drift gap: generation alone rewrites the repo
artifacts, while `apply` patches `~/.config/jev/config.yaml` and the OpenClaw
`jev-router` entry in place, preserving every other key. Running it twice is a
no-op (byte-identical), so it is safe in a cron or a pre-flight check.

## Why a ladder instead of one "best" model

Cost and capability are not linear. On a real workload most steps are mechanical: read a file, grep a symbol, apply a known edit. Paying frontier prices for those is waste. Occasionally a step is genuinely hard — a multi-file refactor, an ambiguous spec, a debugging dead-end — and a cheap model will flail, costing more in retries than the upgrade would have.

A router that can name the difficulty of a step turns that into a pricing decision instead of a vibe.

## How models are ranked

Three components, all visible in `RANKING.md`:

| component | weight | meaning |
|---|---|---|
| **capability** | 60% | benchmark score from `benchmarks.json` when the family is known (LMArena / Artificial Analysis snapshot, 0–100); otherwise a price-proxy fallback. Locals fall back to weight size. |
| **value** | 25% | capability per dollar: `100 / (1 + 2 × blended price)`. Locals score 100. |
| **tokens** | 15% | context window normalised; locals capped at 55 because they are hardware-bound. |

`blended price = 0.7 × input + 0.3 × output`, so long-context work is weighted like the invoice is.

`benchmarks.json` ships a family-level snapshot with provenance (source, capture date, scale). It is approximate by construction: refresh it when a new generation lands, or point `SAGE_BENCH_URL` at a JSON dump with the same shape and run `sage.py benchmarks --refresh`. Models without a benchmark fall back to the price proxy, so the ranking degrades gracefully instead of breaking.

## How the tiers are built

Tiers are the rungs a router climbs, assigned by **price** (what you pay), while the ranking inside a tier is by composite score:

| rule | tier |
|---|---|
| local Ollama weights | 0 `local` |
| hosted, $0 in **and** $0 out | 1 `free` |
| ≤ $0.20/M in **and** ≤ $0.60/M out | 2 `cheap` |
| ≤ $1.00/M in | 3 `standard` |
| < $2.00/M in | 4 `strong` |
| everything above | 5 `frontier` |

## Local preference

If a model is reachable both from a cloud catalog and from local Ollama weights, Sage **drops the hosted copy and keeps the local one** — free, private, no egress. Matching is by normalised model name (case, separators and `:latest` ignored). No duplicates exist on this machine today, but the rule is implemented and tested by the dedupe pass.

## Availability probing

A rank is only useful if the model can actually answer. `sage probe` sends one
minimal request per candidate and records the result in `availability.json`:

```sh
sage probe              # ladder models only (~2s each)
sage probe --all        # every ranked model, slower
sage probe --missing    # only ids with no recorded result yet
sage refresh            # pull the catalog from models.dev (auto when >7d stale)
```

Generation and `apply` then resolve each rung: if the configured model is marked
unavailable, the rung falls back to the highest-scoring available model in the same
tier. Verified by forcing `qwen3.8-max` to `ok: false` — the frontier rung resolved
to `kimi-k3` in both the generated ladder and the live configs.

## Watch it work

```sh
sage watch
```

A ratatui monitor: per-consumer panel with the model currently in use, an applied-rate
bar, the last decisions, a decisions/sec sparkline and a spinner. `q` quits. It reads the
same logs `stats` does — no daemon, nothing to enable.

## Rung order

Rungs climb by **blended cost** (`0.7 × in + 0.3 × out`), so a $2 frontier model sits
near the top even when a $0.10 model scores close to it. Both halves of the price are
printed in each description, because a lone input price makes the ladder look
unordered. Within one price the weaker model comes first, and the default model is
always pinned in — it is where every session starts, so the judge must see the price
it is already paying.

## Choosing a rung

The judge answers "which rung?" as a 20-way choice, so a confident answer is only
about `0.27` — the mass is split 20 ways. A fixed confidence threshold therefore
pins every request to one model. Instead the whole distribution votes:

```
rung = round( Σ p_i · rank_i / Σ p_i )
```

Confidence only gates whether the answer is usable at all (50% above uniform,
i.e. `1.5 / n`). A trivial file read lands around rung 6; a repo-wide refactor
lands higher. Rungs carry an explicit `rank`, so the order survives YAML/JSON
round-trips instead of depending on object key order.

## Shell completion

```sh
sage completions zsh  --install   # writes ~/.zfunc/_sage, prints the two fpath lines to add
sage completions bash --install   # writes ~/.local/share/bash-completion/completions/sage
sage completions fish --install   # writes ~/.config/fish/completions/sage.fish
```

Completion is dynamic where it matters: `--tiers` completes from `sage list-tiers`
(the live ladder) and `list-models` prints the ranked ids for piping into other tools.
The zsh script is checked with `zsh -n` before install; no rc file is edited for you —
it prints the exact lines to add so nothing in your shell config changes behind your back.

## Fan out to multiple agents

Automatic: Jev picks the rung **and** how many agents the task deserves.

```sh
sage spawn --auto --task "Refactor a monorepo across three packages" --dry-run
# auto: tier=cheap agents=1 -> cheap
```

Manual:

```sh
# dry run — show which model each agent would get
python3 sage.py spawn --task "Summarise this repository" --tiers cheap,strong,frontier --dry-run

# real run: one opencode agent per rung, in parallel, logged per agent
python3 sage.py spawn --task "Summarise this repository" --tiers cheap,strong,frontier
# → spawn-cheap.log, spawn-strong.log, spawn-frontier.log
```

Each agent runs with the model that Sage assigned to its rung, so you can compare a cheap attempt against a frontier attempt on the same task, or split work across tiers. Logs are written beside the tool; exit codes are reported per agent.

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

## The niche

Most routers answer **one** question: *which model should handle this request?* They sit in front of an HTTP API, see a prompt, and return a model. That is the right shape for a chat product. It is the wrong shape for a coding agent, which makes hundreds of small decisions per session and re-sends every tool schema on each one.

Sage answers a different question: **how much intelligence does this step deserve, and which tool should carry it?** It runs beside the agent, not in front of a provider, and it can act on both answers in the same request.

What that buys you, and what it costs:

| | prompt-level routers (RouteLLM, OpenRouter Auto, NotDiamond) | Sage |
|---|---|---|
| decision unit | one user request | one tool step |
| sees tool catalogue | no | yes |
| can trim tools | no | yes |
| data leaves the machine | usually | **never** — local classifier |
| needs training data | RouteLLM yes | no, labels live in the request |
| cost | per-call fee or % | electricity |

The trade is honest: Sage's classifier is small, so its confidence is only as good as the state you feed it, and it needs a local GPU to stay under ~150 ms. Prompt-level routers have far more signal and no hardware requirement.

## Features

- **Per-step routing** — tool trim and model tier in one decision round-trip.
- **Benchmark-weighted tiers** — capability from a dated snapshot, price-aware value, context size.
- **Local preference** — a model reachable both hosted and locally resolves to the local copy.
- **Fallback chains** — `policy.json` gives every tier same-tier alternates ordered by score.
- **Budget estimate** — expected daily/monthly spend from an assumed call mix, so a tier change's cost is visible before you ship it.
- **Deterministic decisions** — identical state yields identical answers, so a router-side cache is safe.
- **Usage stats** — `sage stats` prints confidence histograms and apply rates per consumer, which is how you find out whether the gates are earning their latency.
- **Health check** — `sage doctor` verifies the decision server, Ollama, and the agent server.
- **Fan-out** — `sage spawn` runs one agent per rung, or `--auto` lets Jev choose tier and agent count.

## Install

```sh
git clone https://github.com/dazemc/sage
cd sage
cargo build --release --manifest-path rust/Cargo.toml
./rust/target/release/sage          # writes models.json, RANKING.md, tiers.yaml, tiers.openclaw.json, policy.json
./rust/target/release/sage stats    # confidence histograms per consumer
./rust/target/release/sage doctor   # health of judge / ollama / opencode
```

The implementation is Rust (serde_json only, HTTP via `curl`). It reads `~/.cache/opencode/models.json` (the OpenCode provider cache) and
`ollama list`.

Measured on this machine, full generation (40 models, five artifacts): **73 ms Rust** vs
**96 ms Python**, both dominated by the `ollama list` subprocess. The Rust win is real but
modest for batch generation; it matters for `doctor`/`stats` where startup cost dominates.

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

## Default model: fastest while cheapest

`sage` picks a starting model from measured latency, not a guess: among live
models it takes everything within **1.5x of the fastest probe**, then chooses the
cheapest, breaking ties on capability. That id is written to `default.json` and
pushed into `opencode.json` by `sage apply`, so a fresh session starts there
instead of on a hand-picked model.

## Retiring deprecated models

opencode only warns at request time, and some retired ids never warn at all
(`mimo-v2.5` answers happily while `mimo-v2.6-flash` exists at the same price).
Sage retires a model when a newer release of the same family **and variant**
costs no more — and a base model is retired by any newer variant of its family.
Previews, `-free` and alpha builds are never treated as successors.

`sage doctor` also scans the consumer logs for `deprecated` / `is unavailable`
lines, so a warning seen in the wild can be turned into a probe + `sage apply`.

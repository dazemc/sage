#!/usr/bin/env python3
"""Sage — rank every model on this machine into a tier ladder a router can climb.

Reads the local OpenCode model cache (pricing, context, capability flags) and the
Ollama inventory, then emits:
  - models.json  : the ranked catalog
  - RANKING.md   : a human table
  - tiers.yaml   : a ready-to-paste ladder for `jev` (opencode plugin)
  - tiers.openclaw.json5 : the same ladder for the OpenClaw `jev-router` plugin

Tier policy
  0 local     free, private, small — trivial steps
  1 free      hosted but zero-cost
  2 cheap     <= $0.20/M in, <= $0.60/M out
  3 standard  <= $1.00/M in
  4 strong    <= $2.00/M in
  5 frontier  above that

Usage:
  python3 sage.py            # write artifacts next to this file
  python3 sage.py --table    # print the table only
"""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CACHE = Path.home() / ".cache/opencode/models.json"
PROVIDER = "opencode-go"

TIER_NAMES = {
    0: "local",
    1: "free",
    2: "cheap",
    3: "standard",
    4: "strong",
    5: "frontier",
}

# Curated choice per rung for a client that can only change the model id
# (opencode: same provider). Local models live on a different provider, so the
# opencode ladder starts at 1; the OpenClaw ladder can start at 0.
OPENCODE_LADDER = {1: "ox-alpha-free", 2: "glm-5.3-flash", 3: "gpt-5.6-luna", 4: "deepseek-v4-pro", 5: "qwen3.8-max"}
OPENCLAW_LADDER = {
    0: ("ollama", "qwen3:8b"),
    1: ("opencode-go", "ox-alpha-free"),
    2: ("opencode-go", "glm-5.3-flash"),
    3: ("opencode-go", "gpt-5.6-luna"),
    4: ("opencode-go", "deepseek-v4-pro"),
    5: ("opencode-go", "qwen3.8-max"),
}

TIER_DESCRIPTIONS = {
    0: "Trivial, mechanical steps with no ambiguity: greet, echo, single grep",
    1: "Simple single-tool work: read one file, answer a short question",
    2: "Everyday edits and searches: small code changes, targeted greps, summaries",
    3: "Moderate engineering: several files, tests, debugging a clear failure",
    4: "Hard engineering: multi-file refactors, architecture, ambiguous specs",
    5: "Frontier reasoning: long-horizon autonomous work where a wrong answer is expensive",
}


def tier_for(price_in: float, price_out: float, local: bool) -> int:
    if local:
        return 0
    if price_in == 0 and price_out == 0:
        return 1
    if price_in <= 0.20 and price_out <= 0.60:
        return 2
    if price_in <= 1.00:
        return 3
    if price_in < 2.00:
        return 4
    return 5


def load_hosted() -> list[dict]:
    data = json.loads(CACHE.read_text())
    provider = data.get(PROVIDER) or {}
    models = provider.get("models") or {}
    out = []
    for name, model in models.items():
        cost = model.get("cost") or {}
        limit = model.get("limit") or {}
        pin = float(cost.get("input") or 0)
        pout = float(cost.get("output") or 0)
        out.append(
            {
                "id": f"{PROVIDER}/{name}",
                "name": model.get("name") or name,
                "provider": PROVIDER,
                "local": False,
                "cost_input": pin,
                "cost_output": pout,
                "context": limit.get("context"),
                "max_output": limit.get("output"),
                "reasoning": bool(model.get("reasoning")),
                "tool_call": bool(model.get("tool_call")),
                "multimodal": "image" in ((model.get("modalities") or {}).get("input") or []),
                "description": (model.get("description") or "").strip(),
                "tier": tier_for(pin, pout, False),
            }
        )
    return out


def load_local() -> list[dict]:
    try:
        raw = subprocess.run(["ollama", "list"], capture_output=True, text=True, timeout=20).stdout
    except Exception:
        return []
    out = []
    for line in raw.splitlines()[1:]:
        parts = line.split()
        if len(parts) < 3:
            continue
        name = parts[0]
        if "openjev" in name.lower() or name.startswith("qwen:"):
            continue  # decision model, not a chat model
        out.append(
            {
                "id": f"ollama/{name}",
                "name": name,
                "provider": "ollama",
                "local": True,
                "cost_input": 0,
                "cost_output": 0,
                "context": None,
                "max_output": None,
                "reasoning": True,
                "tool_call": True,
                "multimodal": False,
                "description": "Local Ollama model — free, private, hardware-bound",
                "tier": 0,
            }
        )
    return out


def rank(models: list[dict]) -> list[dict]:
    for m in models:
        m["rank_score"] = round(m["cost_input"] * 3 + m["cost_output"], 3)
    return sorted(models, key=lambda m: (m["tier"], m["rank_score"], m["id"]))


def table(models: list[dict]) -> str:
    lines = ["| tier | model | in $/M | out $/M | ctx | tags |", "|---|---|---|---|---|---|"]
    for m in models:
        ctx = f"{m['context']:,}" if m.get("context") else "—"
        tags = ", ".join(
            t for t in (
                "reasoning" if m.get("reasoning") else "",
                "tools" if m.get("tool_call") else "",
                "vision" if m.get("multimodal") else "",
                "local" if m.get("local") else "",
            ) if t
        )
        lines.append(
            f"| {m['tier']} {TIER_NAMES[m['tier']]} | `{m['id']}` | {m['cost_input']} | {m['cost_output']} | {ctx} | {tags} |"
        )
    return "\n".join(lines)


def jev_yaml(models: list[dict]) -> str:
    by_id = {m["id"].split("/")[-1]: m for m in models}
    out = ["# Generated by Sage — tier ladder for the opencode jev plugin.", "tiers:"]
    for tier, name in OPENCODE_LADDER.items():
        model = by_id.get(name)
        if not model:
            continue
        out += [
            f"  {TIER_NAMES[tier]}:",
            f"    model: {name}",
            f"    description: {json.dumps(TIER_DESCRIPTIONS[tier])}",
        ]
    return "\n".join(out) + "\n"


def openclaw_json5(models: list[dict]) -> str:
    by_id = {m["id"]: m for m in models}
    tiers = {}
    for tier, (provider, name) in OPENCLAW_LADDER.items():
        full = f"{provider}/{name}"
        if full not in by_id:
            continue
        tiers[TIER_NAMES[tier]] = {"model": name, "provider": provider, "description": TIER_DESCRIPTIONS[tier]}
    return json.dumps({"tiers": tiers}, indent=2) + "\n"


def main() -> None:
    models = rank(load_local() + load_hosted())
    if "--table" in sys.argv:
        print(table(models))
        return
    (HERE / "models.json").write_text(json.dumps(models, indent=2) + "\n")
    (HERE / "RANKING.md").write_text("# Sage model ranking\n\n" + table(models) + "\n")
    (HERE / "tiers.yaml").write_text(jev_yaml(models))
    (HERE / "tiers.openclaw.json").write_text(openclaw_json5(models))
    counts = {}
    for m in models:
        counts[m["tier"]] = counts.get(m["tier"], 0) + 1
    print(f"wrote {len(models)} models: " + ", ".join(f"tier {t}={n}" for t, n in sorted(counts.items())))


if __name__ == "__main__":
    main()

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
# A ladder is a curated subset: too many rungs flatten the classifier's
# confidence. Four is the practical maximum — free, cheap, strong, frontier —
# plus a local rung where the consumer can change provider.
OPENCODE_LADDER = [("free", "ox-alpha-free"), ("cheap", "glm-5.3-flash"), ("strong", "deepseek-v4-pro"), ("frontier", "qwen3.8-max")]
OPENCLAW_LADDER = [
    ("local", ("ollama", "qwen3:8b")),
    ("free", ("opencode-go", "ox-alpha-free")),
    ("cheap", ("opencode-go", "glm-5.3-flash")),
    ("strong", ("opencode-go", "deepseek-v4-pro")),
    ("frontier", ("opencode-go", "qwen3.8-max")),
]

TIER_DESCRIPTIONS = {
    "local": "Trivial, mechanical steps with no ambiguity: greet, echo, single grep",
    "free": "Simple single-tool work: read one file, answer a short question",
    "cheap": "Everyday edits and searches: small code changes, targeted greps, summaries",
    "strong": "Hard engineering: multi-file refactors, architecture, ambiguous specs, debugging dead-ends",
    "frontier": "Frontier reasoning: long-horizon autonomous work where a wrong answer is expensive",
}

def blended_price(model: dict) -> float:
    return model["cost_input"] * 0.7 + model["cost_output"] * 0.3


def compute_scores(models: list[dict]) -> None:
    """Three transparent components, no invented benchmarks.

    capability — the vendor's own price signal, normalised to 0-100 across the
    catalog, plus small bonuses for reasoning/tools/multimodal/context. Within
    one provider, price tracks capability far better than a name heuristic.
    price      — value score: how much capability you get per dollar (locals 100).
    tokens     — context window, normalised, with locals capped (hardware-bound).
    """
    priced = [m for m in models if not m["local"]]
    prices = sorted(blended_price(m) for m in priced) or [0.0]
    lo, hi = min(prices), max(prices)

    for m in models:
        if m["local"]:
            m["capability"] = round(min(85.0, 30.0 + float(m.get("size_gb") or 4.0) * 2.0), 1)
        else:
            p = blended_price(m)
            span = (hi - lo) or 1.0
            m["capability"] = round(15.0 + 80.0 * ((p - lo) / span), 1)
            if m.get("reasoning"):
                m["capability"] = min(100.0, m["capability"] + 4)
            if m.get("tool_call"):
                m["capability"] = min(100.0, m["capability"] + 2)
            if m.get("multimodal"):
                m["capability"] = min(100.0, m["capability"] + 2)

    for m in models:
        if m["local"]:
            m["price_score"] = 100.0
        else:
            p = blended_price(m)
            m["price_score"] = 100.0 if p <= 0 else round(max(0.0, min(100.0, 100.0 / (1.0 + p * 2))), 1)
        ctx = m.get("context") or 0
        m["tokens_score"] = 55.0 if m["local"] else round(min(100.0, ctx / 1_000_000 * 100), 1)
        m["score"] = round(0.60 * m["capability"] + 0.25 * m["price_score"] + 0.15 * m["tokens_score"], 1)


def tier_of(model: dict) -> int:
    """Routing rung by price (the thing you actually pay), locals on rung 0.
    Ranking order is the composite score; the tier is what a router climbs."""
    if model["local"]:
        return 0
    pin, pout = model["cost_input"], model["cost_output"]
    if pin == 0 and pout == 0:
        return 1
    if pin <= 0.20 and pout <= 0.60:
        return 2
    if pin <= 1.00:
        return 3
    if pin < 2.00:
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
        size_gb = 0.0
        for i, part in enumerate(parts):
            if part.upper() in ("GB", "MB") and i > 0:
                try:
                    size_gb = float(parts[i - 1]) * (1 if part.upper() == "GB" else 0.001)
                except ValueError:
                    size_gb = 0.0
                break
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
                "size_gb": round(size_gb, 1),
                "description": "Local Ollama model — free, private, hardware-bound",
                "release_date": "",
            }
        )
    return out


def rank(models: list[dict]) -> list[dict]:
    compute_scores(models)
    for m in models:
        m["tier"] = tier_of(m)

    # Local preference: if the same model is reachable both ways, keep the local
    # copy (free, private, no egress) and drop the hosted duplicate.
    def norm(model: dict) -> str:
        base = model["id"].split("/")[-1].lower()
        return base.replace(":latest", "").replace("-", "").replace("_", "")

    local_keys = {norm(m) for m in models if m["local"]}
    kept = []
    for m in models:
        if not m["local"] and norm(m) in local_keys:
            continue
        kept.append(m)
    return sorted(kept, key=lambda m: (-m["score"], m["id"]))
def table(models: list[dict]) -> str:
    lines = [
        "| rank | tier | model | score | perf | price | tokens | $/M in | $/M out | ctx |",
        "|---|---|---|---|---|---|---|---|---|---|",
    ]
    for i, m in enumerate(models, 1):
        ctx = f"{m['context']:,}" if m.get("context") else "—"
        tag = "local" if m["local"] else ""
        lines.append(
            f"| {i} | {m['tier']} {TIER_NAMES[m['tier']]} | `{m['id']}` {tag} | {m['score']} | "
            f"{m['capability']} | {m['price_score']} | {m['tokens_score']} | {m['cost_input']} | {m['cost_output']} | {ctx} |"
        )
    return "\n".join(lines)


def jev_yaml(models: list[dict]) -> str:
    by_id = {m["id"].split("/")[-1]: m for m in models}
    out = ["# Generated by Sage — tier ladder for the opencode jev plugin.", "tiers:"]
    for name, model_id in OPENCODE_LADDER:
        if model_id not in by_id:
            continue
        out += [f"  {name}:", f"    model: {model_id}", f"    description: {json.dumps(TIER_DESCRIPTIONS[name])}"]
    return "\n".join(out) + "\n"


def openclaw_json5(models: list[dict]) -> str:
    by_id = {m["id"]: m for m in models}
    tiers = {}
    for name, (provider, model_id) in OPENCLAW_LADDER:
        if f"{provider}/{model_id}" not in by_id:
            continue
        tiers[name] = {"model": model_id, "provider": provider, "description": TIER_DESCRIPTIONS[name]}
    return json.dumps({"tiers": tiers}, indent=2) + "\n"


def spawn(argv: list[str]) -> None:
    """Fan a task out to several agents, one per rung, using opencode."""
    import argparse
    import os
    import subprocess

    ap = argparse.ArgumentParser(prog="sage spawn")
    ap.add_argument("--task", required=True)
    ap.add_argument("--tiers", default="cheap,strong,frontier")
    ap.add_argument("--dry-run", action="store_true")
    args = ap.parse_args(argv)

    models = {m["id"].split("/")[-1]: m for m in rank(load_local() + load_hosted())}
    ladder = dict(OPENCODE_LADDER)
    procs = []
    for i, name in enumerate([t.strip() for t in args.tiers.split(",") if t.strip()]):
        model = ladder.get(name)
        if not model:
            print(f"skip {name}: not in ladder")
            continue
        cmd = ["opencode", "run", args.task, "--model", f"opencode-go/{model}"]
        print(("would run: " if args.dry_run else "spawn: ") + " ".join(cmd[:2]) + f" … --model opencode-go/{model}")
        if not args.dry_run:
            log = open(f"spawn-{name}.log", "w")
            procs.append((name, subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT)))
    if procs:
        print("waiting for", len(procs), "agents…")
        for name, proc in procs:
            proc.wait()
            print(f"  {name}: exit {proc.returncode} → spawn-{name}.log")


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "spawn":
        spawn(sys.argv[2:])
        return
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

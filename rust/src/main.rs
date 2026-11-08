//! Sage — rank every model on the machine into a tier ladder a router can climb.
//!
//! Rust port of `sage.py`: same artifacts, same scoring, no runtime deps beyond
//! serde_json. HTTP goes through `curl` so the binary stays dependency-light.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

const PROVIDER: &str = "opencode-go";
const JEV_URL_DEFAULT: &str = "http://127.0.0.1:3000/v1/systemone";

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".into()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Model {
    id: String,
    name: String,
    provider: String,
    local: bool,
    cost_input: f64,
    cost_output: f64,
    context: Option<u64>,
    max_output: Option<u64>,
    reasoning: bool,
    tool_call: bool,
    multimodal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    benchmark: Option<f64>,
    description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    size_gb: Option<f64>,
    capability: f64,
    price_score: f64,
    tokens_score: f64,
    score: f64,
    tier: u8,
}

fn tier_name(t: u8) -> &'static str {
    match t {
        0 => "local",
        1 => "free",
        2 => "cheap",
        3 => "standard",
        4 => "strong",
        _ => "frontier",
    }
}

fn tier_of(m: &Model) -> u8 {
    if m.local {
        return 0;
    }
    if m.cost_input == 0.0 && m.cost_output == 0.0 {
        return 1;
    }
    if m.cost_input <= 0.20 && m.cost_output <= 0.60 {
        return 2;
    }
    if m.cost_input <= 1.00 {
        return 3;
    }
    if m.cost_input < 2.00 {
        return 4;
    }
    5
}

fn blended(m: &Model) -> f64 {
    m.cost_input * 0.7 + m.cost_output * 0.3
}

fn load_availability(root: &Path) -> BTreeMap<String, bool> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(root.join("availability.json")) else {
        return out;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    if let Some(models) = v.get("models").and_then(|m| m.as_object()) {
        for (id, entry) in models {
            if let Some(ok) = entry.get("ok").and_then(|b| b.as_bool()) {
                out.insert(id.clone(), ok);
            }
        }
    }
    out
}

fn load_benchmarks(root: &Path) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    let Ok(text) = fs::read_to_string(root.join("benchmarks.json")) else {
        return out;
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return out;
    };
    for key in ["families", "local"] {
        if let Some(map) = v.get(key).and_then(|m| m.as_object()) {
            for (k, val) in map {
                if let Some(f) = val.as_f64() {
                    out.insert(k.to_lowercase(), f);
                }
            }
        }
    }
    out
}

fn benchmark_score(m: &Model, table: &BTreeMap<String, f64>) -> Option<f64> {
    let hay = format!("{} {}", m.name.to_lowercase(), m.id.to_lowercase());
    table
        .iter()
        .filter(|(k, _)| hay.contains(k.as_str()))
        .max_by_key(|(k, _)| k.len())
        .map(|(_, v)| *v)
}

fn load_hosted(_root: &Path) -> Vec<Model> {
    let path = home().join(".cache/opencode/models.json");
    let Ok(text) = fs::read_to_string(path) else {
        return vec![];
    };
    let Ok(v) = serde_json::from_str::<Value>(&text) else {
        return vec![];
    };
    let Some(models) = v
        .get(PROVIDER)
        .and_then(|p| p.get("models"))
        .and_then(|m| m.as_object())
    else {
        return vec![];
    };
    models
        .iter()
        .map(|(key, m)| {
            let cost = m.get("cost").cloned().unwrap_or(json!({}));
            let limit = m.get("limit").cloned().unwrap_or(json!({}));
            let modalities = m.get("modalities").cloned().unwrap_or(json!({}));
            let vision = modalities
                .get("input")
                .and_then(|i| i.as_array())
                .map(|a| a.iter().any(|x| x.as_str() == Some("image")))
                .unwrap_or(false);
            Model {
                id: format!("{PROVIDER}/{key}"),
                name: m
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(key)
                    .to_string(),
                provider: PROVIDER.into(),
                local: false,
                cost_input: cost.get("input").and_then(|c| c.as_f64()).unwrap_or(0.0),
                cost_output: cost.get("output").and_then(|c| c.as_f64()).unwrap_or(0.0),
                context: limit.get("context").and_then(|c| c.as_u64()),
                max_output: limit.get("output").and_then(|c| c.as_u64()),
                reasoning: m.get("reasoning").and_then(|b| b.as_bool()).unwrap_or(false),
                tool_call: m.get("tool_call").and_then(|b| b.as_bool()).unwrap_or(false),
                multimodal: vision,
                benchmark: None,
                description: m
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                size_gb: None,
                capability: 0.0,
                price_score: 0.0,
                tokens_score: 0.0,
                score: 0.0,
                tier: 0,
            }
        })
        .collect()
}

fn load_local() -> Vec<Model> {
    let Ok(out) = Command::new("ollama").arg("list").output() else {
        return vec![];
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let mut models = vec![];
    for line in text.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }
        let name = parts[0];
        if name.contains("openjev") || name.starts_with("qwen:") {
            continue;
        }
        let mut size_gb = 0.0;
        for (i, part) in parts.iter().enumerate() {
            if i > 0 && (*part == "GB" || *part == "MB") {
                if let Ok(v) = parts[i - 1].parse::<f64>() {
                    size_gb = if *part == "GB" { v } else { v * 0.001 };
                }
                break;
            }
        }
        models.push(Model {
            id: format!("ollama/{name}"),
            name: name.to_string(),
            provider: "ollama".into(),
            local: true,
            cost_input: 0.0,
            cost_output: 0.0,
            context: None,
            max_output: None,
            reasoning: true,
            tool_call: true,
            multimodal: false,
            benchmark: None,
            description: "Local Ollama model — free, private, hardware-bound".into(),
            size_gb: Some((size_gb * 10.0).round() / 10.0),
            capability: 0.0,
            price_score: 0.0,
            tokens_score: 0.0,
            score: 0.0,
            tier: 0,
        });
    }
    models
}

fn compute_scores(models: &mut [Model], bench: &BTreeMap<String, f64>) {
    let prices: Vec<f64> = models.iter().filter(|m| !m.local).map(blended).collect();
    let lo = prices.iter().cloned().fold(f64::INFINITY, f64::min);
    let hi = prices.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let span = if (hi - lo).abs() < f64::EPSILON { 1.0 } else { hi - lo };

    for m in models.iter_mut() {
        if let Some(b) = benchmark_score(m, bench) {
            m.benchmark = Some(b);
            m.capability = b;
        } else if m.local {
            let size = m.size_gb.unwrap_or(4.0);
            m.capability = (30.0 + size * 2.0).min(85.0);
        } else {
            let p = blended(m);
            m.capability = (15.0 + 80.0 * ((p - lo) / span)).clamp(0.0, 100.0);
            if m.reasoning {
                m.capability = (m.capability + 4.0).min(100.0);
            }
            if m.tool_call {
                m.capability = (m.capability + 2.0).min(100.0);
            }
            if m.multimodal {
                m.capability = (m.capability + 2.0).min(100.0);
            }
        }

        if m.local {
            m.price_score = 100.0;
            m.tokens_score = 55.0;
        } else {
            let p = blended(m);
            m.price_score = if p <= 0.0 {
                100.0
            } else {
                (100.0 / (1.0 + p * 2.0)).clamp(0.0, 100.0)
            };
            m.tokens_score = (m.context.unwrap_or(0) as f64 / 1_000_000.0 * 100.0).min(100.0);
        }

        m.score = (0.60 * m.capability + 0.25 * m.price_score + 0.15 * m.tokens_score) * 10.0;
        m.score = m.score.round() / 10.0;
        m.capability = (m.capability * 10.0).round() / 10.0;
        m.price_score = (m.price_score * 10.0).round() / 10.0;
        m.tokens_score = (m.tokens_score * 10.0).round() / 10.0;
        m.tier = tier_of(m);
    }
}

fn normalize_name(m: &Model) -> String {
    m.id
        .rsplit('/')
        .next()
        .unwrap_or(&m.id)
        .to_lowercase()
        .replace(":latest", "")
        .replace(['-', '_'], "")
}

fn rank(mut models: Vec<Model>, bench: &BTreeMap<String, f64>) -> Vec<Model> {
    compute_scores(&mut models, bench);
    let locals: Vec<String> = models
        .iter()
        .filter(|m| m.local)
        .map(|m| normalize_name(m))
        .collect();
    let mut kept: Vec<Model> = models
        .into_iter()
        .filter(|m| m.local || !locals.contains(&normalize_name(m)))
        .collect();
    kept.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    kept
}

fn table(models: &[Model]) -> String {
    let mut out = String::from(
        "| rank | tier | model | score | perf | price | tokens | $/M in | $/M out | ctx |\n|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for (i, m) in models.iter().enumerate() {
        let ctx = m
            .context
            .map(|c| c.to_string())
            .unwrap_or_else(|| "—".into());
        let tag = if m.local { " local" } else { "" };
        out.push_str(&format!(
            "| {} | {} {} | `{}`{} | {:.1} | {:.1} | {:.1} | {:.1} | {} | {} | {} |\n",
            i + 1,
            m.tier,
            tier_name(m.tier),
            m.id,
            tag,
            m.score,
            m.capability,
            m.price_score,
            m.tokens_score,
            m.cost_input,
            m.cost_output,
            ctx
        ));
    }
    out
}

fn available(m: &Model, availability: &BTreeMap<String, bool>) -> bool {
    availability.get(&m.id).copied().unwrap_or(true)
}

#[derive(Clone)]
struct Rung {
    name: String,
    provider: String,
    model: String,
    description: String,
    rank: u32,
}

fn rung_name(m: &Model) -> String {
    m.id.rsplit('/').next().unwrap_or(&m.id).to_string()
}

fn rung_description(m: &Model) -> String {
    let price = if m.local {
        "local, free".to_string()
    } else {
        format!("${:.2}/M in", m.cost_input)
    };
    let base = if m.description.is_empty() {
        format!("capability {:.0}", m.capability)
    } else {
        m.description.clone()
    };
    format!("{} · {}", price, base).chars().take(180).collect()
}

/// One rung per available model, ordered weakest/cheapest -> strongest.
fn build_ladders(
    models: &[Model],
    availability: &BTreeMap<String, bool>,
) -> (Vec<Rung>, Vec<Rung>) {
    let mut hosted: Vec<&Model> = models
        .iter()
        .filter(|m| !m.local && available(m, availability))
        .collect();
    hosted.sort_by(|a, b| {
        a.capability
            .partial_cmp(&b.capability)
            .unwrap()
            .then_with(|| a.score.partial_cmp(&b.score).unwrap())
    });

    // Ollama caps top_logprobs at 20, so a single decision can only score 20
    // rungs. Keep the full range but sample evenly when the catalog is bigger.
    let selected: Vec<&Model> = if hosted.len() > 20 {
        let step = (hosted.len() - 1) as f64 / 19.0;
        (0..20)
            .map(|i| hosted[(i as f64 * step).round() as usize])
            .collect()
    } else {
        hosted.clone()
    };

    let mut opencode = vec![];
    for (i, m) in selected.iter().enumerate() {
        opencode.push(Rung {
            name: rung_name(m),
            provider: m.provider.clone(),
            model: rung_name(m),
            description: rung_description(m),
            rank: i as u32,
        });
    }

    let mut openclaw = vec![];
    let local: Vec<&Model> = models
        .iter()
        .filter(|m| m.local && available(m, availability))
        .collect();
    if let Some(m) = local.iter().max_by(|a, b| a.capability.partial_cmp(&b.capability).unwrap()) {
        openclaw.push(Rung {
            name: "local".to_string(),
            provider: m.provider.clone(),
            model: rung_name(m),
            description: rung_description(m),
            rank: 0,
        });
    }
    for rung in &opencode {
        openclaw.push(Rung { rank: rung.rank + 1, ..rung.clone() });
    }
    (opencode, openclaw)
}

fn description(rung: &str) -> &'static str {
    match rung {
        "local" => "Trivial, mechanical steps with no ambiguity: greet, echo, single grep",
        "free" => "Simple single-tool work: read one file, answer a short question",
        "cheap" => "Everyday edits and searches: small code changes, targeted greps, summaries",
        "standard" => "Moderate engineering: several files, tests, debugging a clear failure",
        "strong" => "Hard engineering: multi-file refactors, architecture, ambiguous specs, debugging dead-ends",
        _ => "Frontier reasoning: long-horizon autonomous work where a wrong answer is expensive",
    }
}

fn jev_yaml_with(ladder: &[Rung]) -> String {
    let mut out = String::from("# Generated by Sage — tier ladder for the opencode jev plugin.\ntiers:\n");
    for rung in ladder {
        out.push_str(&format!(
            "  {}:\n    model: {}\n    description: {}\n",
            rung.name,
            rung.model,
            serde_json::to_string(&rung.description).unwrap()
        ));
    }
    out
}

fn openclaw_json_with(ladder: &[Rung]) -> String {
    let mut tiers = serde_json::Map::new();
    for rung in ladder {
        tiers.insert(
            rung.name.clone(),
            json!({"model": rung.model, "provider": rung.provider, "description": rung.description}),
        );
    }
    serde_json::to_string_pretty(&json!({"tiers": tiers})).unwrap() + "\n"
}

fn policy(models: &[Model]) -> String {
    let mut tiers = serde_json::Map::new();
    for tier in 1..=5u8 {
        let mut members: Vec<&Model> = models
            .iter()
            .filter(|m| !m.local && m.tier == tier)
            .collect();
        if members.is_empty() {
            continue;
        }
        members.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
        let primary = members[0].id.clone();
        let fallbacks: Vec<String> = members.iter().skip(1).take(2).map(|m| m.id.clone()).collect();
        tiers.insert(
            tier_name(tier).to_string(),
            json!({"primary": primary, "fallbacks": fallbacks}),
        );
    }
    let mut daily = 0.0;
    for (tier, share) in [(2u8, 0.70), (3, 0.25), (4, 0.05)] {
        let best = models
            .iter()
            .filter(|m| !m.local && m.tier == tier)
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap());
        if let Some(m) = best {
            let cost = (1500.0 / 1e6) * m.cost_input + (400.0 / 1e6) * m.cost_output;
            daily += 2000.0 * share * cost;
        }
    }
    serde_json::to_string_pretty(&json!({
        "tiers": tiers,
        "budget": {
            "assumed": "2000 decisions/day, 70% cheap / 25% standard / 5% strong",
            "estimated_daily_usd": (daily * 100.0).round() / 100.0,
            "estimated_monthly_usd": (daily * 30.0 * 100.0).round() / 100.0
        },
        "notes": [
            "Fallbacks are same-tier alternates ordered by Sage score.",
            "Decision calls are deterministic for identical state, so a router cache is safe."
        ]
    }))
    .unwrap()
        + "\n"
}

fn generate(root: &Path) -> Result<(), String> {
    let bench = load_benchmarks(root);
    let mut all = load_local();
    all.extend(load_hosted(root));
    let models = rank(all, &bench);
    let counts = models.iter().fold(BTreeMap::new(), |mut acc, m| {
        *acc.entry(m.tier).or_insert(0) += 1;
        acc
    });

    let write = |name: &str, body: String| -> Result<(), String> {
        fs::write(root.join(name), body).map_err(|e| format!("{name}: {e}"))
    };
    write("models.json", serde_json::to_string_pretty(&models).unwrap() + "\n")?;
    write("RANKING.md", format!("# Sage model ranking\n\n{}\n", table(&models)))?;
    let availability = load_availability(root);
    let (oc_ladder, ow_ladder) = build_ladders(&models, &availability);
    write("tiers.yaml", jev_yaml_with(&oc_ladder))?;
    write("tiers.openclaw.json", openclaw_json_with(&ow_ladder))?;
    write("policy.json", policy(&models))?;
    let summary: Vec<String> = counts.iter().map(|(t, n)| format!("tier {t}={n}")).collect();
    println!("wrote {} models: {}", models.len(), summary.join(", "));
    Ok(())
}

fn parse_conf(line: &str) -> Option<f64> {
    let at = line.find('@')?;
    let rest = &line[at + 1..];
    let end = rest
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(rest.len());
    rest[..end].parse::<f64>().ok()
}

fn report(path: &str, label: &str) {
    let Ok(text) = fs::read_to_string(path) else {
        println!("{label}: no log at {path}");
        return;
    };
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("apply:") || l.contains("bypass:"))
        .collect();
    let mut buckets = [0usize; 10];
    let mut applied = 0;
    for line in &lines {
        if let Some(conf) = parse_conf(line) {
            buckets[(conf * 10.0).floor().clamp(0.0, 9.0) as usize] += 1;
        }
        if line.contains("apply:") {
            applied += 1;
        }
    }
    println!(
        "{label}: {} decisions, {} applied ({:.0}%)",
        lines.len(),
        applied,
        applied as f64 / lines.len().max(1) as f64 * 100.0
    );
    for (i, count) in buckets.iter().enumerate() {
        if *count > 0 {
            println!(
                "  {:.1}–{:.1}  {:>4}  {}",
                i as f64 / 10.0,
                (i + 1) as f64 / 10.0,
                count,
                "█".repeat((*count).min(60))
            );
        }
    }
}

fn stats() {
    report("/tmp/jev-router.log", "OpenClaw");
    println!();
    report("/tmp/opencode-jev.log", "opencode");
}

fn curl_ok(url: &str) -> bool {
    Command::new("curl")
        .args(["-sf", "--max-time", "5", "-o", "/dev/null", url])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn doctor() {
    let checks = [
        ("judge /v1/models", curl_ok("http://127.0.0.1:3000/v1/models")),
        ("ollama", curl_ok("http://127.0.0.1:11434/api/tags")),
        (
            "opencode server",
            Command::new("pgrep")
                .args(["-f", "opencode serve"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false),
        ),
    ];
    for (name, ok) in checks {
        println!("{} {name}", if ok { "ok  " } else { "FAIL" });
    }
}

fn ask_jev(state: &str, questions: Value) -> Option<Value> {
    let url = std::env::var("SAGE_JEV_URL").unwrap_or_else(|_| JEV_URL_DEFAULT.into());
    let body = json!({"state": state, "model": "kev-latest", "questions": questions}).to_string();
    let out = Command::new("curl")
        .args(["-s", "--max-time", "10", "-H", "Content-Type: application/json", "-d", &body, &url])
        .output()
        .ok()?;
    serde_json::from_slice::<Value>(&out.stdout).ok()
}

fn spawn(task: &str, tiers: &str, dry_run: bool) {
    let mut picks: Vec<&str> = tiers.split(',').map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    if picks.is_empty() {
        picks = vec!["cheap"];
    }
    let mut children = vec![];
    for rung in picks {
        let bench = load_benchmarks(Path::new("."));
        let mut all = load_local();
        all.extend(load_hosted(Path::new(".")));
        let ranked = rank(all, &bench);
        let (ladder, _) = build_ladders(&ranked, &load_availability(Path::new(".")));
        let Some(found) = ladder.iter().find(|r| r.name == rung) else {
            println!("skip {rung}: not in ladder");
            continue;
        };
        let spec = format!("opencode-go/{}", found.model);
        if dry_run {
            println!("would run: opencode run … --model {spec}");
            continue;
        }
        match Command::new("opencode")
            .args(["run", task, "--model", &spec])
            .stdout(fs::File::create(format!("spawn-{rung}.log")).unwrap())
            .stderr(std::process::Stdio::inherit())
            .spawn()
        {
            Ok(child) => children.push((rung.to_string(), child)),
            Err(e) => println!("spawn {rung} failed: {e}"),
        }
    }
    for (rung, mut child) in children {
        let status = child.wait();
        println!("  {rung}: {:?} → spawn-{rung}.log", status.map(|s| s.code()));
    }
}

fn auto_plan(_task: &str) -> Vec<String> {
    let criteria: serde_json::Map<String, Value> = (0..=5u8)
        .map(|tier| (tier_name(tier).to_string(), json!(description(tier_name(tier)))))
        .collect();
    let questions = json!({
        "tier": {"type": "choice", "instructions": "Which tier should handle this task?", "criteria": criteria},
        "fanout": {"type": "score", "instructions": "How many parallel agents does this task need?",
                   "criteria": ["1 agent", "2 agents", "3 agents", "4 agents"]}
    });
    let Some(answer) = ask_jev("", questions) else {
        return vec!["cheap".into()];
    };
    let answers = answer.get("answers").cloned().unwrap_or(json!({}));
    let tier = answers
        .get("tier")
        .and_then(|t| t.get("choice"))
        .and_then(|c| c.as_str())
        .unwrap_or("cheap")
        .to_string();
    let score = answers
        .get("fanout")
        .and_then(|f| f.get("score"))
        .and_then(|s| s.as_f64())
        .unwrap_or(0.0);
    let count = ((score.round() as i64) + 1).clamp(1, 4) as usize;
    let order: Vec<&str> = (0..=5u8).map(tier_name).collect();
    let start = order.iter().position(|r| *r == tier).unwrap_or(2);
    (0..count)
        .map(|i| order[(start + i).min(order.len() - 1)].to_string())
        .collect()
}

fn list_tiers() {
    for tier in 0..=5u8 {
        println!("{}", tier_name(tier));
    }
}

fn list_models() {
    let Ok(text) = fs::read_to_string("models.json") else {
        return;
    };
    if let Ok(models) = serde_json::from_str::<Vec<Model>>(&text) {
        for m in models {
            println!("{}", m.id);
        }
    }
}

fn completions(shell: &str, install: bool) {
    let zsh = r#"#compdef sage
_sage() {
  local -a commands
  commands=(
    'stats:confidence histograms per consumer'
    'watch:live monitor of both consumers'
    'doctor:health of judge, ollama and the agent server'
    'spawn:run one agent per tier'
    'list-tiers:print ladder rungs'
    'list-models:print ranked model ids'
    'completions:print shell completion'
  )
  if (( CURRENT == 2 )); then
    _describe 'command' commands
    return
  fi
  case $words[2] in
    spawn)
      _arguments \
        '--task[task text]:task:' \
        '--tiers[tiers to run]:tiers:($(sage list-tiers | tr "\n" " "))' \
        '--auto[let Jev choose tier and count]' \
        '--dry-run[print commands only]'
      ;;
    completions)
      _values 'shell' bash zsh fish
      ;;
  esac
}
_sage
"#;
    let bash = r#"_sage_complete() {
  local cur="${COMP_WORDS[COMP_CWORD]}"
  local prev="${COMP_WORDS[COMP_CWORD-1]}"
  if [[ $COMP_CWORD -eq 1 ]]; then
    COMPREPLY=( $(compgen -W "stats watch doctor spawn list-tiers list-models completions" -- "$cur") )
    return
  fi
  case "$prev" in
    --tiers) COMPREPLY=( $(compgen -W "$(sage list-tiers | tr '
' ' ')" -- "$cur") ) ;;
    --task) COMPREPLY=() ;;
    completions) COMPREPLY=( $(compgen -W "bash zsh fish" -- "$cur") ) ;;
  esac
}
complete -F _sage_complete sage
"#;
    let fish = r#"complete -c sage -f
complete -c sage -n '__fish_use_subcommand' -a 'stats watch doctor spawn list-tiers list-models completions'
complete -c sage -n '__fish_seen_subcommand_from completions' -a 'bash zsh fish'
complete -c sage -n '__fish_seen_subcommand_from spawn' -l task -r
complete -c sage -n '__fish_seen_subcommand_from spawn' -l tiers -r -a '(sage list-tiers)'
complete -c sage -n '__fish_seen_subcommand_from spawn' -l auto
complete -c sage -n '__fish_seen_subcommand_from spawn' -l dry-run
"#;
    let script = match shell {
        "zsh" => zsh,
        "bash" => bash,
        "fish" => fish,
        other => {
            eprintln!("sage: unknown shell {other} (bash|zsh|fish)");
            return;
        }
    };
    if !install {
        print!("{script}");
        return;
    }
    let home = home();
    match shell {
        "zsh" => {
            let dir = home.join(".zfunc");
            let _ = fs::create_dir_all(&dir);
            let path = dir.join("_sage");
            if let Err(e) = fs::write(&path, script) {
                eprintln!("sage: {e}");
                return;
            }
            println!("installed {}", path.display());
            println!("add to ~/.zshrc if missing:");
            println!("  fpath+=~/.zfunc");
            println!("  autoload -Uz compinit && compinit");
        }
        "bash" => {
            let path = home.join(".local/share/bash-completion/completions/sage");
            let _ = fs::create_dir_all(path.parent().unwrap());
            if let Err(e) = fs::write(&path, script) {
                eprintln!("sage: {e}");
                return;
            }
            println!("installed {}", path.display());
        }
        _ => {
            let path = home.join(".config/fish/completions/sage.fish");
            let _ = fs::create_dir_all(path.parent().unwrap());
            if let Err(e) = fs::write(&path, script) {
                eprintln!("sage: {e}");
                return;
            }
            println!("installed {}", path.display());
        }
    }
}

fn apply() -> Result<(), String> {
    let bench = load_benchmarks(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let mut all = load_local();
    all.extend(load_hosted(Path::new(".")));
    let models = rank(all, &bench);
    let availability = load_availability(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let (oc_ladder, ow_ladder) = build_ladders(&models, &availability);
    let jev_path = home().join(".config/jev/config.yaml");
    let text = fs::read_to_string(&jev_path).map_err(|e| format!("{}: {e}", jev_path.display()))?;
    let head_src = match text.find("tiers:") {
        Some(i) => &text[..i],
        None => text.as_str(),
    };
    let mut kept: Vec<&str> = head_src
        .lines()
        .filter(|l| !l.trim_start().starts_with("# Generated by Sage"))
        .collect();
    while kept.last().map(|l| l.trim().is_empty()).unwrap_or(false) {
        kept.pop();
    }
    let head = kept.join("\n") + "\n\n";
    fs::write(&jev_path, head + &jev_yaml_with(&oc_ladder)).map_err(|e| format!("{}: {e}", jev_path.display()))?;
    println!("applied ladder -> {}", jev_path.display());

    let oc_path = home().join(".openclaw/openclaw.json");
    let raw = fs::read_to_string(&oc_path).map_err(|e| format!("{}: {e}", oc_path.display()))?;
    let mut v: Value = serde_json::from_str(&raw).map_err(|e| format!("openclaw.json: {e}"))?;
    let tiers: Value = serde_json::from_str::<Value>(&openclaw_json_with(&ow_ladder))
        .map_err(|e| format!("ladder: {e}"))?
        .get("tiers")
        .cloned()
        .unwrap_or_else(|| json!({}));
    v["plugins"]["entries"]["jev-router"]["config"]["tiers"] = tiers;
    fs::write(&oc_path, serde_json::to_string_pretty(&v).unwrap() + "\n")
        .map_err(|e| format!("{}: {e}", oc_path.display()))?;
    println!("applied ladder -> {}", oc_path.display());
    println!("restart consumers: opencode service restart && openclaw plugins reload jev-router");
    Ok(())
}

fn probe(models_file: &str, limit: usize, all: bool) -> Result<(), String> {
    let raw = fs::read_to_string(models_file).map_err(|e| format!("{models_file}: {e}"))?;
    let models: Vec<Model> = serde_json::from_str(&raw).map_err(|e| format!("models.json: {e}"))?;
    let targets: Vec<&Model> = if all {
        models.iter().collect()
    } else {
        let bench = load_benchmarks(Path::new("."));
        let mut all = load_local();
        all.extend(load_hosted(Path::new(".")));
        let ranked = rank(all, &bench);
        let (ladder, _) = build_ladders(&ranked, &load_availability(Path::new(".")));
        let ladder_models: Vec<String> = ladder.iter().map(|r| r.model.clone()).collect();
        models
            .iter()
            .filter(|m| ladder_models.iter().any(|name| m.id.ends_with(&format!("/{name}"))))
            .take(limit)
            .collect()
    };
    let mut out = serde_json::Map::new();
    for m in targets {
        let id = m.id.clone();
        let started = std::time::Instant::now();
        let status = Command::new("timeout")
            .args(["45", "opencode", "run", "Reply with only: ok", "--model", &id])
            .output();
        let ok = match status {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).to_lowercase();
                o.status.success() && !text.contains("model is unavailable") && !text.contains("error:")
            }
            Err(_) => false,
        };
        println!("{} {} ({}ms)", if ok { "ok  " } else { "FAIL" }, id, started.elapsed().as_millis());
        out.insert(id, json!({"ok": ok, "checked_at": chrono_like()}));
    }
    let doc = json!({"checked_at": chrono_like(), "models": out});
    fs::write("availability.json", serde_json::to_string_pretty(&doc).unwrap() + "
")
        .map_err(|e| format!("availability.json: {e}"))?;
    println!("wrote availability.json");
    Ok(())
}

fn chrono_like() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format!("{now}")
}

fn tail_lines(path: &str, limit: usize) -> Vec<String> {
    let Ok(text) = fs::read_to_string(path) else {
        return vec![];
    };
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.contains("apply:") || l.contains("bypass:") || l.contains("active:"))
        .map(|l| l.to_string())
        .collect();
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }
    lines
}

fn watch() {
    use std::io::{Read, Write};
    use std::sync::mpsc;

    let (tx, rx) = mpsc::channel::<u8>();
    std::thread::spawn(move || {
        let mut buf = [0u8; 1];
        let mut stdin = std::io::stdin();
        while let Ok(1) = stdin.read(&mut buf) {
            let _ = tx.send(buf[0]);
            if buf[0] == b'q' {
                break;
            }
        }
    });

    let mut stdout = std::io::stdout();
    loop {
        if let Ok(key) = rx.try_recv() {
            if key == b'q' || key == 3 {
                break;
            }
        }
        let router = tail_lines("/tmp/jev-router.log", 200);
        let oc = tail_lines("/tmp/opencode-jev.log", 200);
        let mut frame = String::new();
        frame.push_str("\x1b[2J\x1b[H");
        frame.push_str("\x1b[1mSage watch\x1b[0m   q to quit\n\n");
        for (label, lines) in [("opencode", &oc), ("OpenClaw", &router)] {
            let decisions: Vec<&String> = lines.iter().filter(|l| l.contains("apply:") || l.contains("bypass:")).collect();
            let applied = decisions.iter().filter(|l| l.contains("apply:")).count();
            let rate = if decisions.is_empty() { 0.0 } else { applied as f64 / decisions.len() as f64 * 100.0 };
            frame.push_str(&format!(
                "\x1b[1m{label}\x1b[0m  {} decisions  {} applied  {:.0}%\n",
                decisions.len(), applied, rate
            ));
            for line in decisions.iter().rev().take(5) {
                let trimmed = line.trim();
                let body = trimmed.split_once("] ").map(|(_, b)| b).unwrap_or(trimmed);
                let colour = if body.contains("apply:") { "\x1b[32m" } else { "\x1b[90m" };
                frame.push_str(&format!("  {colour}{}\x1b[0m\n", &body[..body.len().min(150)]));
            }
            frame.push('\n');
        }
        frame.push_str("\x1b[90mlogs: /tmp/opencode-jev.log  /tmp/jev-router.log\x1b[0m\n");
        let _ = stdout.write_all(frame.as_bytes());
        let _ = stdout.flush();
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
    let _ = stdout.write_all(b"\x1b[0m\n");
}

fn main() {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
    match cmd {
        "stats" => stats(),
        "watch" => watch(),
        "probe" => {
            let mut file = "models.json".to_string();
            let mut limit = 8usize;
            let mut all = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--models" => { i += 1; file = args.get(i).cloned().unwrap_or(file); }
                    "--limit" => { i += 1; limit = args.get(i).and_then(|v| v.parse().ok()).unwrap_or(limit); }
                    "--all" => all = true,
                    _ => {}
                }
                i += 1;
            }
            if let Err(e) = probe(&file, limit, all) {
                eprintln!("sage: {e}");
                std::process::exit(1);
            }
        }
        "list-tiers" => list_tiers(),
        "list-models" => list_models(),
        "completions" => {
            let shell = args.get(2).map(|s| s.as_str()).unwrap_or("zsh");
            let install = args.iter().any(|a| a == "--install");
            completions(shell, install);
        }
        "doctor" => doctor(),
        "apply" => {
            if let Err(e) = apply() {
                eprintln!("sage: {e}");
                std::process::exit(1);
            }
        }
        "spawn" => {
            let mut task = String::new();
            let mut tiers = String::from("cheap,strong,frontier");
            let mut dry = false;
            let mut auto = false;
            let mut i = 2;
            while i < args.len() {
                match args[i].as_str() {
                    "--task" => {
                        i += 1;
                        task = args.get(i).cloned().unwrap_or_default();
                    }
                    "--tiers" => {
                        i += 1;
                        tiers = args.get(i).cloned().unwrap_or_default();
                    }
                    "--dry-run" => dry = true,
                    "--auto" => auto = true,
                    _ => {}
                }
                i += 1;
            }
            if auto {
                let plan = auto_plan(&task);
                println!("auto: {} -> {}", plan.len(), plan.join(","));
                tiers = plan.join(",");
            }
            spawn(&task, &tiers, dry);
        }
        _ => {
            if let Err(e) = generate(&root) {
                let _ = writeln!(std::io::stderr(), "sage: {e}");
                std::process::exit(1);
            }
        }
    }
}

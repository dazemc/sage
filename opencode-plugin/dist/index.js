import { parse as parseYaml } from "yaml";
import { appendFileSync, renameSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { Plugin } from "@opencode/plugin";
const DEFAULT_API_URL = "https://api.typesafe.ai/v1/systemone";
const DEFAULT_MIN_CONFIDENCE = 0.75;
const DEFAULT_TIMEOUT_MS = 2000;
const DEFAULT_MODEL = "jev-latest";
// Sticky per-session model choice: switching models mid-session invalidates
// opencode's provider metadata and breaks stateful assistant history.
const SESSION_TTL_MS = 2 * 60 * 60 * 1000;
const sessionPicks = new Map();
// Snapshot budgets. Empirically, long state dilutes the decision: on the same
// question, 9 ledger entries scored 0.535 confidence where 3 entries scored
// 0.886. Short and recent beats long and complete.
const STATE_CHAR_BUDGET = 6000;
const TURN_CHAR_BUDGET = 1200;
const RECENT_TURNS = 3;
const TOOL_RESULT_CHARS = 300;
const TOOL_DESC_CHARS = 160;
// TypeSafe choice questions accept at most 255 criteria; one slot is reserved
// for respond_to_user.
const MAX_CRITERIA = 255;
// Explicit function reference for the Jev client, captured at module load.
const nativeFetch = globalThis.fetch.bind(globalThis);
// OpenCode's background service swallows plugin console output, so mirror
// every line to a grep-able file. JEV_DEBUG=1 additionally echoes to stdout
// (useful when running plugins in foreground).
// Capped at JEV_DEBUG_MAX_BYTES (default 256 KiB): when exceeded, the file
// is rotated once to <file>.1 and a fresh file starts. Prevents unbounded
// growth in long-lived background services.
const debugFile = process.env.JEV_DEBUG_FILE ?? "/tmp/opencode-jev.log";
const maxLogBytes = (() => {
    const raw = Number(process.env.JEV_DEBUG_MAX_BYTES);
    return Number.isFinite(raw) && raw > 0 ? raw : 256 * 1024;
})();
function log(...args) {
    const line = `[jev] ${args.map((arg) => String(arg)).join(" ")}`;
    if (process.env.JEV_DEBUG === "1")
        console.log(line);
    try {
        try {
            if (statSync(debugFile).size > maxLogBytes) {
                try {
                    renameSync(debugFile, `${debugFile}.1`);
                }
                catch {
                    // Fall through to plain append when rotation fails.
                }
            }
        }
        catch {
            // File does not exist yet — nothing to rotate.
        }
        appendFileSync(debugFile, `${new Date().toISOString()} ${line}\n`);
    }
    catch {
        // Never break a session over a log line.
    }
}
// Latched on the first 401/403 from the Jev API: the key is invalid, so all
// later requests skip the Jev round-trip entirely (zero added latency) and
// pass through untouched. Reset only by service restart.
let jevAuthFailed = false;
/* ------------------------------------------------------------------ config */
function parseConfigText(filePath, text) {
    try {
        const data = /\.ya?ml$/i.test(filePath)
            ? parseYaml(text)
            : JSON.parse(text);
        return typeof data === "object" && data !== null
            ? data
            : undefined;
    }
    catch {
        return undefined;
    }
}
async function fileConfig(filePath) {
    try {
        return parseConfigText(filePath, await readFile(filePath, "utf8"));
    }
    catch {
        return undefined;
    }
}
function expandHome(candidate) {
    return candidate === "~" || candidate.startsWith("~/")
        ? path.join(os.homedir(), candidate.slice(candidate === "~" ? 1 : 2))
        : candidate;
}
function envConfig() {
    const cfg = {};
    if (process.env.TYPESAFE_API_KEY)
        cfg.apiKey = process.env.TYPESAFE_API_KEY;
    if (process.env.JEV_API_URL)
        cfg.apiUrl = process.env.JEV_API_URL;
    if (process.env.JEV_MIN_CONFIDENCE)
        cfg.minConfidence = Number(process.env.JEV_MIN_CONFIDENCE);
    return cfg;
}
async function configLayers(options, baseDir) {
    const configRoot = process.env.XDG_CONFIG_HOME ?? path.join(os.homedir(), ".config");
    const files = [
        ...(process.env.JEV_CONFIG_PATH
            ? [expandHome(process.env.JEV_CONFIG_PATH)]
            : []),
        ...["jev.config.yaml", "jev.config.yml", "jev.config.json"].map((name) => path.join(baseDir, name)),
        ...["jev.yaml", "jev.json"].map((name) => path.join(baseDir, ".opencode", name)),
        ...["config.yaml", "config.json"].map((name) => path.join(configRoot, "jev", name)),
    ];
    const layers = [];
    for (const file of files) {
        const layer = await fileConfig(file);
        if (layer)
            layers.push(layer);
    }
    layers.push(options, envConfig());
    return layers;
}
function pickString(layers, key) {
    for (const layer of layers) {
        const value = layer[key];
        if (typeof value === "string" && value.trim())
            return value.trim();
    }
    return undefined;
}
function pickNumber(layers, key, validate = () => true) {
    for (const layer of layers) {
        const raw = layer[key];
        const value = typeof raw === "string" ? Number(raw) : raw;
        if (typeof value === "number" &&
            Number.isFinite(value) &&
            validate(value))
            return value;
    }
    return undefined;
}
function positiveIntFromEnv(name, fallback) {
    const value = Number(process.env[name]);
    return Number.isFinite(value) && value > 0 ? value : fallback;
}
function pickTiers(layers) {
    const out = [];
    for (const layer of layers) {
        const raw = layer.tiers;
        if (!raw || typeof raw !== "object" || Array.isArray(raw))
            continue;
        for (const [name, value] of Object.entries(raw)) {
            const spec = (typeof value === "object" && value !== null ? value : {});
            const model = typeof value === "string" ? value : typeof spec.model === "string" ? spec.model : undefined;
            if (!model)
                continue;
            if (out.some((tier) => tier.name === name))
                continue;
            const description = typeof spec.description === "string" && spec.description.trim()
                ? spec.description
                : `Use the ${name} model`;
            const rank = typeof spec.rank === "number" ? spec.rank : out.length;
            out.push({
                name,
                model,
                rank,
                description: description.replace(/\s+/g, " ").trim().slice(0, TOOL_DESC_CHARS),
            });
        }
        if (out.length > 0)
            break;
    }
    return out;
}
async function loadConfig(options, baseDir) {
    const layers = await configLayers(options, baseDir);
    const apiKey = pickString(layers, "apiKey");
    if (!apiKey)
        return undefined;
    const confidence = pickNumber(layers, "minConfidence") ?? DEFAULT_MIN_CONFIDENCE;
    const tierConfidence = pickNumber(layers, "tierMinConfidence") ?? confidence;
    return {
        apiKey,
        apiUrl: pickString(layers, "apiUrl") ?? DEFAULT_API_URL,
        minConfidence: Math.min(Math.max(confidence, 0), 1),
        tierMinConfidence: Math.min(Math.max(tierConfidence, 0), 1),
        timeoutMs: pickNumber(layers, "timeoutMs", (v) => v > 0) ??
            positiveIntFromEnv("JEV_TIMEOUT_MS", DEFAULT_TIMEOUT_MS),
        model: pickString(layers, "model") ?? process.env.JEV_MODEL ?? DEFAULT_MODEL,
        tiers: pickTiers(layers),
    };
}
/* ------------------------------------------------------ request inspection */
function targetOf(url) {
    let parsed;
    try {
        parsed = new URL(url);
    }
    catch {
        return undefined;
    }
    const { hostname, pathname } = parsed;
    if (hostname === "api.anthropic.com" || pathname.endsWith("/v1/messages")) {
        return "anthropic";
    }
    // OpenAI Responses API (/v1/responses). Checked before the api.openai.com
    // host catch-all: that host also serves this different wire format.
    if (pathname.endsWith("/responses")) {
        return "responses";
    }
    if (hostname === "api.openai.com" || pathname.endsWith("/chat/completions")) {
        return "openai";
    }
    return undefined;
}
// Responses-API built-ins (web_search, file_search, namespace groups, ...):
// typed entries that are not plain function tools. Chat/Anthropic tools carry
// no `type` and are NOT built-ins.
function isBuiltIn(tool) {
    if (typeof tool !== "object" || tool === null)
        return false;
    const type = tool.type;
    return type !== undefined && type !== "function";
}
function toolNames(tools) {
    const names = [];
    for (const tool of tools) {
        if (typeof tool !== "object" || tool === null)
            continue;
        // Responses API built-ins cannot be pinned via tool_choice, so only
        // plain function tools become candidates.
        if (isBuiltIn(tool))
            continue;
        const record = tool;
        const name = typeof record.name === "string"
            ? record.name
            : typeof record.function?.name === "string"
                ? record.function.name
                : undefined;
        if (name && !names.includes(name))
            names.push(name);
    }
    return names;
}
function toolDescriptions(tools) {
    const out = {};
    for (const tool of tools) {
        if (typeof tool !== "object" || tool === null || isBuiltIn(tool))
            continue;
        const record = tool;
        const name = typeof record.name === "string"
            ? record.name
            : typeof record.function?.name === "string"
                ? record.function.name
                : undefined;
        if (!name)
            continue;
        const raw = typeof record.description === "string"
            ? record.description
            : typeof record.function?.description === "string"
                ? record.function.description
                : "";
        const desc = raw.replace(/\s+/g, " ").trim();
        if (desc)
            out[name] = desc.slice(0, TOOL_DESC_CHARS);
    }
    return out;
}
function textOf(content) {
    if (typeof content === "string")
        return content;
    if (Array.isArray(content))
        return content.map(textOf).join(" ");
    if (typeof content === "object" && content !== null) {
        const record = content;
        if (typeof record.text === "string")
            return record.text;
        if ("content" in record)
            return textOf(record.content);
    }
    return "";
}
function partKind(part) {
    if (typeof part !== "object" || part === null)
        return "";
    const type = part.type;
    return typeof type === "string" ? type : "";
}
function summarizeValue(value, limit) {
    if (value === undefined || value === null)
        return "";
    let out;
    if (typeof value === "string") {
        out = value;
    }
    else {
        try {
            out = JSON.stringify(value) ?? String(value);
        }
        catch {
            out = String(value);
        }
    }
    return out.replace(/\s+/g, " ").trim().slice(0, limit);
}
function toolNameOfPart(part) {
    const name = part.toolName ?? part.name ?? part.tool_name ?? part.toolCallId;
    return typeof name === "string" && name.length > 0 ? name : "tool";
}
/**
 * One message as a compact ledger line: assistant text plus a marker for each
 * tool call and each tool result. Tool activity is what makes routing decidable
 * — without it the model sees the goal but never what just happened.
 */
function ledgerOf(message) {
    if (typeof message !== "object" || message === null)
        return null;
    const record = message;
    const type = record.type;
    if (type !== undefined && type !== "message")
        return null;
    const role = record.role;
    if (role !== "user" && role !== "assistant" && role !== "tool")
        return null;
    const pieces = [];
    const content = record.content;
    if (Array.isArray(content)) {
        for (const part of content) {
            const kind = partKind(part);
            const partRecord = (typeof part === "object" && part !== null ? part : {});
            if (kind === "text") {
                const text = textOf(part).slice(0, TURN_CHAR_BUDGET);
                if (text)
                    pieces.push(text);
            }
            else if (kind === "tool-call") {
                const input = summarizeValue(partRecord.input ?? partRecord.args ?? partRecord.arguments, 160);
                pieces.push(`\u2192 ${toolNameOfPart(partRecord)}(${input})`);
            }
            else if (kind === "tool-result") {
                const out = summarizeValue(partRecord.output ?? partRecord.result, TOOL_RESULT_CHARS);
                pieces.push(`\u2190 ${toolNameOfPart(partRecord)}${out ? `: ${out}` : ""}`);
            }
            else if (kind === "tool-error") {
                const err = summarizeValue(partRecord.error ?? partRecord.output, TOOL_RESULT_CHARS);
                pieces.push(`\u2190 ${toolNameOfPart(partRecord)} ERROR${err ? `: ${err}` : ""}`);
            }
            else if (kind === "reasoning" ||
                kind === "step-start" ||
                kind === "step-finish" ||
                kind === "snapshot") {
                continue;
            }
            else {
                const fallback = textOf(part).slice(0, TURN_CHAR_BUDGET);
                if (fallback)
                    pieces.push(fallback);
            }
        }
    }
    else {
        const text = textOf(content).slice(0, TURN_CHAR_BUDGET);
        if (text)
            pieces.push(text);
    }
    // Legacy OpenAI shape: tool calls ride on the assistant message itself.
    if (Array.isArray(record.tool_calls)) {
        for (const call of record.tool_calls) {
            const callRecord = (typeof call === "object" && call !== null ? call : {});
            const fn = (typeof callRecord.function === "object" && callRecord.function !== null
                ? callRecord.function
                : callRecord);
            const name = typeof fn.name === "string" && fn.name ? fn.name : "tool";
            const input = summarizeValue(fn.arguments ?? callRecord.input, 160);
            pieces.push(`\u2192 ${name}(${input})`);
        }
    }
    const joined = pieces.join(" ").replace(/\s+/g, " ").trim();
    if (!joined)
        return null;
    return { role: String(role), text: joined.slice(0, TURN_CHAR_BUDGET + TOOL_RESULT_CHARS) };
}
function stateOf(payload) {
    // Anthropic/OpenAI chat carry `messages`; the Responses API carries `input`
    // items of type "message".
    const conversation = Array.isArray(payload.messages)
        ? payload.messages
        : Array.isArray(payload.input)
            ? payload.input
            : [];
    const turns = conversation
        .map(ledgerOf)
        .filter((turn) => turn !== null);
    const recent = turns.slice(-RECENT_TURNS);
    const firstUser = turns.find((turn) => turn.role === "user");
    const picked = firstUser && !recent.includes(firstUser) ? [firstUser, ...recent] : recent;
    return JSON.stringify(picked).slice(0, STATE_CHAR_BUDGET);
}
async function askJev(cfg, state, names, descriptions = {}) {
    const tierCriteria = Object.fromEntries(cfg.tiers.map((tier) => [tier.name, tier.description]));
    if (names.length > MAX_CRITERIA - 1)
        log(`note: ${names.length} tools exceed criteria cap ${MAX_CRITERIA - 1} — first ${MAX_CRITERIA - 1} offered to jev`);
    const criteria = {
        respond_to_user: "Task is complete, or clarification from user is required",
    };
    for (const name of names.slice(0, MAX_CRITERIA - 1))
        criteria[name] = descriptions[name] ?? `Use tool ${name}`;
    const response = await nativeFetch(cfg.apiUrl, {
        method: "POST",
        headers: {
            "content-type": "application/json",
            authorization: `Bearer ${cfg.apiKey}`,
        },
        body: JSON.stringify({
            state,
            model: cfg.model,
            questions: {
                next_tool: {
                    type: "choice",
                    instructions: "Which tool should the coding agent execute next?",
                    criteria,
                },
                ...(cfg.tiers.length > 0
                    ? {
                        model_tier: {
                            type: "choice",
                            instructions: "Which model tier should execute this step? Pick the cheapest tier that can handle it.",
                            criteria: tierCriteria,
                        },
                    }
                    : {}),
            },
        }),
        signal: AbortSignal.timeout(cfg.timeoutMs),
    });
    if (!response.ok) {
        // Invalid key: latch off so every later request passes through with no
        // added latency. Any other status is transient — keep trying.
        if (response.status === 401 || response.status === 403) {
            jevAuthFailed = true;
            log(`jev http ${response.status} (invalid key, routing disabled)`);
        }
        else {
            log(`jev http ${response.status}`);
        }
        return undefined;
    }
    const payload = (await response.json());
    const answer = payload.answers?.next_tool;
    if (!answer ||
        answer.type !== "choice" ||
        typeof answer.choice !== "string" ||
        typeof answer.confidence !== "number" ||
        !Number.isFinite(answer.confidence)) {
        log("jev answer malformed");
        return undefined;
    }
    const tierAnswer = payload.answers?.model_tier;
    const tier = tierAnswer &&
        tierAnswer.type === "choice" &&
        typeof tierAnswer.choice === "string" &&
        typeof tierAnswer.confidence === "number" &&
        Number.isFinite(tierAnswer.confidence) &&
        cfg.tiers.some((candidate) => candidate.name === tierAnswer.choice)
        ? {
            name: tierAnswer.choice,
            confidence: tierAnswer.confidence,
            probabilities: tierAnswer.probabilities && typeof tierAnswer.probabilities === "object"
                ? tierAnswer.probabilities
                : undefined,
        }
        : undefined;
    return { choice: answer.choice, confidence: answer.confidence, tier };
}
/**
 * Map Jev's model_tier answer onto one rung.
 *
 * Confidence alone cannot index a 20-rung ladder: with 20 options a decisive
 * answer is ~0.27 because the mass is split 20 ways, so a 0.35 threshold would
 * pin every request to the default. Instead the whole distribution votes:
 * the expected rung is sum(p_i * rank_i) / sum(p_i). Confidence only gates
 * whether the answer is trustworthy at all (50% above uniform).
 */
function pickRung(tiers, tier) {
    const ordered = [...tiers].sort((a, b) => (a.rank ?? 0) - (b.rank ?? 0));
    if (ordered.length === 0) {
        return { rung: undefined, why: "no rungs" };
    }
    const gate = 1.5 / ordered.length;
    const confidence = Number(tier.confidence);
    if (!Number.isFinite(confidence) || confidence < gate) {
        return { rung: undefined, why: `${tier.name}@${confidence} below gate ${gate.toFixed(3)}` };
    }
    const probs = tier.probabilities;
    if (probs) {
        let weighted = 0;
        let mass = 0;
        ordered.forEach((rung, index) => {
            const p = Number(probs[rung.name]);
            if (Number.isFinite(p) && p > 0) {
                weighted += p * index;
                mass += p;
            }
        });
        if (mass > 0) {
            const expected = weighted / mass;
            const index = Math.max(0, Math.min(ordered.length - 1, Math.round(expected)));
            return {
                rung: ordered[index],
                why: `expected rung ${expected.toFixed(2)} of ${ordered.length - 1}, p1 ${confidence.toFixed(3)}`,
            };
        }
    }
    const step = Math.floor((confidence - gate) / 0.05);
    const rung = ordered[Math.max(0, Math.min(ordered.length - 1, step))];
    return { rung, why: `${tier.name}@${confidence} band ${step} (no distribution)` };
}

/* --------------------------------------------------- interception core */
async function routePayload(cfg, target, payload, sessionId) {
    const names = toolNames(payload.tools);
    // opencode rejects historical reasoning_details when the model changes
    // protocol. The field is optional for replay, so drop it from history on
    // every routed request; the current turn's reasoning is unaffected.
    let stripped = 0;
    // History arrives as `messages` (chat/Anthropic) or `input` (Responses API).
    // Reading only `messages` left Responses-API payloads untouched.
    const history = Array.isArray(payload.messages)
        ? payload.messages
        : Array.isArray(payload.input)
            ? payload.input
            : [];
    for (const message of history) {
        if (message && typeof message === "object" && "reasoning_details" in message) {
            delete message.reasoning_details;
            stripped += 1;
        }
    }
    if (stripped > 0) {
        log(`stripped reasoning_details from ${stripped} history message(s)`);
    }
    // Forked sessions (and any session that resumed reasoning) make opencode emit
    // composite reasoning ids like `rs_a:rs_b`, which the Responses API rejects
    // outright: "Expected an ID that contains letters, numbers, underscores, or
    // dashes". The id is only a local correlation key for replay, so drop the
    // reasoning items carrying one and let the model continue without them.
    let droppedComposite = 0;
    for (const item of history) {
        if (!item || typeof item !== "object") continue;
        if (typeof item.id === "string" && item.id.includes(":")) {
            item.__jevComposite = true;
            droppedComposite += 1;
        }
    }
    if (droppedComposite > 0 && Array.isArray(payload.input)) {
        payload.input = payload.input.filter((item) => !item || !item.__jevComposite);
        log(`dropped ${droppedComposite} history item(s) with composite reasoning ids`);
    } else if (droppedComposite > 0 && Array.isArray(payload.messages)) {
        payload.messages = payload.messages.filter((item) => !item || !item.__jevComposite);
        log(`dropped ${droppedComposite} history message(s) with composite reasoning ids`);
    }
    // No usable candidate, or a lone candidate: trimming can only reproduce the
    // original tools array — built-ins always pass through untouched — so there
    // is no decision for Jev and the round-trip would be pure added latency.
    if (names.length <= 1) {
        return false;
    }
    // Invalid key latched earlier: skip the round-trip, zero added latency.
    if (jevAuthFailed)
        return false;
    // A thrown round-trip (fetch network error, timeout abort, non-JSON body)
    // must still end in the tagged "bypass:" line — log contract §5.
    let answer;
    try {
        answer = await askJev(cfg, stateOf(payload), names, toolDescriptions(payload.tools));
    }
    catch (error) {
        log(`jev round-trip threw: ${error}`);
    }
    if (!answer) {
        log("bypass: jev unavailable (see preceding 'jev ...' error line) — full tool list sent untouched");
        return false;
    }
    // Model tier is an independent question answered in the same round-trip:
    // it stays useful even when the tool decision is below threshold.
    let changed = false;
    if (answer.tier) {
        const current = typeof payload.model === "string" ? payload.model : undefined;
        const now = Date.now();
        let sticky = sessionId ? sessionPicks.get(sessionId) : undefined;
        if (sticky && now - sticky.at > SESSION_TTL_MS) {
            sessionPicks.delete(sessionId);
            sticky = undefined;
        }
        if (sticky && current !== sticky.baseModel) {
            // User changed the session model explicitly; decide again from the new base.
            sessionPicks.delete(sessionId);
            sticky = undefined;
        }
        if (sticky) {
            if (current !== undefined && current !== sticky.model) {
                payload.model = sticky.model;
                changed = true;
                log(`sticky: model ${current} -> ${sticky.model} (session ${sessionId})`);
            }
        } else {
            // Responses-API providers (gpt-6-luna, codex-family) carry history in
            // `input`, not `messages`. Reading only `messages` left this guard dead
            // for them, so every turn looked like a fresh session and the model
            // switched mid-conversation — which makes opencode emit composite
            // reasoning ids (`rs_a:rs_b`) that the Responses API rejects.
            const historyLen = Array.isArray(payload.messages)
                ? payload.messages.length
                : Array.isArray(payload.input)
                    ? payload.input.length
                    : 0;
            if (current !== undefined && historyLen > 2) {
                // Never switch an established session: opencode stores provider
                // metadata per message and a late switch breaks history replay.
                if (sessionId) {
                    sessionPicks.set(sessionId, { baseModel: current, model: current, at: now });
                }
                log(`bypass: existing history (${historyLen} messages) — model ${current} kept`);
            } else {
                const pick = pickRung(cfg.tiers, answer.tier);
                if (pick.rung && current !== undefined && answer.tier.confidence >= cfg.tierMinConfidence) {
                    if (current !== pick.rung.model) {
                        // Drop provider-specific reasoning metadata so the new model can
                        // replay the history without a protocol mismatch.
                        for (const message of Array.isArray(payload.messages)
                            ? payload.messages
                            : Array.isArray(payload.input)
                                ? payload.input
                                : []) {
                            if (message && typeof message === "object") {
                                delete message.reasoning_details;
                                delete message.provider_metadata;
                            }
                        }
                        payload.model = pick.rung.model;
                        changed = true;
                        log(`apply: model ${current} -> ${pick.rung.model} (${pick.why}, session ${sessionId ?? "unknown"})`);
                    }
                    if (sessionId) {
                        sessionPicks.set(sessionId, { baseModel: current, model: pick.rung.model, at: now });
                    }
                } else if (pick.rung) {
                    log(`bypass: tier ${pick.rung.name} p1 ${answer.tier.confidence} below tierMinConfidence ${cfg.tierMinConfidence}`);
                }
            }
        }
    }
    const at = `${answer.choice}@${answer.confidence}`;
    if (answer.choice === "respond_to_user") {
        log(`bypass: jev says answer directly (${at}) — nothing to trim to, full tool list sent untouched`);
        return changed;
    }
    if (!names.includes(answer.choice)) {
        log(`bypass: jev choice ${at} is not among this request's tools — full tool list sent untouched`);
        return changed;
    }
    if (answer.confidence < cfg.minConfidence) {
        log(`bypass: jev choice ${at} below minConfidence ${cfg.minConfidence} — full tool list sent untouched`);
        return changed;
    }
    // Force by trimming the candidate set to the chosen tool instead of pinning
    // via tool_choice: providers reject forced tool_choice in thinking mode
    // (verified live: HTTP 400), while a single offered tool is always valid.
    // Side benefit: dropped tool schemas shrink the request substantially.
    // tool_choice itself is left exactly as OpenCode sent it.
    const tools = payload.tools.filter((tool) => {
        if (typeof tool !== "object" || tool === null)
            return false;
        const record = tool;
        // Invariant 4 (issue #12 verdict): built-ins are never offered to Jev,
        // so Jev must never remove them either — they always survive the trim.
        if (isBuiltIn(tool))
            return true;
        return (record.name === answer.choice || record.function?.name === answer.choice);
    });
    if (tools.length === 0) {
        log(`bypass: jev choice ${at} listed but missing from payload — full tool list sent untouched`);
        return changed;
    }
    payload.tools = tools;
    changed = true;
    const keptBuiltIns = tools.filter(isBuiltIn).length;
    log(`apply: tools trimmed to ${answer.choice}${keptBuiltIns
        ? ` (+ ${keptBuiltIns} built-in${keptBuiltIns === 1 ? "" : "s"} kept)`
        : " only"} (${at} >= minConfidence ${cfg.minConfidence})`);
    return changed;
}
function eligible(payload) {
    if (!Array.isArray(payload.tools) || payload.tools.length === 0)
        return false;
    const choice = payload.tool_choice;
    return (choice === undefined ||
        choice === null ||
        (typeof choice === "string" && choice !== "none"));
}
/* --------------------------------------------- transport: http.request hook */
// Rebuild a provider Request preserving everything except content-length,
// which is stale once the body changes (the transport recomputes it).
function rebuildRequest(request, body) {
    const headers = new Headers(request.headers);
    headers.delete("content-length");
    return new Request(request.url, {
        method: request.method,
        headers,
        body,
    });
}
async function handleRequest(cfg, request) {
    const target = targetOf(request.url);
    if (!target || request.method.toUpperCase() !== "POST" || !request.body) {
        return request;
    }
    // Provider bodies are one-shot streams: once the read starts, every path
    // returns a rebuilt readable Request — the original comes back only when
    // the read itself fails, in which case the body was unreadable anyway.
    let text;
    try {
        text = await request.text();
    }
    catch (error) {
        log(`bypass: body read failed (${error}) — passing original request through`);
        return request;
    }
    try {
        const payload = JSON.parse(text);
        if (payload && typeof payload === "object" && eligible(payload)) {
            const sessionId = request.headers.get("x-opencode-session-id") ?? request.headers.get("x-opencode-session") ?? undefined;
            const forced = await routePayload(cfg, target, payload, sessionId);
            return rebuildRequest(request, forced ? JSON.stringify(payload) : text);
        }
    }
    catch (error) {
        log(`intercept failed, falling back to untouched request: ${error}`);
    }
    try {
        return rebuildRequest(request, text);
    }
    catch (fallbackError) {
        log(`rebuild fallback failed (${fallbackError}) — passing original through`);
        return request;
    }
}
/* ------------------------------------------------------------- entrypoint */
export default Plugin.define({
    id: "jev",
    async setup(ctx) {
        const options = (ctx.options ?? {});
        const baseDir = ctx.location.directory ?? process.cwd();
        const cfg = await loadConfig(options, baseDir);
        if (!cfg) {
            // Fully transparent when unconfigured: no hook registered, zero
            // overhead, and no log file touched (console only under JEV_DEBUG).
            if (process.env.JEV_DEBUG === "1")
                console.log("[jev] disabled: no apiKey found in any config source");
            return;
        }
        await ctx.session.hook("http.request", async (event) => {
            // Agent-loop calls only; title/compaction/generate requests have no
            // tool decision to save.
            if (event.kind !== "primary")
                return;
            try {
                event.request = await handleRequest(cfg, event.request);
            }
            catch (error) {
                log(`http.request hook failed, request untouched: ${error}`);
            }
        });
        log(`active: ${cfg.apiUrl} minConfidence=${cfg.minConfidence}`);
    },
});
//# sourceMappingURL=index.js.map
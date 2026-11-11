# dist/ is the source of truth — do not rebuild blindly

`dist/index.js` is hand-patched and is **ahead of** `index.ts`. Running
`npm run build` (tsc) overwrites `dist/` from the stale TypeScript source and
silently deletes the following, all of which are live in production:

| Feature | Present in `dist/index.js` | Present in `index.ts` |
| --- | --- | --- |
| `expected rung` tier selection | yes | **no** |
| `sticky` per-session model pin + `SESSION_TTL_MS` | yes | **no** |
| Responses-API `input` history support | yes | partial |
| `historyLen` established-session guard | yes | **no** |
| composite-reasoning-id drop | yes | yes (unbuilt) |

## Before any rebuild

Diff `dist/index.js` against the tsc output and re-apply every row above, or
port `index.ts` up to parity first. `dist/` is committed to git, so
`git diff` after a build shows exactly what a rebuild destroyed.

## The two bugs this file guards

1. **Dead established-session guard.** The tier switcher counted
   `payload.messages` only. Responses-API models (`gpt-6-luna`, the codex
   family) send history in `payload.input`, so `historyLen` was always `0`,
   every turn looked like a fresh session, and the model switched
   mid-conversation. Fixed by reading `messages` then `input`.

2. **Composite reasoning ids.** A forked session (`opencode run --fork`) makes
   opencode emit reasoning ids like `rs_a:rs_b`, which the Responses API
   rejects: `Expected an ID that contains letters, numbers, underscores, or
   dashes`. The plugin drops those history items before the request leaves.

   Note: the remaining `--fork` failure
   (`Expected an ID that begins with 'msg'`) is **opencode's own bug** — it
   reproduces with this plugin fully disabled, so it cannot be fixed here.

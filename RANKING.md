# Sage model ranking

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

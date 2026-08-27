# xConsole Plugin: Autonomous AI Agent Engine 🤖

Official plugin for **xConsole** providing autonomous multi-provider AI Agent integration (DeepSeek, Claude, OpenAI, Ollama, Google Gemini) with dynamic tool execution, file diff approval gates, and runtime self-composition.

---

## 🚀 1-Command Installation

```bash
xconsole plugin install xconsole-plugins/xconsole-plugin-agent
```

Or from URL:
```bash
xconsole plugin install https://github.com/xconsole-plugins/xconsole-plugin-agent
```

---

## ✨ Features & Capabilities

- **Multi-Provider Hub**: Stream tokens and tool calls from DeepSeek, Anthropic Claude, OpenAI, Local Ollama, and Google Gemini.
- **Safety Gate & Verification**: Plan diff approval, interactive questions, and command dry-runs.
- **Cordis Spatiotemporal Tool Injection**: Automatically acquires tools from any active plugin mounted in xConsole.
- **Autonomous Plugin Self-Composition**: Discovers and installs community plugins mid-task when encountering new challenges.

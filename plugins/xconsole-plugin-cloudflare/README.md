# xConsole Plugin: Cloudflare Zero Trust & Security ☁️

Official plugin for **xConsole** providing complete integration with **Cloudflare Zero Trust (Tunnels)**, **DNS Management**, and **WAF Security & Attack Mode**, backed by automatic audit logging and 1-click rollback.

---

## 🚀 1-Command Installation

You can install this plugin directly inside xConsole via the command line or from the in-app Plugin Manager:

```bash
xconsole plugin install xconsole-plugins/xconsole-plugin-cloudflare
```

Or from URL:
```bash
xconsole plugin install https://github.com/xconsole-plugins/xconsole-plugin-cloudflare
```

---

## ✨ Features & Capabilities

### 1. 🛡️ Zero Trust Tunnels (cloudflared)
- Create and manage Cloudflare Tunnels right from your desktop.
- Configure Ingress rules (forwarding public hostnames to local ports `http://localhost:3000` or VPS services).
- Generate instant Tunnel tokens with one click for Docker or systemd daemons.

### 2. 🌐 DNS Zone Management
- List, create, modify, and delete `A`, `AAAA`, `CNAME`, `TXT`, and `MX` records.
- Toggle Cloudflare Proxy (Orange Cloud ☁️) on any record.

### 3. 🔒 WAF & Under Attack Mode
- Inspect and configure security levels (`Essentially Off`, `Low`, `Medium`, `High`, `I'm Under Attack`).
- Emergency 1-click "Under Attack" activation during DDoS incidents.

### 4. 📜 Audit Log & 1-Click Rollback
- Every change performed by either the **AI Agent** or the **User** is recorded with full before/after state snapshots.
- Revert any erroneous change instantly with the **`[ ↩️ Revert / Anulează ]`** button.

### 5. 🤖 AI Agent Tool Suite
The plugin exposes 6 specialized tools to the xConsole AI Agent:
- `cloudflare_list_tunnels`: List all Zero Trust tunnels.
- `cloudflare_list_dns`: Query DNS records for a domain.
- `cloudflare_upsert_dns`: Add or update DNS routing.
- `cloudflare_set_security_level`: Adjust WAF protection levels.
- `cloudflare_get_history`: Inspect recent changes.
- `cloudflare_revert_action`: Automatically rollback bad configs if healthchecks fail.

---

## 🛠️ Developing Plugins for xConsole

Built with the **xConsole Plugin SDK**. See the [Plugin Developer Documentation](https://github.com/DemOnJR/xConsole#plugins) to create your own plugins!

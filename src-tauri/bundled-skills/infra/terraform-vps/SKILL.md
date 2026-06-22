---
description: Create and apply minimal Terraform on a VPS runner (Phase 1). Load meta/ponytail first.
---

# Terraform on VPS (xConsole Phase 1)

Use this playbook for infra tasks before writing HCL. **Load `meta/ponytail` first.**

## Flow

1. `project_list` — see existing projects.
2. `project_create` — template `blank` or `vps-web`; set `default_vps_id` to the runner VPS.
3. `project_write` / `project_read` — edit `.tf` files locally (minimal HCL only).
4. `terraform_init` → `terraform_plan` → user approves → `terraform_apply`.

Projects sync to `$HOME/xconsole-projects/<slug>/` on the runner VPS. Terraform runs there via SSH.

## Rules (ponytail + safety)

- Prefer **one provider, few resources** — no module tree until asked.
- **VPS bootstrap**: `vps-web` template uses `null_resource` + `remote-exec`; expand only when needed.
- **plan before apply** always; summarize plan changes for the user.
- `terraform apply` requires user approval unless safety mode is `full`.
- Never put secrets in `.tf` files; use variables + env (future: TFC/AWS/GCP backends).

## Future (not Phase 1)

- AWS/GCP: add provider blocks + credentials via keychain (not SQLite).
- Terraform Cloud: remote backend + `tfc_*` tools.
- Mixed targets: cloud resources + VPS provisioners in one project only when the user asks.

## Example ask

> "Deploy nginx on my VPS with Terraform"

1. Create project `vps-web` with the selected VPS as runner.
2. `terraform plan` — show diff.
3. Wait for approval.
4. `terraform apply` with `-auto-approve` only after explicit user OK.

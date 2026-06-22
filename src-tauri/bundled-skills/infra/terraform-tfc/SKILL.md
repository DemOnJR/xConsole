---
description: Terraform Cloud remote backend, upload, and remote runs.
---

# Terraform Cloud

1. Add a **TFC** cloud account with organization name + API token (keychain).
2. `tfc_list_workspaces` with the account id to see workspaces.
3. `project_create` with `backend: tfc`, `cloud_account_id`, and:
   `config_json: {"tfc_org":"MY_ORG","tfc_workspace":"my-workspace"}`.
4. `terraform_init` runs locally to validate the backend block.
5. `terraform_plan` / `terraform_apply` **upload config to TFC and queue a remote run** (no VPS required).
6. Poll with `tfc_run_status` using the returned `run_id`.

State and execution live in Terraform Cloud. The desktop only uploads `.tf` files and triggers runs via API.

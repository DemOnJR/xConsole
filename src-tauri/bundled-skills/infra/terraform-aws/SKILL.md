---
description: Minimal AWS Terraform — one resource, ponytail style. Load meta/ponytail first.
---

# Terraform AWS (minimal)

1. User adds an **AWS** cloud account in Settings → Cloud (access key + secret in keychain).
2. `cloud_account_list` to find the account id.
3. **Before planning:** `cloud_list_resources` with `resource: all` to see existing S3 buckets and EC2 instances.
4. `project_create` with `template: aws-minimal`, `cloud_account_id`, `config_json: {"aws_region":"eu-west-1"}`.
5. `terraform_init` → `terraform_plan` → approve → `terraform_apply`.

Use `runner: local` when Terraform is installed on the desktop (no VPS required). Otherwise use a runner VPS.

Credentials are injected at run time — never in `.tf` files or SQLite.

Add resources only when the user asks; start from the single S3 bucket template.

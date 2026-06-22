---
description: Minimal GCP Terraform — one GCS bucket. Load meta/ponytail first.
---

# Terraform GCP (minimal)

1. Add a **GCP** cloud account (service account JSON in keychain) with `project_id` set.
2. **Before planning:** `cloud_list_resources` with `resource: gcs_buckets`.
3. `project_create` with `template: gcp-minimal`, `cloud_account_id`, `config_json: {"gcp_region":"us-central1"}`.
4. Standard init → plan → apply (`runner: local` or VPS runner).

GCP credentials are written to a temp file on the runner/host; `GOOGLE_APPLICATION_CREDENTIALS` is set for that run only.

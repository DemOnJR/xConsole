//! Local Terraform project directories under the agent home.

use std::path::{Component, Path, PathBuf};

use crate::ai::AgentHome;
use crate::storage::models::{InfraProject, InfraProjectInput};

pub fn projects_root(home: &AgentHome) -> PathBuf {
    home.projects_dir()
}

pub fn project_dir(home: &AgentHome, slug: &str) -> PathBuf {
    projects_root(home).join(slugify(slug))
}

/// Safe relative path inside a project (blocks `..` escapes).
pub fn resolve_project_file(home: &AgentHome, slug: &str, rel: &str) -> Result<PathBuf, String> {
    let base = project_dir(home, slug);
    let joined = base.join(rel.trim_start_matches('/'));
    let norm = normalize(&joined);
    let base_norm = normalize(&base);
    if !norm.starts_with(&base_norm) {
        return Err("path escapes project directory".into());
    }
    Ok(norm)
}

fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn slugify(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn write_project_file(
    home: &AgentHome,
    slug: &str,
    rel: &str,
    content: &str,
) -> Result<(), String> {
    let path = resolve_project_file(home, slug, rel)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

pub fn read_project_file(home: &AgentHome, slug: &str, rel: &str) -> Result<String, String> {
    let path = resolve_project_file(home, slug, rel)?;
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

pub fn list_project_files(home: &AgentHome, slug: &str) -> Result<Vec<String>, String> {
    let root = project_dir(home, slug);
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    walk_files(&root, &root, &mut out)?;
    out.sort();
    Ok(out)
}

fn walk_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().and_then(|n| n.to_str()) == Some(".terraform") {
                continue;
            }
            walk_files(base, &path, out)?;
        } else if path.is_file() {
            if let Ok(rel) = path.strip_prefix(base) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    Ok(())
}

/// Scaffold a new project on disk. Returns the slug used.
pub fn scaffold(home: &AgentHome, input: &InfraProjectInput) -> Result<String, String> {
    let slug = input
        .slug
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(slugify)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| slugify(&input.name));
    if slug.is_empty() {
        return Err("project name must contain at least one letter or digit".into());
    }
    let dir = project_dir(home, &slug);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let template = input.template.as_deref().unwrap_or("blank");
    match template {
        "vps-web" => write_vps_web_template(home, &slug)?,
        "aws-minimal" => write_aws_minimal_template(home, &slug, input)?,
        "gcp-minimal" => write_gcp_minimal_template(home, &slug, input)?,
        _ => write_blank_template(home, &slug)?,
    }
    if input.backend.as_deref() == Some("tfc") {
        write_tfc_backend(home, &slug, input)?;
    }
    Ok(slug)
}

fn write_blank_template(home: &AgentHome, slug: &str) -> Result<(), String> {
    write_project_file(
        home,
        slug,
        "main.tf",
        r#"# ponytail: minimal starter — add only what this environment needs.
terraform {
  required_version = ">= 1.5"
}
"#,
    )?;
    write_project_file(
        home,
        slug,
        "variables.tf",
        r#"variable "environment" {
  type    = string
  default = "dev"
}
"#,
    )?;
    write_project_file(
        home,
        slug,
        ".gitignore",
        ".terraform/\n*.tfstate\n*.tfstate.*\n.terraform.lock.hcl\n",
    )
}

fn write_vps_web_template(home: &AgentHome, slug: &str) -> Result<(), String> {
    write_blank_template(home, slug)?;
    write_project_file(
        home,
        slug,
        "main.tf",
        r#"# ponytail: nginx via remote-exec — no cloud provider until you need one.
terraform {
  required_version = ">= 1.5"
  required_providers {
    null = {
      source  = "hashicorp/null"
      version = "~> 3.2"
    }
  }
}

variable "vps_host" { type = string }
variable "vps_user" { type = string }
variable "vps_port" { type = number default = 22 }

resource "null_resource" "web" {
  connection {
    type = "ssh"
    host = var.vps_host
    user = var.vps_user
    port = var.vps_port
  }

  provisioner "remote-exec" {
    inline = [
      "command -v nginx >/dev/null || (sudo apt-get update -qq && sudo DEBIAN_FRONTEND=noninteractive apt-get install -y nginx)",
      "sudo systemctl enable --now nginx",
    ]
  }
}
"#,
    )
}

fn config_str(input: &InfraProjectInput, key: &str, default: &str) -> String {
    input
        .config_json
        .as_deref()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
        .and_then(|v| v.get(key).and_then(|x| x.as_str()).map(String::from))
        .unwrap_or_else(|| default.to_string())
}

fn write_tfc_backend(home: &AgentHome, slug: &str, input: &InfraProjectInput) -> Result<(), String> {
    let org = config_str(input, "tfc_org", "ORG");
    let ws = config_str(input, "tfc_workspace", slug);
    write_project_file(
        home,
        slug,
        "backend.tf",
        &format!(
            r#"# ponytail: remote state on Terraform Cloud — no local state file.
terraform {{
  cloud {{
    organization = "{org}"
    workspaces {{
      name = "{ws}"
    }}
  }}
}}
"#
        ),
    )
}

fn write_aws_minimal_template(home: &AgentHome, slug: &str, input: &InfraProjectInput) -> Result<(), String> {
    write_blank_template(home, slug)?;
    let region = config_str(input, "aws_region", "us-east-1");
    write_project_file(
        home,
        slug,
        "main.tf",
        &format!(
            r#"# ponytail: one S3 bucket — expand only when asked.
terraform {{
  required_version = ">= 1.5"
  required_providers {{
    aws = {{ source = "hashicorp/aws", version = "~> 5.0" }}
    random = {{ source = "hashicorp/random", version = "~> 3.6" }}
  }}
}}

provider "aws" {{
  region = var.aws_region
}}

variable "aws_region" {{
  type    = string
  default = "{region}"
}}

resource "random_id" "suffix" {{
  byte_length = 4
}}

resource "aws_s3_bucket" "app" {{
  bucket = "xconsole-${{random_id.suffix.hex}}"
}}
"#
        ),
    )
}

fn write_gcp_minimal_template(home: &AgentHome, slug: &str, input: &InfraProjectInput) -> Result<(), String> {
    write_blank_template(home, slug)?;
    let region = config_str(input, "gcp_region", "us-central1");
    write_project_file(
        home,
        slug,
        "main.tf",
        &format!(
            r#"# ponytail: one GCS bucket — expand only when asked.
terraform {{
  required_version = ">= 1.5"
  required_providers {{
    google = {{ source = "hashicorp/google", version = "~> 5.0" }}
    random = {{ source = "hashicorp/random", version = "~> 3.6" }}
  }}
}}

provider "google" {{
  region = var.gcp_region
}}

variable "gcp_region" {{
  type    = string
  default = "{region}"
}}

resource "random_id" "suffix" {{
  byte_length = 4
}}

resource "google_storage_bucket" "app" {{
  name     = "xconsole-${{random_id.suffix.hex}}"
  location = var.gcp_region
}}
"#
        ),
    )
}

pub fn format_project_list(projects: &[InfraProject]) -> String {
    if projects.is_empty() {
        return "no infra projects yet".into();
    }
    projects
        .iter()
        .map(|p| {
            let runner = p
                .default_vps_id
                .as_deref()
                .unwrap_or("no default runner");
            let cloud = p
                .cloud_account_id
                .as_deref()
                .unwrap_or("no cloud account");
            format!(
                "{} (slug: {}, template: {}, backend: {}, runner: {}, cloud: {})",
                p.name, p.slug, p.template, p.backend, runner, cloud
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Web App"), "my-web-app");
    }

    #[test]
    fn blocks_path_traversal() {
        let home = AgentHome::new(std::env::temp_dir().join("xconsole-test-projects"));
        let slug = "test";
        let _ = std::fs::create_dir_all(project_dir(&home, slug));
        assert!(resolve_project_file(&home, slug, "../secret").is_err());
    }
}

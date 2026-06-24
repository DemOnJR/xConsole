//! Tauri commands for Terraform / IaC projects.

use tauri::State;

use crate::ai::AgentHome;
use crate::infra::projects::{project_dir, project_slug, scaffold, slugify};
use crate::storage::models::{InfraProject, InfraProjectInput};
use crate::storage::Db;

#[tauri::command]
pub fn list_infra_projects(db: State<'_, Db>) -> Result<Vec<InfraProject>, String> {
    db.list_infra_projects().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn save_infra_project(
    home: State<'_, AgentHome>,
    db: State<'_, Db>,
    input: InfraProjectInput,
) -> Result<InfraProject, String> {
    let slug = if input.id.as_ref().is_some_and(|id| !id.is_empty()) {
        db.get_infra_project(input.id.as_ref().unwrap())
            .map_err(|e| e.to_string())?
            .map(|p| p.slug)
            .ok_or_else(|| "project not found".to_string())?
    } else {
        // New project: refuse before scaffolding if the slug is taken, so we
        // never overwrite an existing project's files.
        let slug = project_slug(&input)?;
        if db.get_infra_project(&slug).map_err(|e| e.to_string())?.is_some() {
            return Err(format!("a project named '{slug}' already exists"));
        }
        scaffold(&home, &input)?
    };
    db.upsert_infra_project(&input, &slug)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_infra_project(
    home: State<'_, AgentHome>,
    db: State<'_, Db>,
    id: String,
) -> Result<(), String> {
    if let Ok(Some(p)) = db.get_infra_project(&id) {
        let dir = project_dir(&home, &p.slug);
        if dir.exists() {
            std::fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
    }
    db.delete_infra_project(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_infra_project(db: State<'_, Db>, id: String) -> Result<Option<InfraProject>, String> {
    db.get_infra_project(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn read_project_file_cmd(
    home: State<'_, AgentHome>,
    slug: String,
    path: String,
) -> Result<String, String> {
    crate::infra::projects::read_project_file(&home, &slugify(&slug), &path)
}

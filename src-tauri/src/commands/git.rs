use tauri::Manager;

use crate::error::AppError;
use crate::git::diff::{DiffResult, DiffSourceArgs};
use crate::git::repository::validate_repo_path;

#[tauri::command]
pub async fn get_diff(
    app: tauri::AppHandle,
    repo_path: String,
    source: DiffSourceArgs,
) -> Result<DiffResult, AppError> {
    let root = validate_repo_path(&repo_path).await?;
    let shadow_base = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::Internal {
            message: format!("failed to resolve app data dir: {e}"),
        })?
        .join("shadow");
    crate::git::diff::get_diff(&root, &source, &shadow_base).await
}

use crate::error::AppError;
use crate::git::diff::{DiffResult, DiffSourceArgs};
use crate::git::repository::validate_repo_path;

#[tauri::command]
pub async fn get_diff(repo_path: String, source: DiffSourceArgs) -> Result<DiffResult, AppError> {
    let root = validate_repo_path(&repo_path).await?;
    crate::git::diff::get_diff(&root, &source).await
}

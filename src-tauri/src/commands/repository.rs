use crate::error::AppError;
use crate::git::repository::RepositoryInfo;

#[tauri::command]
pub async fn open_repository(path: String) -> Result<RepositoryInfo, AppError> {
    crate::git::repository::open_repository(&path).await
}

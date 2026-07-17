mod commands;
mod error;
mod git;
mod github;
#[cfg(test)]
mod test_support;

use tauri_plugin_sql::{Migration, MigrationKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let migrations = vec![
        Migration {
            version: 1,
            description: "initial schema",
            sql: include_str!("../migrations/001_initial.sql"),
            kind: MigrationKind::Up,
        },
        Migration {
            version: 2,
            description: "github import",
            sql: include_str!("../migrations/002_github.sql"),
            kind: MigrationKind::Up,
        },
    ];

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_sql::Builder::default()
                .add_migrations("sqlite:lgtm.db", migrations)
                .build(),
        )
        .manage(crate::github::device::DeviceFlowManager::default())
        .invoke_handler(tauri::generate_handler![
            commands::repository::open_repository,
            commands::git::get_diff,
            commands::github::github_set_token,
            commands::github::github_token_status,
            commands::github::github_clear_token,
            commands::github::github_open_pr,
            commands::github::github_submit_review,
            commands::github::github_import_review_comments,
            commands::github::github_list_pull_requests,
            commands::github::github_merge_pr,
            commands::github::github_set_pr_state,
            commands::github::github_list_reviews,
            commands::github::github_dismiss_review,
            commands::github::github_list_pr_comments,
            commands::github::github_delete_review_comment,
            commands::github::github_add_conversation_comment,
            commands::github::github_list_conversation_comments,
            commands::github::github_pr_ci_status,
            commands::github::github_device_start,
            commands::github::github_device_wait,
            commands::github::github_device_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

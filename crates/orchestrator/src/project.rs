//! Per-repository todo numbering: every todo reads as `L-3`, a prefix taken
//! from the repository's name and a number that only ever goes up.

use std::collections::HashSet;

use lgtm_protocol::Project;

use crate::state::{random_id, State};

/// The longest a prefix may be, so a display id stays something a person can
/// say out loud.
pub const PREFIX_MAX: usize = 8;

/// Last path segment of a git URL without its `.git`, which is what people
/// call the repository. `None` is the bucket for todos tied to no repository.
pub fn project_name(repository: Option<&str>) -> String {
    let Some(repository) = repository else {
        return "general".to_string();
    };
    let name = repository
        .trim_end_matches('/')
        .rsplit(['/', ':'])
        .next()
        .unwrap_or(repository)
        .trim_end_matches(".git");
    match name.is_empty() {
        true => "general".to_string(),
        false => name.to_string(),
    }
}

/// Growing prefixes of `name`'s uppercased ASCII letters — `L`, `LG`, `LGT` —
/// and the first one nobody holds wins. When the whole name is taken the
/// number of the collision is appended, so `lgtm` becomes `LGTM2`.
pub fn derive_prefix(name: &str, taken: &HashSet<String>) -> String {
    let letters: String = name
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_uppercase())
        .take(PREFIX_MAX)
        .collect();
    // A repository named in digits alone still has to start from a letter.
    let base = match letters.is_empty() {
        true => "P".to_string(),
        false => letters,
    };
    if let Some(prefix) = (1..=base.len())
        .map(|take| base[..take].to_string())
        .find(|candidate| !taken.contains(candidate))
    {
        return prefix;
    }
    // Distinct candidates against a finite `taken`, so this always lands.
    (2..)
        .map(|n: u64| {
            let suffix = n.to_string();
            let keep = PREFIX_MAX.saturating_sub(suffix.len()).min(base.len());
            format!("{}{suffix}", &base[..keep])
        })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or(base)
}

impl State {
    fn new_project_id(&self) -> String {
        std::iter::repeat_with(random_id)
            .find(|id| !self.projects.contains_key(id))
            .unwrap_or_default()
    }

    /// Id of `repository`'s project, created on demand: a project exists
    /// because a todo needed a number, so nothing else has to make one.
    pub fn project_for(&mut self, repository: Option<&str>) -> String {
        let existing = self
            .projects
            .values()
            .find(|project| project.repository.as_deref() == repository)
            .map(|project| project.id.clone());
        if let Some(id) = existing {
            return id;
        }
        let name = project_name(repository);
        let taken: HashSet<String> = self
            .projects
            .values()
            .map(|project| project.prefix.clone())
            .collect();
        let project = Project {
            id: self.new_project_id(),
            repository: repository.map(str::to_string),
            prefix: derive_prefix(&name, &taken),
            name,
            next_number: 1,
        };
        let id = project.id.clone();
        tracing::info!(project = %id, prefix = %project.prefix, "project created");
        self.projects.insert(id.clone(), project);
        self.mark_project(&id);
        id
    }

    fn mark_project(&mut self, id: &str) {
        if !self.dirty_projects.iter().any(|dirty| dirty == id) {
            self.dirty_projects.push(id.to_string());
        }
    }

    /// The number the next todo in `repository` takes, bumping the project's
    /// counter past it.
    pub fn take_number(&mut self, repository: Option<&str>) -> u64 {
        let id = self.project_for(repository);
        let project = self.projects.get_mut(&id).expect("just looked up");
        let number = project.next_number;
        project.next_number += 1;
        self.mark_project(&id);
        number
    }

    /// How a todo reads: `L-3`. The project is created on demand rather than
    /// falling back to a bare number — after the startup migration every todo
    /// has one, so this only fires for a project whose file went missing.
    pub fn display_id(&mut self, todo: &lgtm_protocol::Todo) -> String {
        let id = self.project_for(todo.repository.as_deref());
        let prefix = &self.projects[&id].prefix;
        format!("{prefix}-{}", todo.number)
    }

    /// Gives every todo written before numbering a number: grouped by
    /// repository, oldest first, appended after whatever that project has
    /// already handed out. Returns the ids it changed, for the caller to
    /// re-persist. Numbered todos are left alone, so a second startup is a
    /// no-op.
    pub fn number_legacy_todos(&mut self) -> Vec<String> {
        let mut repositories: Vec<Option<String>> = Vec::new();
        for todo in self.todos.values() {
            if !repositories.contains(&todo.repository) {
                repositories.push(todo.repository.clone());
            }
        }
        let mut changed = Vec::new();
        for repository in repositories {
            let mut legacy: Vec<(u64, String)> = self
                .todos
                .values()
                .filter(|todo| todo.repository == repository && todo.number == 0)
                .map(|todo| (todo.created_at, todo.id.clone()))
                .collect();
            let highest = self
                .todos
                .values()
                .filter(|todo| todo.repository == repository)
                .map(|todo| todo.number)
                .max()
                .unwrap_or(0);
            // Ids break a tie so two todos made in the same millisecond keep
            // the same order on every startup.
            legacy.sort();
            let id = self.project_for(repository.as_deref());
            let mut next = self.projects[&id].next_number.max(highest + 1);
            for (_, todo) in legacy {
                self.todos.get_mut(&todo).expect("just listed").number = next;
                changed.push(todo);
                next += 1;
            }
            let project = self.projects.get_mut(&id).expect("just looked up");
            if project.next_number != next {
                project.next_number = next;
                self.mark_project(&id);
            }
        }
        if !changed.is_empty() {
            tracing::info!(
                todos = changed.len(),
                "numbered todos from before numbering"
            );
        }
        changed
    }
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;

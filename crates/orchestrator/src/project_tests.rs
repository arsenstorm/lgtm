//! Unit tests for `project.rs`: prefixes, numbering, and the startup pass
//! that numbers todos written before numbering existed.

use super::*;

fn state() -> State {
    State::default()
}

fn taken(prefixes: &[&str]) -> HashSet<String> {
    prefixes.iter().map(|p| p.to_string()).collect()
}

#[test]
fn a_name_is_the_last_segment_without_its_git_suffix() {
    assert_eq!(
        project_name(Some("https://github.com/arsenstorm/lgtm.git")),
        "lgtm"
    );
    assert_eq!(
        project_name(Some("git@github.com:arsenstorm/lgtm.git")),
        "lgtm"
    );
    assert_eq!(project_name(Some("https://example.com/repo/")), "repo");
    assert_eq!(project_name(None), "general");
}

#[test]
fn a_prefix_grows_only_as_far_as_it_must_to_be_unique() {
    assert_eq!(derive_prefix("lgtm", &taken(&[])), "L");
    // Arsen's example: "lgtm" holds "L", so "LegalBase" takes "LE".
    assert_eq!(derive_prefix("LegalBase", &taken(&["L"])), "LE");
    assert_eq!(derive_prefix("LegalBase", &taken(&["L", "LE"])), "LEG");
    assert_eq!(derive_prefix("my-repo_2", &taken(&[])), "M");
}

#[test]
fn an_exhausted_name_takes_a_numbered_prefix() {
    let all = taken(&["L", "LG", "LGT", "LGTM"]);
    assert_eq!(derive_prefix("lgtm", &all), "LGTM2");
    let mut and_two = all.clone();
    and_two.insert("LGTM2".into());
    assert_eq!(derive_prefix("lgtm", &and_two), "LGTM3");
}

#[test]
fn a_prefix_stays_within_eight_letters() {
    let name = "averyverylongrepositoryname";
    let prefix = derive_prefix(name, &taken(&[]));
    assert_eq!(prefix, "A");
    let mut all: HashSet<String> = (1..=PREFIX_MAX)
        .map(|take| name[..take].to_ascii_uppercase())
        .collect();
    let next = derive_prefix(name, &all);
    assert_eq!(next.len(), PREFIX_MAX);
    all.insert(next.clone());
    assert!(!all.contains(&derive_prefix(name, &all)));
}

#[test]
fn numbers_run_per_project_and_prefixes_do_not_collide() {
    let mut state = state();
    let lgtm = state.create_todo(
        Some("https://github.com/arsenstorm/lgtm.git".into()),
        "first".into(),
        String::new(),
        None,
    );
    let second = state.create_todo(
        Some("https://github.com/arsenstorm/lgtm.git".into()),
        "second".into(),
        String::new(),
        None,
    );
    let legal = state.create_todo(
        Some("https://github.com/arsenstorm/LegalBase.git".into()),
        "elsewhere".into(),
        String::new(),
        None,
    );
    assert_eq!((lgtm.number, second.number, legal.number), (1, 2, 1));
    assert_eq!(state.display_id(&lgtm), "L-1");
    assert_eq!(state.display_id(&second), "L-2");
    assert_eq!(state.display_id(&legal), "LE-1");
    assert_eq!(state.projects.len(), 2);
}

#[test]
fn a_todo_without_a_repository_lands_in_the_general_project() {
    let mut state = state();
    let todo = state.create_todo(None, "t".into(), String::new(), None);
    assert_eq!(state.display_id(&todo), "G-1");
}

/// A todo as it was stored before numbering: `number` defaults to 0.
fn legacy(state: &mut State, repository: Option<&str>, created_at: u64) -> String {
    let todo = state.create_todo(
        repository.map(str::to_string),
        "old".into(),
        String::new(),
        None,
    );
    let id = todo.id.clone();
    state.todos.get_mut(&id).unwrap().number = 0;
    state.todos.get_mut(&id).unwrap().created_at = created_at;
    id
}

#[test]
fn the_startup_pass_numbers_legacy_todos_oldest_first_and_repeats_nothing() {
    let mut state = state();
    let repository = Some("https://github.com/arsenstorm/lgtm.git");
    let newer = legacy(&mut state, repository, 200);
    let older = legacy(&mut state, repository, 100);
    let elsewhere = legacy(&mut state, Some("https://example.com/other.git"), 50);
    // Reset the projects too: this is state as it comes off disk.
    state.projects.clear();
    state.dirty_projects.clear();

    let changed = state.number_legacy_todos();

    assert_eq!(changed.len(), 3);
    assert_eq!(state.todos[&older].number, 1);
    assert_eq!(state.todos[&newer].number, 2);
    assert_eq!(state.todos[&elsewhere].number, 1);
    assert_eq!(state.display_id(&state.todos[&older].clone()), "L-1");

    // A second startup finds nothing to do.
    state.dirty_projects.clear();
    assert!(state.number_legacy_todos().is_empty());
    assert_eq!(state.todos[&older].number, 1);
    assert_eq!(state.todos[&newer].number, 2);
    assert!(state.dirty_projects.is_empty());
}

#[test]
fn legacy_todos_are_numbered_after_the_ones_that_already_have_numbers() {
    let mut state = state();
    let repository = Some("https://github.com/arsenstorm/lgtm.git");
    let numbered = state.create_todo(
        repository.map(str::to_string),
        "new".into(),
        String::new(),
        None,
    );
    let old = legacy(&mut state, repository, 1);
    // The project survived the restart; only the todo predates numbering.
    state.dirty_projects.clear();

    state.number_legacy_todos();

    assert_eq!(state.todos[&numbered.id].number, 1);
    assert_eq!(state.todos[&old].number, 3);
    // The one the second `create_todo` burned is not handed out twice.
    assert_eq!(state.take_number(repository), 4);
}

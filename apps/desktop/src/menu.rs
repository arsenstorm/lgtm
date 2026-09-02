//! The context menu: what a right-click offers on a thread, a project, or the
//! Projects heading. The rows are ours; AppKit draws them (see `native`).

mod native;

use crate::app::LgtmApp;
use crate::tasks::repo_slug;
use crate::thread::ThreadAction;
use gpui::{Context, Pixels, Point, Window};

/// What was clicked.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Target {
    /// One thread, by session id.
    Thread(String),
    /// One project, by slug.
    Project(String),
    /// The Projects heading.
    Projects,
}

/// What a row does. Data rather than a closure, because AppKit hands the
/// choice back long after the menu was built.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Act {
    Rename(String),
    Archive(String),
    Unarchive(String),
    Delete(String),
    OpenProject(String),
    NewTask(String),
    Fold(String),
    Unfold(String),
    FoldAll,
    UnfoldAll,
    AddProject,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Item {
    Row {
        /// An SF Symbol name: the menu is the system's, so its icons are too.
        icon: &'static str,
        label: &'static str,
        act: Act,
    },
    Separator,
}

fn row(icon: &'static str, label: &'static str, act: Act) -> Item {
    Item::Row { icon, label, act }
}

/// The rows one target offers, in the order they are shown.
fn items(app: &LgtmApp, target: &Target) -> Vec<Item> {
    match target {
        Target::Thread(id) => thread_items(
            id,
            app.sessions
                .iter()
                .find(|open| &open.id == id)
                .is_some_and(|open| open.archived),
        ),
        Target::Project(slug) => project_items(slug, app.ui.collapsed.contains(slug)),
        Target::Projects => projects_items(),
    }
}

fn thread_items(id: &str, archived: bool) -> Vec<Item> {
    vec![
        row("pencil", "Rename", Act::Rename(id.into())),
        match archived {
            true => row("tray.and.arrow.up", "Unarchive", Act::Unarchive(id.into())),
            false => row("archivebox", "Archive", Act::Archive(id.into())),
        },
        Item::Separator,
        row("trash", "Delete", Act::Delete(id.into())),
    ]
}

fn project_items(slug: &str, collapsed: bool) -> Vec<Item> {
    vec![
        row("folder", "Open project", Act::OpenProject(slug.into())),
        row("square.and.pencil", "New task", Act::NewTask(slug.into())),
        Item::Separator,
        match collapsed {
            true => row("chevron.down", "Expand", Act::Unfold(slug.into())),
            false => row("chevron.right", "Collapse", Act::Fold(slug.into())),
        },
    ]
}

fn projects_items() -> Vec<Item> {
    vec![
        row("chevron.right", "Collapse all", Act::FoldAll),
        row("chevron.down", "Expand all", Act::UnfoldAll),
        Item::Separator,
        row("plus", "Add project", Act::AddProject),
    ]
}

fn run(app: &mut LgtmApp, act: Act, window: &mut Window, cx: &mut Context<LgtmApp>) {
    match act {
        Act::Rename(id) => app.open_thread_action(id, ThreadAction::Rename, window, cx),
        Act::Archive(id) => app.open_thread_action(id, ThreadAction::Archive, window, cx),
        // Unarchiving loses nothing, so it does not ask first.
        Act::Unarchive(id) => app.set_thread_archived(id, false, cx),
        Act::Delete(id) => app.open_thread_action(id, ThreadAction::Delete, window, cx),
        Act::OpenProject(slug) => app.open_project(slug, None, cx),
        // The composer, with the project already chosen: starting an empty
        // thread here would list a "New task" row nobody asked for.
        Act::NewTask(slug) => {
            app.composer.project = app
                .known_repositories()
                .into_iter()
                .find(|url| repo_slug(url) == slug);
            app.go_home(window, cx);
        }
        Act::Fold(slug) => {
            app.ui.collapsed.insert(slug);
            cx.notify();
        }
        Act::Unfold(slug) => {
            app.ui.collapsed.remove(&slug);
            cx.notify();
        }
        Act::FoldAll => {
            app.ui.collapsed.extend(crate::sidebar::project_slugs(app));
            cx.notify();
        }
        Act::UnfoldAll => {
            app.ui.collapsed.clear();
            cx.notify();
        }
        Act::AddProject => app.open_add_project(window, cx),
    }
}

impl LgtmApp {
    /// Opens the menu for `target` under the pointer.
    pub fn open_menu(
        &mut self,
        target: Target,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_menus(cx);
        let rows = items(self, &target);
        // AppKit tracks the menu in a run loop of its own, so it is opened
        // from a task: nothing of ours is borrowed while it blocks.
        cx.spawn_in(window, async move |this, cx| {
            let Some(act) = native::popup(&rows, at) else {
                return;
            };
            let _ = this.update_in(cx, |app, window, cx| run(app, act, window, cx));
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(items: &[Item]) -> Vec<&'static str> {
        items
            .iter()
            .filter_map(|item| match item {
                Item::Row { label, .. } => Some(*label),
                Item::Separator => None,
            })
            .collect()
    }

    #[test]
    fn a_thread_offers_the_way_back_out_of_the_archive() {
        assert_eq!(
            labels(&thread_items("s1", false)),
            vec!["Rename", "Archive", "Delete"]
        );
        assert_eq!(
            labels(&thread_items("s1", true)),
            vec!["Rename", "Unarchive", "Delete"]
        );
    }

    #[test]
    fn a_folded_project_offers_to_expand_it_and_an_open_one_to_fold_it() {
        assert_eq!(labels(&project_items("one", true))[2], "Expand");
        assert_eq!(labels(&project_items("one", false))[2], "Collapse");
    }
}

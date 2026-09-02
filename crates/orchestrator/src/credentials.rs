//! Who a workspace's commits are attributed to, and which credential pushes
//! them. Secrets live here and never travel in a protocol type; the runner is
//! told a name and an address, and handed one token per push.

use lgtm_protocol::{
    same_workspace, AuthMode, Authorship, CredentialKind, CredentialSummary, Identity,
    WorkspaceSettings,
};
use serde::{Deserialize, Serialize};

/// One git credential a workspace can push with.
///
/// `owner` is what separates the two scopings the workspace can ask for. A
/// human credential is always owned: an unowned one would say "anyone may
/// push as this person". An agent credential's owner is optional, and leaving
/// it off is what makes the agent shared by the whole workspace.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CredentialRecord {
    pub id: String,
    #[serde(default)]
    pub workspace: Option<String>,
    pub kind: CredentialKind,
    #[serde(default)]
    pub owner: Option<String>,
    /// The name and address that go on the commit.
    pub identity: Identity,
    /// A credential for pushing over https. Held here and shipped one push at
    /// a time; the runner never stores it.
    #[serde(default)]
    pub token: Option<String>,
    /// Path to an SSH key on the runner, used to sign and to push. The key
    /// itself never travels: it belongs to the machine that holds it.
    #[serde(default)]
    pub ssh_key: Option<String>,
}

/// A workspace's own settings. Absent means the defaults below, which are
/// what LGTM did before any of this was configurable.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq, Default)]
pub struct WorkspaceRecord {
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub mode: AuthMode,
    /// Name the agent as a co-author under `Human` mode. Off by default: a
    /// human-authored commit says one name unless the workspace asks for two.
    #[serde(default)]
    pub credit_agent: bool,
}

impl CredentialRecord {
    /// The record without its secret, which is all the API ever shows.
    pub fn summary(&self) -> CredentialSummary {
        CredentialSummary {
            id: self.id.clone(),
            workspace: self.workspace.clone(),
            kind: self.kind,
            owner: self.owner.clone(),
            identity: self.identity.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CredentialStore {
    #[serde(default)]
    pub workspaces: Vec<WorkspaceRecord>,
    #[serde(default)]
    pub credentials: Vec<CredentialRecord>,
}

/// What a task needs to commit and push: the names on the commit, and the
/// token that authorizes the push.
pub struct Resolved {
    pub authorship: Authorship,
    pub token: Option<String>,
}

impl CredentialStore {
    /// The workspace's settings as the API shows them.
    pub fn public_settings(&self, workspace: Option<&str>) -> WorkspaceSettings {
        let held = self.settings(workspace);
        WorkspaceSettings {
            mode: held.mode,
            credit_agent: held.credit_agent,
        }
    }

    pub fn settings(&self, workspace: Option<&str>) -> WorkspaceRecord {
        self.workspaces
            .iter()
            .find(|rec| same_workspace(rec.workspace.as_deref(), workspace))
            .cloned()
            .unwrap_or_default()
    }

    /// A person's own credential in this workspace. Always owned, so a user
    /// id is required to find one at all.
    pub fn human(&self, workspace: Option<&str>, user: &str) -> Option<&CredentialRecord> {
        self.credentials.iter().find(|rec| {
            rec.kind == CredentialKind::Human
                && same_workspace(rec.workspace.as_deref(), workspace)
                && rec.owner.as_deref() == Some(user)
        })
    }

    /// The agent credential this user's tasks push with: their own if they
    /// registered one, else the workspace's shared agent.
    pub fn agent(&self, workspace: Option<&str>, user: Option<&str>) -> Option<&CredentialRecord> {
        let in_workspace = |rec: &&CredentialRecord| {
            rec.kind == CredentialKind::Agent && same_workspace(rec.workspace.as_deref(), workspace)
        };
        let owned = user.and_then(|user| {
            self.credentials
                .iter()
                .find(|rec| in_workspace(rec) && rec.owner.as_deref() == Some(user))
        });
        owned.or_else(|| {
            self.credentials
                .iter()
                .find(|rec| in_workspace(rec) && rec.owner.is_none())
        })
    }

    /// The names and the token for a task raised by `user` in `workspace`.
    /// `others` are further people who worked on it, e.g. by sending a
    /// follow-up; each is credited by their own agent, or by themselves.
    ///
    /// Every name here comes from a registered credential. A harness is not a
    /// GitHub account, so naming one in a trailer would attribute nothing.
    pub fn resolve(
        &self,
        workspace: Option<&str>,
        user: Option<&str>,
        others: &[String],
    ) -> Resolved {
        let settings = self.settings(workspace);
        let (author, token, signing_key) = match settings.mode {
            AuthMode::Human => self.pick(self.human_of(workspace, user)),
            AuthMode::Agent => self.pick(self.agent(workspace, user)),
        };
        let mut co_authors = Vec::new();
        match settings.mode {
            // The agent that did the work, if the workspace asks for it.
            AuthMode::Human if settings.credit_agent => {
                co_authors.extend(self.agent(workspace, user).map(identity_of));
            }
            AuthMode::Human => {}
            // The person the work was done for.
            AuthMode::Agent => {
                co_authors.extend(self.human_of(workspace, user).map(identity_of));
            }
        }
        for other in others {
            let helper = self
                .agent(workspace, Some(other))
                .or_else(|| self.human(workspace, other));
            co_authors.extend(helper.map(identity_of));
        }
        Resolved {
            authorship: Authorship {
                co_authors: without(dedup(co_authors), &author),
                author,
                signing_key,
            },
            token,
        }
    }

    fn human_of(&self, workspace: Option<&str>, user: Option<&str>) -> Option<&CredentialRecord> {
        user.and_then(|user| self.human(workspace, user))
    }

    /// The identity, token and signing key of a credential, or the name LGTM
    /// has always used when the workspace has registered nothing.
    fn pick(&self, held: Option<&CredentialRecord>) -> (Identity, Option<String>, Option<String>) {
        match held {
            Some(rec) => (rec.identity.clone(), rec.token.clone(), rec.ssh_key.clone()),
            None => (Identity::anonymous(), None, None),
        }
    }
}

fn identity_of(rec: &CredentialRecord) -> Identity {
    rec.identity.clone()
}

/// Nobody is their own co-author.
fn without(identities: Vec<Identity>, author: &Identity) -> Vec<Identity> {
    identities
        .into_iter()
        .filter(|it| it.email != author.email)
        .collect()
}

fn dedup(identities: Vec<Identity>) -> Vec<Identity> {
    let mut out: Vec<Identity> = Vec::new();
    for identity in identities {
        if !out.iter().any(|it| it.email == identity.email) {
            out.push(identity);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(name: &str) -> Identity {
        Identity {
            name: name.to_string(),
            email: format!("{name}@example.com"),
        }
    }

    fn credential(
        id: &str,
        kind: CredentialKind,
        owner: Option<&str>,
        name: &str,
    ) -> CredentialRecord {
        CredentialRecord {
            id: id.to_string(),
            workspace: Some("w".to_string()),
            kind,
            owner: owner.map(str::to_string),
            identity: identity(name),
            token: Some(format!("token-{id}")),
            ssh_key: None,
        }
    }

    fn store(
        mode: AuthMode,
        credit_agent: bool,
        credentials: Vec<CredentialRecord>,
    ) -> CredentialStore {
        CredentialStore {
            workspaces: vec![WorkspaceRecord {
                workspace: Some("w".to_string()),
                mode,
                credit_agent,
            }],
            credentials,
        }
    }

    #[test]
    fn an_owned_agent_credential_shadows_the_shared_one() {
        let store = store(
            AuthMode::Agent,
            false,
            vec![
                credential("shared", CredentialKind::Agent, None, "team-bot"),
                credential("mine", CredentialKind::Agent, Some("u1"), "arsen-bot"),
            ],
        );
        assert_eq!(store.agent(Some("w"), Some("u1")).unwrap().id, "mine");
        // A teammate with no agent of their own falls back to the shared one.
        assert_eq!(store.agent(Some("w"), Some("u2")).unwrap().id, "shared");
        assert_eq!(store.agent(Some("w"), None).unwrap().id, "shared");
    }

    #[test]
    fn a_human_credential_is_never_shared() {
        let store = store(
            AuthMode::Human,
            false,
            vec![credential("h", CredentialKind::Human, Some("u1"), "arsen")],
        );
        assert!(store.human(Some("w"), "u1").is_some());
        // No fallback: another user does not get to push as this person.
        assert!(store.human(Some("w"), "u2").is_none());
    }

    #[test]
    fn human_mode_credits_the_agent_only_when_asked() {
        let held = vec![
            credential("h", CredentialKind::Human, Some("u1"), "arsen"),
            credential("a", CredentialKind::Agent, Some("u1"), "arsenstorm2"),
        ];

        let quiet = store(AuthMode::Human, false, held.clone());
        let resolved = quiet.resolve(Some("w"), Some("u1"), &[]);
        assert_eq!(resolved.authorship.author, identity("arsen"));
        assert!(resolved.authorship.co_authors.is_empty());
        assert_eq!(resolved.token.as_deref(), Some("token-h"));

        // The co-author is the agent's own account, not the harness that ran.
        let crediting = store(AuthMode::Human, true, held);
        let resolved = crediting.resolve(Some("w"), Some("u1"), &[]);
        assert_eq!(
            resolved.authorship.co_authors,
            vec![identity("arsenstorm2")]
        );
    }

    #[test]
    fn agent_mode_authors_as_the_agents_account_and_credits_the_human() {
        let store = store(
            AuthMode::Agent,
            false,
            vec![
                credential("h", CredentialKind::Human, Some("u1"), "arsen"),
                credential("a", CredentialKind::Agent, Some("u1"), "arsenstorm2"),
            ],
        );
        let resolved = store.resolve(Some("w"), Some("u1"), &[]);
        assert_eq!(resolved.authorship.author, identity("arsenstorm2"));
        assert_eq!(resolved.authorship.co_authors, vec![identity("arsen")]);
        assert_eq!(resolved.token.as_deref(), Some("token-a"));
    }

    #[test]
    fn a_second_person_on_the_task_is_credited_by_their_own_agent() {
        let store = store(
            AuthMode::Agent,
            false,
            vec![
                credential("h1", CredentialKind::Human, Some("u1"), "arsen"),
                credential("a1", CredentialKind::Agent, Some("u1"), "arsenstorm2"),
                credential("a2", CredentialKind::Agent, Some("u2"), "mira-bot"),
            ],
        );
        // One author, two co-authors: the person it was done for, and the
        // teammate whose agent joined.
        let resolved = store.resolve(Some("w"), Some("u1"), &["u2".to_string()]);
        assert_eq!(resolved.authorship.author, identity("arsenstorm2"));
        assert_eq!(
            resolved.authorship.co_authors,
            vec![identity("arsen"), identity("mira-bot")]
        );
    }

    #[test]
    fn a_teammate_with_no_agent_is_credited_as_themselves() {
        let store = store(
            AuthMode::Agent,
            false,
            vec![
                credential("a1", CredentialKind::Agent, Some("u1"), "arsenstorm2"),
                credential("h2", CredentialKind::Human, Some("u2"), "mira"),
            ],
        );
        let resolved = store.resolve(Some("w"), Some("u1"), &["u2".to_string()]);
        assert_eq!(resolved.authorship.co_authors, vec![identity("mira")]);
    }

    #[test]
    fn nobody_is_their_own_co_author() {
        // One person, one account, credited twice by two routes.
        let store = store(
            AuthMode::Agent,
            false,
            vec![credential("a", CredentialKind::Agent, None, "team-bot")],
        );
        let resolved = store.resolve(Some("w"), Some("u1"), &["u2".to_string()]);
        assert_eq!(resolved.authorship.author, identity("team-bot"));
        assert!(resolved.authorship.co_authors.is_empty());
    }

    #[test]
    fn an_unconfigured_workspace_commits_the_way_lgtm_always_did() {
        let store = CredentialStore::default();
        let resolved = store.resolve(None, Some("u1"), &[]);
        assert_eq!(resolved.authorship.author, Identity::anonymous());
        assert!(resolved.authorship.co_authors.is_empty());
        assert!(resolved.token.is_none());
    }
}

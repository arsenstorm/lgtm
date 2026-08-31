//! Per-user tokens beside the shared one: who a request came from.

use lgtm_protocol::User;
use serde::{Deserialize, Serialize};

use crate::state::{now_ms, State};

/// A user and the tokens that authenticate as them. Written to
/// `<data_dir>/users.json` as a whole; users are few and change rarely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserRecord {
    pub user: User,
    pub tokens: Vec<String>,
}

impl State {
    /// Mints a token for `name`, returned once here and never listed again.
    /// Logging in again under the same name adds a token to the same user
    /// instead of minting a duplicate, so revoking that user cuts off every
    /// token they were ever given.
    pub fn create_user(&mut self, name: &str) -> (User, String) {
        let token = crate::token::generate_token();
        if let Some(rec) = self
            .users
            .values_mut()
            .find(|rec| rec.user.name == name && !rec.user.revoked)
        {
            rec.tokens.push(token.clone());
            return (rec.user.clone(), token);
        }
        let user = User {
            id: self.new_user_id(),
            name: name.to_string(),
            created_at: now_ms(),
            revoked: false,
        };
        self.users.insert(
            user.id.clone(),
            UserRecord {
                user: user.clone(),
                tokens: vec![token.clone()],
            },
        );
        (user, token)
    }

    /// The user a per-user token belongs to; `None` for the shared token,
    /// a revoked user, or a stranger.
    pub fn user_for_token(&self, token: &str) -> Option<&User> {
        self.users
            .values()
            .find(|rec| !rec.user.revoked && rec.tokens.iter().any(|t| t == token))
            .map(|rec| &rec.user)
    }

    /// Marks the user revoked, keeping the record for attribution.
    pub fn revoke_user(&mut self, id: &str) -> Option<User> {
        let rec = self.users.get_mut(id)?;
        rec.user.revoked = true;
        Some(rec.user.clone())
    }

    fn new_user_id(&self) -> String {
        std::iter::repeat_with(crate::state::random_id)
            .find(|id| !self.users.contains_key(id))
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minted_token_resolves_to_its_user() {
        let mut state = State::default();
        let (user, token) = state.create_user("alice");
        assert_eq!(state.user_for_token(&token), Some(&user));
    }

    #[test]
    fn the_shared_or_unknown_token_resolves_to_no_user() {
        let mut state = State::default();
        state.create_user("alice");
        assert_eq!(state.user_for_token("not-a-token"), None);
    }

    #[test]
    fn a_revoked_users_token_stops_resolving() {
        let mut state = State::default();
        let (user, token) = state.create_user("alice");
        let revoked = state.revoke_user(&user.id).unwrap();
        assert!(revoked.revoked);
        assert_eq!(state.user_for_token(&token), None);
    }

    #[test]
    fn logging_in_again_adds_a_token_to_the_same_user() {
        let mut state = State::default();
        let (first, old_token) = state.create_user("alice");
        let (second, new_token) = state.create_user("alice");
        assert_eq!(first.id, second.id);
        assert_ne!(old_token, new_token);
        assert_eq!(state.user_for_token(&old_token), Some(&first));
        assert_eq!(state.user_for_token(&new_token), Some(&first));
        state.revoke_user(&first.id).unwrap();
        assert_eq!(state.user_for_token(&old_token), None);
        assert_eq!(state.user_for_token(&new_token), None);
    }

    #[test]
    fn two_users_keep_distinct_tokens() {
        let mut state = State::default();
        let (alice, alice_token) = state.create_user("alice");
        let (bob, bob_token) = state.create_user("bob");
        assert_ne!(alice_token, bob_token);
        assert_eq!(state.user_for_token(&alice_token), Some(&alice));
        assert_eq!(state.user_for_token(&bob_token), Some(&bob));
    }
}

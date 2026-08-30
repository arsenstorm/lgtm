//! Folding a pull request's reviews into the one that decides whether human
//! review blocks anything.

use lgtm_protocol::{PrReview, ReviewState};
use serde_json::Value;

fn str_field<'a>(review: &'a Value, key: &str) -> &'a str {
    review.get(key).and_then(Value::as_str).unwrap_or("")
}

fn login(review: &Value) -> &str {
    review
        .get("user")
        .and_then(|user| user.get("login"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// Pure aggregation used by [`crate::GitHub::pull_reviews`]: GitHub returns
/// every review a person left, oldest first, so only the last one per user is
/// their current say. A `CHANGES_REQUESTED` outranks any approval.
pub fn aggregate_reviews(reviews: &[Value]) -> Option<PrReview> {
    let mut latest: Vec<&Value> = Vec::new();
    for review in reviews {
        match latest
            .iter_mut()
            .find(|other| login(other) == login(review))
        {
            Some(slot) => *slot = review,
            None => latest.push(review),
        }
    }
    latest
        .iter()
        .find(|review| str_field(review, "state") == "CHANGES_REQUESTED")
        .or_else(|| {
            latest
                .iter()
                .find(|review| str_field(review, "state") == "APPROVED")
        })
        .map(|review| PrReview {
            state: if str_field(review, "state") == "CHANGES_REQUESTED" {
                ReviewState::ChangesRequested
            } else {
                ReviewState::Approved
            },
            url: str_field(review, "html_url").to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn review(login: &str, state: &str, url: &str) -> Value {
        json!({ "user": { "login": login }, "state": state, "html_url": url })
    }

    #[test]
    fn no_reviews_is_none() {
        assert_eq!(aggregate_reviews(&[]), None);
    }

    #[test]
    fn approval_wins_without_a_changes_request() {
        let reviews = [review(
            "ana",
            "APPROVED",
            "https://github.com/o/r/pull/1#pullrequestreview-1",
        )];
        assert_eq!(
            aggregate_reviews(&reviews),
            Some(PrReview {
                state: ReviewState::Approved,
                url: "https://github.com/o/r/pull/1#pullrequestreview-1".into(),
            })
        );
    }

    #[test]
    fn a_changes_request_outranks_an_approval() {
        let reviews = [
            review(
                "ana",
                "APPROVED",
                "https://github.com/o/r/pull/1#pullrequestreview-1",
            ),
            review(
                "bo",
                "CHANGES_REQUESTED",
                "https://github.com/o/r/pull/1#pullrequestreview-2",
            ),
        ];
        assert_eq!(
            aggregate_reviews(&reviews),
            Some(PrReview {
                state: ReviewState::ChangesRequested,
                url: "https://github.com/o/r/pull/1#pullrequestreview-2".into(),
            })
        );
    }

    #[test]
    fn only_a_users_latest_review_counts() {
        let reviews = [
            review(
                "ana",
                "CHANGES_REQUESTED",
                "https://github.com/o/r/pull/1#pullrequestreview-1",
            ),
            review(
                "ana",
                "APPROVED",
                "https://github.com/o/r/pull/1#pullrequestreview-2",
            ),
        ];
        assert_eq!(
            aggregate_reviews(&reviews),
            Some(PrReview {
                state: ReviewState::Approved,
                url: "https://github.com/o/r/pull/1#pullrequestreview-2".into(),
            })
        );
    }

    #[test]
    fn a_comment_with_no_approval_or_request_is_none() {
        let reviews = [review(
            "ana",
            "COMMENTED",
            "https://github.com/o/r/pull/1#pullrequestreview-1",
        )];
        assert_eq!(aggregate_reviews(&reviews), None);
    }
}

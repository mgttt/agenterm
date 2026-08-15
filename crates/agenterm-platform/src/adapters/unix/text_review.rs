use crate::text_review::TextReviewError;

pub(crate) fn review_text(
    _owner: Option<i64>,
    _title: &str,
    _prompt: &str,
    _initial: &str,
) -> Result<Option<String>, TextReviewError> {
    Err(TextReviewError::unsupported(
        "this host does not expose an editable native modal",
    ))
}

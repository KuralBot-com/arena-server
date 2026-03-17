use crate::error::AppError;

/// Trim whitespace, reject empty strings, and enforce a maximum length.
pub fn trimmed_non_empty(field: &str, value: &str, max_len: usize) -> Result<String, AppError> {
    let trimmed = value.trim().to_string();
    if trimmed.is_empty() {
        return Err(AppError::BadRequest(format!("{field} cannot be empty")));
    }
    if trimmed.len() > max_len {
        return Err(AppError::BadRequest(format!(
            "{field} must be at most {max_len} characters"
        )));
    }
    Ok(trimmed)
}

/// For optional fields: None passes through, Some is trimmed and length-checked.
pub fn optional_trimmed(
    field: &str,
    value: &Option<String>,
    max_len: usize,
) -> Result<Option<String>, AppError> {
    match value {
        None => Ok(None),
        Some(v) => {
            let trimmed = v.trim().to_string();
            if trimmed.is_empty() {
                return Ok(None);
            }
            if trimmed.len() > max_len {
                return Err(AppError::BadRequest(format!(
                    "{field} must be at most {max_len} characters"
                )));
            }
            Ok(Some(trimmed))
        }
    }
}

/// Validate that a vote value is -1, 0, or 1.
pub fn validate_vote(value: i16) -> Result<(), AppError> {
    if value != 1 && value != -1 && value != 0 {
        return Err(AppError::BadRequest(
            "Vote value must be -1, 0, or 1".to_string(),
        ));
    }
    Ok(())
}

/// Clamp a pagination limit to 1..=100 with a default of 20.
pub fn clamp_limit(limit: Option<i64>) -> i32 {
    limit.unwrap_or(20).clamp(1, 100) as i32
}

/// Generate a URL-friendly slug from a name.
/// Keeps alphanumeric characters (including Tamil/Unicode), lowercases ASCII,
/// and joins words with hyphens.
pub fn slugify(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Validate that a list of topic IDs does not exceed the maximum allowed.
pub fn validate_topic_ids(ids: &[uuid::Uuid]) -> Result<(), AppError> {
    if ids.len() > 5 {
        return Err(AppError::BadRequest(
            "A request can have at most 5 topics".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_non_empty_valid() {
        assert_eq!(
            trimmed_non_empty("name", "  hello  ", 100).unwrap(),
            "hello"
        );
    }

    #[test]
    fn trimmed_non_empty_rejects_empty() {
        assert!(trimmed_non_empty("name", "", 100).is_err());
    }

    #[test]
    fn trimmed_non_empty_rejects_whitespace_only() {
        assert!(trimmed_non_empty("name", "   ", 100).is_err());
    }

    #[test]
    fn trimmed_non_empty_rejects_too_long() {
        let long = "a".repeat(101);
        assert!(trimmed_non_empty("name", &long, 100).is_err());
    }

    #[test]
    fn trimmed_non_empty_allows_exact_max() {
        let exact = "a".repeat(100);
        assert_eq!(trimmed_non_empty("name", &exact, 100).unwrap(), exact);
    }

    #[test]
    fn optional_trimmed_none_passes_through() {
        assert_eq!(optional_trimmed("name", &None, 100).unwrap(), None);
    }

    #[test]
    fn optional_trimmed_empty_becomes_none() {
        assert_eq!(
            optional_trimmed("name", &Some("   ".to_string()), 100).unwrap(),
            None
        );
    }

    #[test]
    fn optional_trimmed_valid() {
        assert_eq!(
            optional_trimmed("name", &Some("  hello  ".to_string()), 100).unwrap(),
            Some("hello".to_string())
        );
    }

    #[test]
    fn optional_trimmed_rejects_too_long() {
        let long = "a".repeat(101);
        assert!(optional_trimmed("name", &Some(long), 100).is_err());
    }

    #[test]
    fn clamp_limit_default() {
        assert_eq!(clamp_limit(None), 20);
    }

    #[test]
    fn clamp_limit_normal() {
        assert_eq!(clamp_limit(Some(50)), 50);
    }

    #[test]
    fn clamp_limit_caps_high() {
        assert_eq!(clamp_limit(Some(200)), 100);
    }

    #[test]
    fn clamp_limit_floors_zero() {
        assert_eq!(clamp_limit(Some(0)), 1);
    }

    #[test]
    fn clamp_limit_floors_negative() {
        assert_eq!(clamp_limit(Some(-5)), 1);
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Love and Romance"), "love-and-romance");
    }

    #[test]
    fn slugify_trims_and_collapses() {
        assert_eq!(slugify("  Hello   World  "), "hello-world");
    }

    #[test]
    fn slugify_strips_special_chars() {
        assert_eq!(slugify("Life & Death!"), "life-death");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify("   "), "");
    }

    #[test]
    fn validate_topic_ids_within_limit() {
        let ids: Vec<uuid::Uuid> = (0..5).map(|_| uuid::Uuid::new_v4()).collect();
        assert!(validate_topic_ids(&ids).is_ok());
    }

    #[test]
    fn validate_topic_ids_over_limit() {
        let ids: Vec<uuid::Uuid> = (0..6).map(|_| uuid::Uuid::new_v4()).collect();
        assert!(validate_topic_ids(&ids).is_err());
    }
}

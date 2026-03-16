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

/// Clamp a pagination limit to 1..=100 with a default of 20.
pub fn clamp_limit(limit: Option<i64>) -> i32 {
    limit.unwrap_or(20).clamp(1, 100) as i32
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
}

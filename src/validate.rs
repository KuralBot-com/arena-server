use crate::error::AppError;

/// Strip null bytes and Unicode control/format characters from input text.
/// Preserves normal ASCII whitespace (space, tab, newline) for trim() to handle.
fn sanitize_text(s: &str) -> String {
    s.chars()
        .filter(|c| {
            if c.is_ascii_whitespace() {
                return true;
            }
            // Remove ASCII control chars (includes null byte) and Unicode format chars
            // (zero-width spaces, RTL/LTR overrides, joiners, BOM, etc.)
            !c.is_control()
                && !matches!(
                    *c as u32,
                    0x200B..=0x200F | 0x202A..=0x202E | 0x2060..=0x2069 | 0xFEFF
                )
        })
        .collect()
}

/// Strip HTML angle brackets from name/identity fields.
/// Content fields (prompts, comments) are left as-is — the frontend escapes on render.
pub fn strip_html_tags(s: &str) -> String {
    s.replace(['<', '>'], "")
}

// Maximum length constants for input validation.
pub const MAX_PROMPT_LEN: usize = 2000;
pub const MAX_CONTENT_LEN: usize = 5000;
pub const MAX_DESCRIPTION_LEN: usize = 500;
pub const MAX_COMMENT_LEN: usize = 2000;
pub const MAX_REASONING_LEN: usize = 2000;
pub const MAX_NAME_LEN: usize = 100;
pub const MAX_SHORT_NAME_LEN: usize = 50;
pub const MAX_AVATAR_URL_LEN: usize = 2048;
pub const MAX_DISPLAY_NAME_LEN: usize = 100;
pub const MAX_TOPICS_PER_REQUEST: usize = 5;

/// Sanitize, trim whitespace, reject empty strings, and enforce a maximum length.
pub fn trimmed_non_empty(field: &str, value: &str, max_len: usize) -> Result<String, AppError> {
    let trimmed = sanitize_text(value).trim().to_string();
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

/// For optional fields: None passes through, Some is sanitized, trimmed, and length-checked.
pub fn optional_trimmed(
    field: &str,
    value: &Option<String>,
    max_len: usize,
) -> Result<Option<String>, AppError> {
    match value {
        None => Ok(None),
        Some(v) => {
            let trimmed = sanitize_text(v).trim().to_string();
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

/// Generate a URL-friendly ASCII slug from a name.
/// Keeps only ASCII alphanumeric characters, lowercases, and joins words with hyphens.
pub fn slugify(name: &str) -> String {
    name.trim()
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Validate that a slug is URL-friendly: non-empty, ASCII lowercase alphanumeric + hyphens,
/// no leading/trailing/consecutive hyphens.
pub fn validate_slug(slug: &str) -> Result<String, AppError> {
    let slug = slug.trim().to_lowercase();
    if slug.is_empty() {
        return Err(AppError::BadRequest("slug cannot be empty".to_string()));
    }
    if slug.len() > MAX_SHORT_NAME_LEN {
        return Err(AppError::BadRequest(format!(
            "slug must be at most {MAX_SHORT_NAME_LEN} characters"
        )));
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(AppError::BadRequest(
            "slug must contain only lowercase letters, digits, and hyphens".to_string(),
        ));
    }
    if slug.starts_with('-') || slug.ends_with('-') || slug.contains("--") {
        return Err(AppError::BadRequest(
            "slug must not start/end with a hyphen or contain consecutive hyphens".to_string(),
        ));
    }
    Ok(slug)
}

/// Maximum slug length for requests and responses.
pub const MAX_SLUG_LEN: usize = 80;

/// Generate a URL slug for a request from its prompt text.
/// Handles both English and Tamil prompts via transliteration.
pub fn generate_request_slug(prompt: &str) -> String {
    crate::transliterate::smart_slugify(prompt, 60)
}

/// Generate a URL slug for a response from the agent name.
/// The kural is already scoped to a prompt via the URL path, so the slug
/// only needs to differentiate kurals within that prompt.
pub fn generate_response_slug(agent_name: &str) -> String {
    crate::transliterate::smart_slugify(agent_name, 40)
}

/// Generate a URL slug for an agent from its name.
pub fn generate_agent_slug(name: &str) -> String {
    crate::transliterate::smart_slugify(name, 60)
}

/// Generate a URL slug for a user from their display name.
pub fn generate_user_slug(display_name: &str) -> String {
    crate::transliterate::smart_slugify(display_name, 60)
}

/// Validate that a list of topic IDs does not exceed the maximum allowed.
pub fn validate_topic_ids(ids: &[uuid::Uuid]) -> Result<(), AppError> {
    if ids.len() > MAX_TOPICS_PER_REQUEST {
        return Err(AppError::BadRequest(format!(
            "A request can have at most {MAX_TOPICS_PER_REQUEST} topics"
        )));
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
    fn slugify_strips_non_ascii() {
        // Tamil text produces empty slug since no ASCII alphanumeric chars
        assert_eq!(slugify("அறம்"), "");
    }

    #[test]
    fn validate_slug_valid() {
        assert_eq!(
            validate_slug("love-and-romance").unwrap(),
            "love-and-romance"
        );
    }

    #[test]
    fn validate_slug_rejects_non_ascii() {
        assert!(validate_slug("அறம்").is_err());
    }

    #[test]
    fn validate_slug_rejects_leading_hyphen() {
        assert!(validate_slug("-foo").is_err());
    }

    #[test]
    fn validate_slug_rejects_trailing_hyphen() {
        assert!(validate_slug("foo-").is_err());
    }

    #[test]
    fn validate_slug_rejects_consecutive_hyphens() {
        assert!(validate_slug("foo--bar").is_err());
    }

    #[test]
    fn validate_slug_rejects_uppercase() {
        // validate_slug lowercases input, so this should pass
        assert_eq!(validate_slug("Foo-Bar").unwrap(), "foo-bar");
    }

    #[test]
    fn validate_slug_rejects_empty() {
        assert!(validate_slug("").is_err());
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

    // sanitize_text tests

    #[test]
    fn sanitize_strips_null_bytes() {
        assert_eq!(sanitize_text("hello\0world"), "helloworld");
    }

    #[test]
    fn sanitize_strips_zero_width_spaces() {
        assert_eq!(sanitize_text("hello\u{200B}world"), "helloworld");
    }

    #[test]
    fn sanitize_strips_rtl_override() {
        assert_eq!(sanitize_text("hello\u{202E}world"), "helloworld");
    }

    #[test]
    fn sanitize_strips_bom() {
        assert_eq!(sanitize_text("\u{FEFF}hello"), "hello");
    }

    #[test]
    fn sanitize_preserves_normal_text() {
        assert_eq!(sanitize_text("  hello world  "), "  hello world  ");
    }

    #[test]
    fn sanitize_preserves_tamil() {
        assert_eq!(sanitize_text("அறத்தின் மேன்மை"), "அறத்தின் மேன்மை");
    }

    #[test]
    fn trimmed_non_empty_strips_null_byte() {
        assert_eq!(trimmed_non_empty("name", "hel\0lo", 100).unwrap(), "hello");
    }

    // strip_html_tags tests

    #[test]
    fn strip_html_removes_script_tags() {
        assert_eq!(
            strip_html_tags("<script>alert(1)</script>"),
            "scriptalert(1)/script"
        );
    }

    #[test]
    fn strip_html_removes_img_tag() {
        assert_eq!(
            strip_html_tags("<img src=x onerror=alert(1)>"),
            "img src=x onerror=alert(1)"
        );
    }

    #[test]
    fn strip_html_preserves_normal_text() {
        assert_eq!(strip_html_tags("Hello World"), "Hello World");
    }

    #[test]
    fn generate_request_slug_english() {
        assert_eq!(
            generate_request_slug("The importance of kindness"),
            "importance-of-kindness"
        );
    }

    #[test]
    fn generate_request_slug_tamil() {
        let slug = generate_request_slug("அறத்தின் மேன்மை");
        assert_eq!(slug, "araththin-meenmai");
    }

    #[test]
    fn generate_response_slug_agent_name() {
        assert_eq!(generate_response_slug("Tamil Poet AI"), "tamil-poet-ai");
    }

    #[test]
    fn generate_response_slug_empty_agent() {
        assert_eq!(generate_response_slug(""), "");
    }

    #[test]
    fn generate_response_slug_truncates() {
        let slug = generate_response_slug("Very Long Agent Name That Exceeds The Limit");
        assert!(slug.len() <= 40);
    }
}

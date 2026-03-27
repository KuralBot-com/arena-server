//! Tamil Unicode to ASCII romanization for URL slug generation.
//!
//! Uses a simple phonetic scheme: consonant + vowel sign → romanized syllable.
//! Pulli (்) suppresses the inherent "a" vowel. Consonants without a following
//! vowel sign or pulli get an implicit "a" appended.

/// Returns the romanized form of a standalone Tamil vowel, or None.
fn vowel(c: char) -> Option<&'static str> {
    match c {
        'அ' => Some("a"),
        'ஆ' => Some("aa"),
        'இ' => Some("i"),
        'ஈ' => Some("ii"),
        'உ' => Some("u"),
        'ஊ' => Some("uu"),
        'எ' => Some("e"),
        'ஏ' => Some("ee"),
        'ஐ' => Some("ai"),
        'ஒ' => Some("o"),
        'ஓ' => Some("oo"),
        'ஔ' => Some("au"),
        _ => None,
    }
}

/// Returns the romanized consonant (without inherent vowel), or None.
fn consonant(c: char) -> Option<&'static str> {
    match c {
        'க' => Some("k"),
        'ங' => Some("ng"),
        'ச' => Some("ch"),
        'ஜ' => Some("j"),
        'ஞ' => Some("nj"),
        'ட' => Some("t"),
        'ண' => Some("n"),
        'த' => Some("th"),
        'ந' => Some("n"),
        'ப' => Some("p"),
        'ம' => Some("m"),
        'ய' => Some("y"),
        'ர' => Some("r"),
        'ற' => Some("r"),
        'ல' => Some("l"),
        'ள' => Some("l"),
        'ழ' => Some("zh"),
        'ன' => Some("n"),
        'வ' => Some("v"),
        'ஶ' => Some("sh"),
        'ஷ' => Some("sh"),
        'ஸ' => Some("s"),
        'ஹ' => Some("h"),
        _ => None,
    }
}

/// Returns the romanized vowel for a combining vowel sign, or None.
fn vowel_sign(c: char) -> Option<&'static str> {
    match c {
        '\u{0BBE}' => Some("aa"), // ா
        '\u{0BBF}' => Some("i"),  // ி
        '\u{0BC0}' => Some("ii"), // ீ
        '\u{0BC1}' => Some("u"),  // ு
        '\u{0BC2}' => Some("uu"), // ூ
        '\u{0BC6}' => Some("e"),  // ெ
        '\u{0BC7}' => Some("ee"), // ே
        '\u{0BC8}' => Some("ai"), // ை
        '\u{0BCA}' => Some("o"),  // ொ
        '\u{0BCB}' => Some("oo"), // ோ
        '\u{0BCC}' => Some("au"), // ௌ
        _ => None,
    }
}

const PULLI: char = '\u{0BCD}'; // ்

/// Returns true if the character is in the Tamil Unicode block (U+0B80–U+0BFF).
fn is_tamil_char(c: char) -> bool {
    ('\u{0B80}'..='\u{0BFF}').contains(&c)
}

/// Returns true if the text contains a significant proportion of Tamil characters.
/// "Significant" means more than half of the alphabetic characters are Tamil.
pub fn is_tamil(text: &str) -> bool {
    let mut tamil = 0u32;
    let mut alpha = 0u32;
    for c in text.chars() {
        if is_tamil_char(c) {
            tamil += 1;
            alpha += 1;
        } else if c.is_alphabetic() {
            alpha += 1;
        }
    }
    alpha > 0 && tamil * 2 >= alpha
}

/// Transliterate Tamil Unicode text to ASCII romanized form.
///
/// Non-Tamil characters pass through unchanged (spaces, digits, punctuation, Latin).
pub fn transliterate_tamil(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len() * 2);
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if let Some(v) = vowel(c) {
            // Standalone vowel
            out.push_str(v);
            i += 1;
        } else if let Some(cons) = consonant(c) {
            out.push_str(cons);
            // Check what follows the consonant
            if i + 1 < chars.len() {
                let next = chars[i + 1];
                if next == PULLI {
                    // Pulli suppresses inherent vowel — emit nothing extra
                    i += 2;
                } else if let Some(vs) = vowel_sign(next) {
                    out.push_str(vs);
                    i += 2;
                } else {
                    // No modifier — inherent "a"
                    out.push('a');
                    i += 1;
                }
            } else {
                // End of string — inherent "a"
                out.push('a');
                i += 1;
            }
        } else if is_tamil_char(c) {
            // Tamil character we don't handle (digits, symbols, etc.) — skip
            i += 1;
        } else {
            // Non-Tamil character — pass through
            out.push(c);
            i += 1;
        }
    }

    out
}

/// Detect language and produce an ASCII slug: transliterate Tamil text first,
/// then apply the standard slugify + truncation logic.
///
/// - Strips leading English articles ("the", "a", "an").
/// - Truncates at a word boundary to `max_len` characters.
pub fn smart_slugify(text: &str, max_len: usize) -> String {
    let ascii_text = if is_tamil(text) {
        transliterate_tamil(text)
    } else {
        text.to_string()
    };

    let mut slug = crate::validate::slugify(&ascii_text);

    // Strip leading articles
    for prefix in &["the-", "a-", "an-"] {
        if slug.starts_with(prefix) {
            slug = slug[prefix.len()..].to_string();
            break;
        }
    }

    // Truncate at word boundary
    if slug.len() > max_len {
        slug.truncate(max_len);
        if let Some(last_hyphen) = slug.rfind('-') {
            slug.truncate(last_hyphen);
        }
    }

    // Remove trailing hyphen (safety)
    while slug.ends_with('-') {
        slug.pop();
    }

    slug
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_tamil ---

    #[test]
    fn is_tamil_pure_tamil() {
        assert!(is_tamil("அறம்"));
    }

    #[test]
    fn is_tamil_pure_english() {
        assert!(!is_tamil("virtue of kindness"));
    }

    #[test]
    fn is_tamil_empty() {
        assert!(!is_tamil(""));
    }

    #[test]
    fn is_tamil_mixed_majority_tamil() {
        // Tamil words with one English word
        assert!(is_tamil("அறம் செய்ய love விரும்பு"));
    }

    #[test]
    fn is_tamil_mixed_majority_english() {
        assert!(!is_tamil("The virtue of அறம்"));
    }

    // --- transliterate_tamil ---

    #[test]
    fn transliterate_single_vowels() {
        assert_eq!(transliterate_tamil("அ"), "a");
        assert_eq!(transliterate_tamil("ஆ"), "aa");
        assert_eq!(transliterate_tamil("இ"), "i");
        assert_eq!(transliterate_tamil("ஈ"), "ii");
        assert_eq!(transliterate_tamil("உ"), "u");
        assert_eq!(transliterate_tamil("ஊ"), "uu");
        assert_eq!(transliterate_tamil("எ"), "e");
        assert_eq!(transliterate_tamil("ஏ"), "ee");
        assert_eq!(transliterate_tamil("ஐ"), "ai");
        assert_eq!(transliterate_tamil("ஒ"), "o");
        assert_eq!(transliterate_tamil("ஓ"), "oo");
        assert_eq!(transliterate_tamil("ஔ"), "au");
    }

    #[test]
    fn transliterate_consonant_with_inherent_a() {
        assert_eq!(transliterate_tamil("க"), "ka");
        assert_eq!(transliterate_tamil("ப"), "pa");
        assert_eq!(transliterate_tamil("ம"), "ma");
    }

    #[test]
    fn transliterate_consonant_with_pulli() {
        assert_eq!(transliterate_tamil("க்"), "k");
        assert_eq!(transliterate_tamil("ம்"), "m");
        assert_eq!(transliterate_tamil("ன்"), "n");
    }

    #[test]
    fn transliterate_consonant_with_vowel_sign() {
        assert_eq!(transliterate_tamil("கி"), "ki");
        assert_eq!(transliterate_tamil("கு"), "ku");
        assert_eq!(transliterate_tamil("கா"), "kaa");
        assert_eq!(transliterate_tamil("கீ"), "kii");
        assert_eq!(transliterate_tamil("கூ"), "kuu");
        assert_eq!(transliterate_tamil("கே"), "kee");
        assert_eq!(transliterate_tamil("கை"), "kai");
        assert_eq!(transliterate_tamil("கொ"), "ko");
        assert_eq!(transliterate_tamil("கோ"), "koo");
        assert_eq!(transliterate_tamil("கௌ"), "kau");
    }

    #[test]
    fn transliterate_word_aram() {
        // அறம் = a + ra + m(pulli)
        assert_eq!(transliterate_tamil("அறம்"), "aram");
    }

    #[test]
    fn transliterate_word_kural() {
        // குறள் = ku + ra + l(pulli)
        assert_eq!(transliterate_tamil("குறள்"), "kural");
    }

    #[test]
    fn transliterate_word_tamil() {
        // தமிழ் = tha + mi + zh(pulli)
        assert_eq!(transliterate_tamil("தமிழ்"), "thamizh");
    }

    #[test]
    fn transliterate_phrase_with_spaces() {
        // அறத்தின் மேன்மை = a + ra + th(pulli) + thi + n(pulli) + space + mee + n(pulli) + mai
        assert_eq!(transliterate_tamil("அறத்தின் மேன்மை"), "araththin meenmai");
    }

    #[test]
    fn transliterate_preserves_english() {
        assert_eq!(transliterate_tamil("hello world"), "hello world");
    }

    #[test]
    fn transliterate_mixed() {
        assert_eq!(transliterate_tamil("அறம் is virtue"), "aram is virtue");
    }

    #[test]
    fn transliterate_empty() {
        assert_eq!(transliterate_tamil(""), "");
    }

    // --- smart_slugify ---

    #[test]
    fn smart_slugify_english() {
        assert_eq!(
            smart_slugify("The importance of kindness", 60),
            "importance-of-kindness"
        );
    }

    #[test]
    fn smart_slugify_tamil() {
        assert_eq!(smart_slugify("அறத்தின் மேன்மை", 60), "araththin-meenmai");
    }

    #[test]
    fn smart_slugify_truncates_at_word_boundary() {
        let long = "importance of kindness in daily life and everything else";
        let slug = smart_slugify(long, 30);
        assert!(slug.len() <= 30);
        assert!(!slug.ends_with('-'));
        assert_eq!(slug, "importance-of-kindness-in");
    }

    #[test]
    fn smart_slugify_strips_article_a() {
        assert_eq!(smart_slugify("A great virtue", 60), "great-virtue");
    }

    #[test]
    fn smart_slugify_strips_article_an() {
        assert_eq!(
            smart_slugify("An excellent approach", 60),
            "excellent-approach"
        );
    }

    #[test]
    fn smart_slugify_empty() {
        assert_eq!(smart_slugify("", 60), "");
    }

    #[test]
    fn smart_slugify_only_special_chars() {
        assert_eq!(smart_slugify("!@#$%", 60), "");
    }
}

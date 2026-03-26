/// Wilson score lower bound for upvote/downvote ranking.
/// Returns a value in the 0.0–1.0 range, or None if there are no votes.
/// Uses z = 1.96 (95% confidence interval).
pub fn wilson_lower_bound(upvotes: i64, downvotes: i64) -> Option<f64> {
    let n = (upvotes + downvotes) as f64;
    if n == 0.0 {
        return None;
    }

    let p = upvotes as f64 / n;
    let z = 1.96_f64;
    let z2 = z * z;

    let numerator = p + z2 / (2.0 * n) - z * ((p * (1.0 - p) + z2 / (4.0 * n)) / n).sqrt();
    let denominator = 1.0 + z2 / n;

    Some(numerator / denominator)
}

/// Composite score: evaluator base (0–40) + community votes.
///
/// Formula: `avg(criterion_scores) × 40 + vote_total`
/// Returns None only when there are no evaluations and no votes.
pub fn composite_score(vote_total: i64, criterion_avgs: &[f64]) -> Option<f64> {
    if criterion_avgs.is_empty() && vote_total == 0 {
        return None;
    }

    let base = if criterion_avgs.is_empty() {
        0.0
    } else {
        let sum: f64 = criterion_avgs.iter().sum();
        (sum / criterion_avgs.len() as f64) * 40.0
    };

    Some(base + vote_total as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_votes_returns_none() {
        assert_eq!(wilson_lower_bound(0, 0), None);
    }

    #[test]
    fn single_upvote() {
        let score = wilson_lower_bound(1, 0).unwrap();
        assert!(score > 0.0);
        assert!(score < 1.0);
    }

    #[test]
    fn all_upvotes_large_n() {
        let score = wilson_lower_bound(1000, 0).unwrap();
        assert!(score > 0.99, "Expected > 0.99, got {score}");
    }

    #[test]
    fn all_downvotes() {
        let score = wilson_lower_bound(0, 100).unwrap();
        assert!(score < 0.01, "Expected < 0.01, got {score}");
    }

    #[test]
    fn even_split() {
        let score = wilson_lower_bound(50, 50).unwrap();
        assert!(score > 0.3 && score < 0.5, "Expected 0.3..0.5, got {score}");
    }

    #[test]
    fn more_upvotes_higher_score() {
        let high = wilson_lower_bound(80, 20).unwrap();
        let low = wilson_lower_bound(20, 80).unwrap();
        assert!(high > low);
    }

    #[test]
    fn composite_no_data_returns_none() {
        assert_eq!(composite_score(0, &[]), None);
    }

    #[test]
    fn composite_votes_only() {
        let score = composite_score(3, &[]).unwrap();
        assert!((score - 3.0).abs() < 0.01);
    }

    #[test]
    fn composite_evaluator_only() {
        // Single criterion score 0.9 → 0.9 * 40 = 36
        let score = composite_score(0, &[0.9]).unwrap();
        assert!((score - 36.0).abs() < 0.01);
    }

    #[test]
    fn composite_evaluator_plus_votes() {
        // avg(0.9, 0.8) = 0.85 → 0.85 * 40 = 34, + 4 votes = 38
        let score = composite_score(4, &[0.9, 0.8]).unwrap();
        assert!((score - 38.0).abs() < 0.01);
    }

    #[test]
    fn composite_negative_votes() {
        // 0.9 * 40 = 36, - 2 votes = 34
        let score = composite_score(-2, &[0.9]).unwrap();
        assert!((score - 34.0).abs() < 0.01);
    }
}

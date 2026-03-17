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

/// Composite score from vote score and dynamic criteria averages.
/// Returns a value in the 0–100 range, or None if no dimensions have data.
pub fn composite_score(
    vote_score: Option<f64>,
    vote_weight: f32,
    criterion_avgs: &[(f32, f64)], // (weight, avg_score) pairs
) -> Option<f64> {
    let mut total = 0.0;
    let mut weight_sum = 0.0;

    if let Some(vs) = vote_score {
        total += vs * vote_weight as f64;
        weight_sum += vote_weight as f64;
    }
    for &(w, avg) in criterion_avgs {
        total += avg * w as f64;
        weight_sum += w as f64;
    }

    if weight_sum == 0.0 {
        None
    } else {
        Some((total / weight_sum) * 100.0)
    }
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
        assert_eq!(composite_score(None, 0.34, &[]), None);
    }

    #[test]
    fn composite_vote_only() {
        let score = composite_score(Some(0.8), 0.34, &[]).unwrap();
        assert!((score - 80.0).abs() < 0.01);
    }

    #[test]
    fn composite_all_dimensions() {
        let score = composite_score(Some(0.8), 0.34, &[(0.33, 0.9), (0.33, 0.7)]).unwrap();
        assert!(score > 0.0 && score <= 100.0);
    }
}

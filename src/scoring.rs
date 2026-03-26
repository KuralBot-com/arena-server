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

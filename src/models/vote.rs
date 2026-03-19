use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoteBody {
    pub value: i16,
}

#[derive(Serialize)]
pub struct VoteResult {
    pub vote_total: i64,
}

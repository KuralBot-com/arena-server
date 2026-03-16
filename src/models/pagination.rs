use serde::Serialize;

#[derive(Serialize)]
pub struct PaginatedResponse<T: Serialize> {
    pub data: Vec<T>,
    pub next_cursor: Option<String>,
    pub limit: i64,
}

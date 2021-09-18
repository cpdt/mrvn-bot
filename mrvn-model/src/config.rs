#[derive(Debug, Clone, Copy)]
pub struct AppModelConfig {
    pub skip_votes_required: usize,
    pub stop_votes_required: usize,
}

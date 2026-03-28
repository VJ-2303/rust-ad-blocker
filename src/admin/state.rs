use std::sync::Arc;

use crate::{blocklist::Blocklist, metrics::Metrics};

#[derive(Clone)]
pub struct AppState {
    pub metrics: Arc<Metrics>,
    pub blocklist: Arc<Blocklist>,
}

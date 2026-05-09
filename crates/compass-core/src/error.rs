use thiserror::Error;

#[derive(Debug, Error)]
pub enum CompassError {
    #[error("payment required")]
    PaymentRequired,
    #[error("invalid receipt: {0}")]
    InvalidReceipt(String),
    #[error("subgraph not found: {0}")]
    SubgraphNotFound(String),
    #[error("graph-node error: {0}")]
    GraphNodeError(String),
    #[error(transparent)]
    Internal(#[from] anyhow::Error),
}

// error.rs — one error enum, mapped to REST status codes and MCP error objects. Mechanical
// derive (thiserror-shaped, hand-rolled here to keep the dependency list short for the
// skeleton) over a taxonomy this crate authored, not one a crate invented for it.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use std::fmt;

#[derive(Debug)]
pub enum StoreError {
    /// The requested stack/slug pair does not exist.
    NotFound,
    /// Underlying Postgres error, opaque to the caller beyond "the store failed".
    Db(sqlx::Error),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NotFound => write!(f, "not found"),
            StoreError::Db(e) => write!(f, "store error: {e}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<sqlx::Error> for StoreError {
    fn from(e: sqlx::Error) -> Self {
        StoreError::Db(e)
    }
}

// Note deliberately absent: no `SlugCollision` variant. A colliding (stack, slug) lay is not an
// error — it's an inert append (Society::lay's own `bool`, see wire::LayResult). Only genuine
// failures (not-found reads, DB faults) are errors here.

impl IntoResponse for StoreError {
    fn into_response(self) -> Response {
        let status = match self {
            StoreError::NotFound => StatusCode::NOT_FOUND,
            StoreError::Db(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, self.to_string()).into_response()
    }
}

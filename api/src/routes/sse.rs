//! T07: per-Branch SSE stream.
use axum::{extract::State, response::sse::{Event, KeepAlive, Sse}};
use futures_core::Stream;
use std::{convert::Infallible, time::Duration};
use crate::{extractors::AuthorizedBranch, state::AppState};

pub async fn stream_branch_events(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.sse_receiver(branch_id).await;
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(signal) => {
                    if let Ok(json) = serde_json::to_string(&signal) {
                        yield Ok(Event::default().data(json));
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(
        KeepAlive::new().interval(Duration::from_secs(30)).text("keep-alive"),
    )
}

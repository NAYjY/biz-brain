//! T07's web -> browser hop: `web` re-exposes its own SSE endpoint at
//! `/branches/:branch_id/events` (no `/api/v1` prefix — this is the
//! browser-facing route; `api`'s own `/api/v1/branches/:id/events` stays a
//! same-process, non-browser-facing stream). Same auth extractor as `api`
//! (T05's Branch-ownership check), same underlying broadcast channel — this
//! is a re-origination, not a proxy, but the browser only ever talks to `web`.

use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
};
use futures_core::Stream;
use std::{convert::Infallible, time::Duration};

use api::{extractors::AuthorizedBranch, AppState};

pub async fn relay_branch_events(
    AuthorizedBranch { branch_id, .. }: AuthorizedBranch,
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.sse_receiver(branch_id).await;

    // Bounded relay to the browser: if the tab is slower than `api` produces
    // (T07's backpressure concern), collapse to "something changed" rather
    // than buffering unboundedly — these signals are idempotent re-fetch pings.
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

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30)).text("keep-alive"))
}

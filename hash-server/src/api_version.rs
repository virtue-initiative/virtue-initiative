use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Json;
use axum::extract::Request;
use axum::http::{StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use serde_json::json;
use tower_layer::Layer;
use tower_service::Service;

/// The whole codebase shares one version, tracked in `client/version.properties`. This
/// is that version's `/vX`/`/vX.Y` URL-prefix form (HASH-004) — kept in sync
/// by `client/scripts/update-version.sh`, which is the only thing that should ever edit
/// this line.
const CURRENT_API_VERSION: &str = "v0.1";

/// HASH-004: strips a leading `/vX` or `/vX.Y` path segment naming the
/// current version before routing, and responds 410 Gone for any other version.
/// Requests with no version segment at all are passed through unchanged, so existing
/// unversioned callers keep working.
///
/// This has to be applied around the whole `Router` (see `router()` in lib.rs), not via
/// `Router::layer`, since `Router::layer` middleware runs *after* routing has already
/// happened and so can't rewrite the path used for matching.
#[derive(Clone)]
pub struct ApiVersionLayer;

impl<S> Layer<S> for ApiVersionLayer {
    type Service = ApiVersion<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ApiVersion { inner }
    }
}

#[derive(Clone)]
pub struct ApiVersion<S> {
    inner: S,
}

impl<S> Service<Request> for ApiVersion<S>
where
    S: Service<Request, Response = Response, Error = Infallible> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let path = req.uri().path().to_string();

        let Some((segment, rest)) = split_version_segment(&path) else {
            return Box::pin(self.inner.call(req));
        };

        if segment != CURRENT_API_VERSION {
            return Box::pin(async move {
                Ok((
                    StatusCode::GONE,
                    Json(json!({ "error": "This API version is no longer supported" })),
                )
                    .into_response())
            });
        }

        let new_path_and_query = match req.uri().query() {
            Some(q) => format!("{rest}?{q}"),
            None => rest,
        };
        if let Ok(new_uri) = Uri::try_from(new_path_and_query) {
            *req.uri_mut() = new_uri;
        }

        Box::pin(self.inner.call(req))
    }
}

/// Returns `(version_segment, remaining_path)` if `path` starts with a `/vN` or
/// `/vN.M` segment, e.g. `/v0.1/hash` -> `("v0.1", "/hash")`.
fn split_version_segment(path: &str) -> Option<(&str, String)> {
    let rest = path.strip_prefix('/')?;
    let (segment, remainder) = match rest.split_once('/') {
        Some((seg, rem)) => (seg, format!("/{rem}")),
        None => (rest, "/".to_string()),
    };

    is_version_segment(segment).then_some((segment, remainder))
}

fn is_version_segment(segment: &str) -> bool {
    let Some(version) = segment.strip_prefix('v') else {
        return false;
    };
    let is_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts = version.split('.');

    match (parts.next(), parts.next(), parts.next()) {
        (Some(major), None, None) => is_digits(major),
        (Some(major), Some(minor), None) => is_digits(major) && is_digits(minor),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::get;
    use tower::ServiceExt;

    fn app() -> ApiVersion<Router> {
        let router: Router = Router::new()
            .route("/", get(|| async { "root" }))
            .route("/hash", get(|| async { "hash" }));
        ApiVersionLayer.layer(router)
    }

    async fn get_status(uri: &str) -> StatusCode {
        app()
            .oneshot(HttpRequest::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn routes_current_version_prefix_the_same_as_unprefixed() {
        assert_eq!(get_status("/hash").await, StatusCode::OK);
        assert_eq!(get_status("/v0.1/hash").await, StatusCode::OK);
        assert_eq!(get_status("/v0.1").await, StatusCode::OK);
        assert_eq!(get_status("/").await, StatusCode::OK);
    }

    #[tokio::test]
    async fn preserves_query_string_when_stripping_the_version() {
        let status = app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/v0.1/hash?devices=abc")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap()
            .status();
        assert_eq!(status, StatusCode::OK);
    }

    #[tokio::test]
    async fn responds_410_for_no_longer_supported_versions() {
        for version in ["v0.2", "v1", "v2"] {
            let status = get_status(&format!("/{version}/hash")).await;
            assert_eq!(status, StatusCode::GONE, "version {version}");
        }
    }
}

use axum::extract::Request;
use axum::middleware::Next;
use axum::response::Response;

/// SPEC.md section 3.4: logs every request at debug level, without the body.
pub async fn log_request(req: Request, next: Next) -> Response {
    tracing::debug!(method = %req.method(), path = %req.uri().path(), "request");
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Router;
    use axum::body::Body;
    use axum::http::{Request as HttpRequest, StatusCode};
    use axum::routing::post;
    use tower::ServiceExt;
    use tracing_test::traced_test;

    #[traced_test]
    #[tokio::test]
    async fn logs_every_request_at_debug_level_without_the_body() {
        let app: Router = Router::new()
            .route("/hash", post(|| async { "ok" }))
            .layer(axum::middleware::from_fn(log_request));

        let secret_body = "super-secret-request-body-marker";
        let response = app
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri("/hash")
                    .body(Body::from(secret_body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        // SPEC.md section 3.4: "The server SHOULD log every request at level debug."
        assert!(logs_contain("request"));
        assert!(logs_contain("POST"));
        assert!(logs_contain("/hash"));
        // SPEC.md section 3.4: "The server SHOULD NOT log the body of every request."
        assert!(!logs_contain(secret_body));
    }
}

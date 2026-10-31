//! The routes.

use axum::extract::Path;
use axum::response::Html;
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;

pub fn router() -> Router {
    Router::new().route("/", get(home)).route("/api/hello/{name}", get(hello))
}

async fn home() -> Html<&'static str> {
    Html("<h1>{{name}}</h1><p>Try <a href=\"/api/hello/world\">/api/hello/world</a>.</p>")
}

#[derive(Debug, Serialize)]
pub struct Greeting {
    pub message: String,
}

async fn hello(Path(name): Path<String>) -> Json<Greeting> {
    Json(Greeting { message: format!("Hello, {name}!") })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn call(uri: &str) -> (StatusCode, String) {
        let response = router().oneshot(Request::get(uri).body(Body::empty()).unwrap()).await.unwrap();
        let status = response.status();
        let body = response.into_body().collect().await.unwrap().to_bytes();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    #[tokio::test]
    async fn the_page_is_there() {
        let (status, body) = call("/").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("/api/hello/world"));
    }

    #[tokio::test]
    async fn hello_answers_in_json() {
        let (status, body) = call("/api/hello/Ada").await;
        assert_eq!(status, StatusCode::OK);
        let json: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["message"], "Hello, Ada!");
    }

    #[tokio::test]
    async fn anything_else_is_not_found() {
        assert_eq!(call("/nowhere").await.0, StatusCode::NOT_FOUND);
    }
}

use axum::{
    http::{HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Response},
};

const INDEX_HTML: &str = include_str!("../assets/console/index.html");
const STYLES_CSS: &str = include_str!("../assets/console/styles.css");
const APP_JS: &str = include_str!("../assets/console/app.js");
const ICONFONT_WOFF2: &[u8] = include_bytes!("../assets/console/iconfont.woff2");
const LOGIN_HTML: &str = include_str!("../assets/login/index.html");
const LOGIN_STYLES_CSS: &str = include_str!("../assets/login/styles.css");
const LOGIN_APP_JS: &str = include_str!("../assets/login/app.js");

pub async fn console_index() -> Html<&'static str> {
    Html(INDEX_HTML)
}

pub async fn console_styles() -> Response {
    text_response("text/css; charset=utf-8", STYLES_CSS)
}

pub async fn console_script() -> Response {
    text_response("application/javascript; charset=utf-8", APP_JS)
}

pub async fn console_iconfont() -> Response {
    bytes_response("font/woff2", ICONFONT_WOFF2)
}

pub async fn login_index() -> Html<&'static str> {
    Html(LOGIN_HTML)
}

pub async fn login_styles() -> Response {
    text_response("text/css; charset=utf-8", LOGIN_STYLES_CSS)
}

pub async fn login_script() -> Response {
    text_response("application/javascript; charset=utf-8", LOGIN_APP_JS)
}

fn text_response(content_type: &'static str, body: &'static str) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("no-store, no-cache, must-revalidate"),
            ),
        ],
        body,
    )
        .into_response()
}

fn bytes_response(content_type: &'static str, body: &'static [u8]) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static(content_type)),
            (
                header::CACHE_CONTROL,
                HeaderValue::from_static("public, max-age=31536000, immutable"),
            ),
        ],
        body,
    )
        .into_response()
}

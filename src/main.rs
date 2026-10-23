use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
};
use serde::{Deserialize, Serialize};
use url::Url;

const FEED_BASE: &str = "https://www.youtube.com/feeds/videos.xml?channel_id=";
const ALLOWED_HOSTS: [&str; 3] = ["youtube.com", "www.youtube.com", "m.youtube.com"];

#[derive(Deserialize)]
struct FeedQuery {
    url: String,
}

#[derive(Serialize)]
struct FeedResponse {
    channel_id: String,
    feed_url: String,
}

#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

type ApiResult = Result<Json<FeedResponse>, (StatusCode, Json<ErrorResponse>)>;

#[tokio::main]
async fn main() {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0")
        .build()
        .expect("http client");

    let app = Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../static/index.html")) }),
        )
        .route("/api/feed", get(feed))
        .with_state(client);

    let addr = std::env::var("BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&addr).await.expect("bind");
    println!("listening on {addr}");
    axum::serve(listener, app).await.expect("server");
}

async fn feed(State(client): State<reqwest::Client>, Query(q): Query<FeedQuery>) -> ApiResult {
    let channel_id = resolve(&client, q.url.trim())
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, Json(ErrorResponse { error: e })))?;
    Ok(Json(FeedResponse {
        feed_url: format!("{FEED_BASE}{channel_id}"),
        channel_id,
    }))
}

async fn resolve(client: &reqwest::Client, input: &str) -> Result<String, String> {
    if is_channel_id(input) {
        return Ok(input.to_string());
    }

    let url = to_youtube_url(input)?;

    let mut segments = url.path_segments().into_iter().flatten();
    if let (Some("channel"), Some(id)) = (segments.next(), segments.next()) {
        if is_channel_id(id) {
            return Ok(id.to_string());
        }
    }

    let html = client
        .get(url)
        .header("Cookie", "SOCS=CAI; CONSENT=YES+1")
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Could not fetch the channel page: {e}"))?
        .text()
        .await
        .map_err(|e| format!("Could not read the channel page: {e}"))?;

    find_channel_id(&html).ok_or_else(|| "No channel ID found on that page.".into())
}

fn to_youtube_url(input: &str) -> Result<Url, String> {
    let input = if input.starts_with('@') {
        format!("https://www.youtube.com/{input}")
    } else if input.contains("://") {
        input.to_string()
    } else {
        format!("https://{input}")
    };

    let url = Url::parse(&input).map_err(|_| "That is not a valid URL.".to_string())?;
    match url.host_str() {
        Some(host) if ALLOWED_HOSTS.contains(&host) => Ok(url),
        _ => Err("Only youtube.com URLs are supported.".into()),
    }
}

fn find_channel_id(html: &str) -> Option<String> {
    [
        "\"externalId\":\"",
        "<link rel=\"canonical\" href=\"https://www.youtube.com/channel/",
    ]
    .iter()
    .find_map(|marker| {
        let start = html.find(marker)? + marker.len();
        let id = html.get(start..start + 24)?;
        is_channel_id(id).then(|| id.to_string())
    })
}

fn is_channel_id(s: &str) -> bool {
    s.len() == 24
        && s.starts_with("UC")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_channel_ids() {
        assert!(is_channel_id("UCXuqSBlHAE6Xw-yeJA0Tunw"));
        assert!(!is_channel_id("UCXuqSBlHAE6Xw-yeJA0Tun"));
        assert!(!is_channel_id("XXXuqSBlHAE6Xw-yeJA0Tunw"));
    }

    #[test]
    fn normalises_input() {
        assert_eq!(
            to_youtube_url("@LinusTechTips").unwrap().as_str(),
            "https://www.youtube.com/@LinusTechTips"
        );
        assert_eq!(
            to_youtube_url("youtube.com/c/foo").unwrap().as_str(),
            "https://youtube.com/c/foo"
        );
        assert!(to_youtube_url("https://example.com/@foo").is_err());
    }

    #[test]
    fn finds_id_in_html() {
        let html = r#"..."externalId":"UCXuqSBlHAE6Xw-yeJA0Tunw","foo"..."#;
        assert_eq!(
            find_channel_id(html).as_deref(),
            Some("UCXuqSBlHAE6Xw-yeJA0Tunw")
        );
    }
}

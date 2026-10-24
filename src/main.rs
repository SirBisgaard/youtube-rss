use axum::{
    Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
};
use serde::{Deserialize, Serialize};
use url::Url;

const FEED_BASE: &str = "https://www.youtube.com/feeds/videos.xml";
const ALLOWED_HOSTS: [&str; 4] = [
    "youtube.com",
    "www.youtube.com",
    "m.youtube.com",
    "youtu.be",
];
const MAX_VIDEOS: usize = 6;

#[derive(Deserialize)]
struct FeedQuery {
    url: String,
    #[serde(default)]
    hide_shorts: bool,
}

#[derive(Serialize)]
struct FeedResponse {
    channel_id: String,
    feed_url: String,
    #[serde(flatten)]
    preview: FeedPreview,
}

#[derive(Serialize, Default)]
struct FeedPreview {
    title: String,
    videos: Vec<Video>,
}

#[derive(Serialize)]
struct Video {
    title: String,
    url: String,
    thumbnail: String,
}

#[derive(Deserialize)]
struct AtomFeed {
    author: AtomAuthor,
    #[serde(rename = "entry", default)]
    entries: Vec<AtomEntry>,
}

#[derive(Deserialize)]
struct AtomEntry {
    title: String,
    link: AtomLink,
    group: MediaGroup,
}

#[derive(Deserialize)]
struct AtomAuthor {
    name: String,
}

#[derive(Deserialize)]
struct AtomLink {
    #[serde(rename = "@href")]
    href: String,
}

#[derive(Deserialize)]
struct MediaGroup {
    thumbnail: MediaThumbnail,
}

#[derive(Deserialize)]
struct MediaThumbnail {
    #[serde(rename = "@url")]
    url: String,
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
    let feed_url = feed_url(&channel_id, q.hide_shorts);
    let preview = fetch_preview(&client, &feed_url).await.unwrap_or_default();
    Ok(Json(FeedResponse {
        feed_url,
        channel_id,
        preview,
    }))
}

/// Channel feeds include Shorts; the `UULF` uploads playlist is the same channel without them.
fn feed_url(channel_id: &str, hide_shorts: bool) -> String {
    if hide_shorts {
        format!("{FEED_BASE}?playlist_id=UULF{}", &channel_id[2..])
    } else {
        format!("{FEED_BASE}?channel_id={channel_id}")
    }
}

async fn fetch_preview(client: &reqwest::Client, feed_url: &str) -> Option<FeedPreview> {
    let xml = client
        .get(feed_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .ok()?
        .text()
        .await
        .ok()?;
    parse_preview(&xml)
}

fn parse_preview(xml: &str) -> Option<FeedPreview> {
    let feed: AtomFeed = quick_xml::de::from_str(xml).ok()?;
    Some(FeedPreview {
        title: feed.author.name,
        videos: feed
            .entries
            .into_iter()
            .take(MAX_VIDEOS)
            .map(|e| Video {
                title: e.title,
                url: e.link.href,
                thumbnail: e.group.thumbnail.url,
            })
            .collect(),
    })
}

async fn resolve(client: &reqwest::Client, input: &str) -> Result<String, String> {
    if is_channel_id(input) {
        return Ok(input.to_string());
    }

    let mut url = to_youtube_url(input)?;

    let mut segments = url.path_segments().into_iter().flatten();
    match (segments.next(), segments.next()) {
        (Some("channel"), Some(id)) if is_channel_id(id) => return Ok(id.to_string()),
        (Some("embed"), Some(id)) => {
            url = Url::parse(&format!("https://www.youtube.com/watch?v={id}"))
                .map_err(|_| "That is not a valid URL.".to_string())?;
        }
        _ => {}
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
        "\"externalChannelId\":\"",
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
        assert!(to_youtube_url("youtu.be/BKubvJHBkbs").is_ok());
        assert!(to_youtube_url("https://example.com/@foo").is_err());
    }

    #[test]
    fn builds_feed_urls() {
        assert_eq!(
            feed_url("UCXuqSBlHAE6Xw-yeJA0Tunw", false),
            "https://www.youtube.com/feeds/videos.xml?channel_id=UCXuqSBlHAE6Xw-yeJA0Tunw"
        );
        assert_eq!(
            feed_url("UCXuqSBlHAE6Xw-yeJA0Tunw", true),
            "https://www.youtube.com/feeds/videos.xml?playlist_id=UULFXuqSBlHAE6Xw-yeJA0Tunw"
        );
    }

    #[test]
    fn finds_id_in_video_page() {
        let html = r#"..."externalChannelId":"UCwiaPYufmQOq5F1TI-FzQhw","foo"..."#;
        assert_eq!(
            find_channel_id(html).as_deref(),
            Some("UCwiaPYufmQOq5F1TI-FzQhw")
        );
    }

    #[test]
    fn finds_id_in_html() {
        let html = r#"..."externalId":"UCXuqSBlHAE6Xw-yeJA0Tunw","foo"..."#;
        assert_eq!(
            find_channel_id(html).as_deref(),
            Some("UCXuqSBlHAE6Xw-yeJA0Tunw")
        );
    }

    #[test]
    fn parses_feed() {
        let entry = r#"<entry>
            <yt:videoId>abc</yt:videoId>
            <title>Tom &amp; Jerry</title>
            <link rel="alternate" href="https://www.youtube.com/watch?v=abc"/>
            <media:group>
                <media:title>Tom &amp; Jerry</media:title>
                <media:thumbnail url="https://i1.ytimg.com/vi/abc/hqdefault.jpg" width="480" height="360"/>
            </media:group>
        </entry>"#;
        let xml = format!(
            r#"<?xml version="1.0"?><feed xmlns:yt="y" xmlns:media="m" xmlns="a"><title>Videos</title><author><name>Chan</name></author>{}</feed>"#,
            entry.repeat(7)
        );
        let preview = parse_preview(&xml).unwrap();
        assert_eq!(preview.title, "Chan");
        let videos = preview.videos;
        assert_eq!(videos.len(), MAX_VIDEOS);
        assert_eq!(videos[0].title, "Tom & Jerry");
        assert_eq!(videos[0].url, "https://www.youtube.com/watch?v=abc");
        assert_eq!(
            videos[0].thumbnail,
            "https://i1.ytimg.com/vi/abc/hqdefault.jpg"
        );
    }
}

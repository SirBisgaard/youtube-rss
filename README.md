# youtube-rss

A tiny website that turns a YouTube channel URL into its RSS feed URL.

Accepts `https://www.youtube.com/@handle`, `/channel/UC…`, `/c/name`, `/user/name`, a bare `@handle` or a bare channel ID.

## Run

```sh
docker compose up -d --build
```

Open http://localhost:8080.

## API

```
GET /api/feed?url=<channel url>
→ {"channel_id": "UC…", "feed_url": "https://www.youtube.com/feeds/videos.xml?channel_id=UC…"}
```

"""RyM's bridge to ytmusicapi (github.com/sigma67/ytmusicapi, MIT).

RyM starts this once and keeps it running. One JSON request per line on stdin, one JSON answer
per line on stdout:
    {"id": 1, "op": "search", "query": "...", "filter": "songs", "limit": 40}
    -> {"id": 1, "ok": true, "data": [track, ...]}
Tracks come out already in RyM's shape (video_id, title, artist, album, duration, thumb,
album_id, artist_id), so a change in YouTube's answers can only break this file, never the app.
The first line out says whether ytmusicapi loaded: {"ready": true, "version": "1.12.3"}.
The sign-in cookie only ever comes in on stdin (never on the command line).
"""

import json
import sys


def out(obj):
    sys.stdout.write(json.dumps(obj, ensure_ascii=False) + "\n")
    sys.stdout.flush()


try:
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stdin.reconfigure(encoding="utf-8")
    import ytmusicapi
    from ytmusicapi import YTMusic
except Exception as e:  # not installed, or broken
    out({"ready": False, "error": f"ytmusicapi missing: {e}"})
    sys.exit(1)


def names(artists):
    return ", ".join(a.get("name", "") for a in (artists or []) if isinstance(a, dict) and a.get("name"))


def first_id(artists):
    for a in artists or []:
        if isinstance(a, dict) and a.get("id"):
            return a["id"]
    return None


def best_thumb(thumbs):
    thumbs = [t for t in (thumbs or []) if isinstance(t, dict) and t.get("url")]
    if not thumbs:
        return None
    return max(thumbs, key=lambda t: t.get("width") or 0)["url"]


def track(r):
    vid = r.get("videoId")
    if not vid:
        return None
    album = r.get("album") or {}
    if isinstance(album, str):
        album = {"name": album}
    return {
        "video_id": vid,
        "title": r.get("title") or "",
        "artist": names(r.get("artists")),
        "album": album.get("name") or "",
        "duration": r.get("duration") or r.get("length") or "",
        "thumb": best_thumb(r.get("thumbnails") or r.get("thumbnail")),
        "album_id": album.get("id"),
        "artist_id": first_id(r.get("artists")),
    }


def tracks(rows):
    return [t for t in (track(r) for r in rows or []) if t]


yt = YTMusic()
out({"ready": True, "version": getattr(ytmusicapi, "__version__", "")})

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    rid = None
    try:
        req = json.loads(line)
        rid = req.get("id")
        op = req.get("op")
        lang = req.get("lang") or "en"
        if op == "auth":
            cookie = req.get("cookie") or ""
            if cookie:
                yt = YTMusic(auth={"cookie": cookie, "x-goog-authuser": "0", "origin": "https://music.youtube.com"}, language=lang)
            else:
                yt = YTMusic(language=lang)
            data = True
        elif op == "search":
            data = tracks(yt.search(req["query"], filter=req.get("filter", "songs"), limit=req.get("limit", 40)))
        elif op == "radio":
            data = tracks(yt.get_watch_playlist(videoId=req["videoId"], limit=req.get("limit", 50), radio=True).get("tracks"))
        elif op == "mix":
            data = tracks(yt.get_watch_playlist(playlistId=req["playlistId"], limit=req.get("limit", 100)).get("tracks"))
        elif op == "version":
            data = getattr(ytmusicapi, "__version__", "")
        else:
            raise ValueError(f"unknown op {op!r}")
        out({"id": rid, "ok": True, "data": data})
    except Exception as e:
        out({"id": rid, "ok": False, "error": f"{type(e).__name__}: {e}"})

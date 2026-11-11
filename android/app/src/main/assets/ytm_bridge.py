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


CLUTTER = {"official", "music", "video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial",
           "officiel", "clip", "full", "videoclip", "videoclipe", "with", "on", "screen"}
CLUTTER_MARK = {"video", "audio", "lyric", "lyrics", "visualizer", "visualiser", "hd", "hq", "4k", "mv", "m/v", "oficial", "officiel"}


def is_clutter(inner):
    words = [w for w in inner.lower().replace("-", " ").replace("_", " ").split() if w]
    return bool(words) and all(w in CLUTTER for w in words) and any(w in CLUTTER_MARK for w in words)


def artist_key(name):
    key = name.strip().lower()
    while True:
        before = len(key)
        for ending in (" - topic", "- topic", "vevo", " official", "official"):
            if key.endswith(ending) and key[: -len(ending)].strip():
                key = key[: -len(ending)].rstrip()
        if len(key) == before:
            break
    return " ".join(key.split())


def clean_title(title, artist):
    """A title as a song's, not an upload's (the same rules as RyM's api::clean_title)."""
    out, rest = "", title
    while True:
        starts = [i for i in (rest.find("("), rest.find("[")) if i >= 0]
        if not starts:
            break
        i = min(starts)
        close = ")" if rest[i] == "(" else "]"
        j = rest.find(close, i + 1)
        if j < 0:
            break
        out += rest[:i]
        if not is_clutter(rest[i + 1 : j]):
            out += rest[i : j + 1]
        rest = rest[j + 1 :]
    out += rest
    for sep in (" | ", " - ", " – "):
        i = out.rfind(sep)
        if i >= 0 and is_clutter(out[i + len(sep) :]):
            out = out[:i]
    joined = " ".join(w for w in out.split() if not w.startswith("#"))
    text = joined
    first = artist_key((artist or "").replace("&", ",").split(",")[0])
    for sep in (" - ", " – "):
        if sep in joined:
            front, song = joined.split(sep, 1)
            if first and artist_key(front) == first and song.strip():
                text = song.strip()
                break
    cleaned = " ".join(text.split()).strip(" -|–")
    return cleaned or title.strip()


def track(r):
    vid = r.get("videoId")
    if not vid:
        return None
    album = r.get("album") or {}
    if isinstance(album, str):
        album = {"name": album}
    return {
        "video_id": vid,
        "title": clean_title(r.get("title") or "", names(r.get("artists"))),
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
                # "SAPISIDHASH" marks it as a browser sign-in; ytmusicapi works out the real value
                # from the cookie's SAPISID for every request.
                headers = {
                    "cookie": cookie,
                    "authorization": "SAPISIDHASH 0_0",
                    "x-goog-authuser": "0",
                    "origin": "https://music.youtube.com",
                }
                yt = YTMusic(auth=headers, language=lang)
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

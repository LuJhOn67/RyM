//! The last home feed, saved as JSON so the apps show it the moment they open; a fresh one
//! replaces it when YouTube Music answers.

use std::path::PathBuf;

use crate::api::Track;

fn path() -> PathBuf {
    crate::data_dir().join("cache-home.json")
}

pub fn save_home(tracks: &[Track]) {
    if let Ok(json) = serde_json::to_string(tracks) {
        let _ = std::fs::create_dir_all(crate::data_dir());
        let _ = std::fs::write(path(), json);
    }
}

/// The saved feed, or nothing when there isn't one (or it can't be read).
pub fn load_home() -> Vec<Track> {
    std::fs::read_to_string(path()).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// Signing out: the saved feed was that account's.
pub fn clear() {
    let _ = std::fs::remove_file(path());
}

#[cfg(test)]
mod tests {
    use crate::api::{Link, Track};

    #[test]
    fn songs_come_back_as_saved() {
        let t = Track {
            video_id: "abc".into(),
            title: "Song".into(),
            artist: "Artist".into(),
            album: "Quick picks".into(),
            duration: "3:30".into(),
            thumb: Some("https://x/y=w120".into()),
            album_id: None,
            artist_id: Some("UC1".into()),
            link: Some(Link::Mix("RDabc".into())),
        };
        let json = serde_json::to_string(&vec![t.clone()]).unwrap();
        let back: Vec<Track> = serde_json::from_str(&json).unwrap();
        assert_eq!((back[0].video_id.as_str(), back[0].album.as_str(), &back[0].link), ("abc", "Quick picks", &Some(Link::Mix("RDabc".into()))));
    }
}

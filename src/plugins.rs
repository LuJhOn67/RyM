//! Plugins: listed in the RyM-rustYoutubeMusic-plugins repository on GitHub (its `plugins.json`)
//! and switched on per user. A plugin's engine ships inside the apps; installing it from the
//! store turns it on, and a newer version in the list shows up as an update.
//!
//! The first plugin, "video", is the YouTube video player and universal downloader: without it
//! RyM stays an audio-only music player.

use std::collections::BTreeMap;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

pub const PLUGINS_REPO: &str = "LuJhOn67/RyM-rustYoutubeMusic-plugins";
pub const INDEX_URL: &str = "https://raw.githubusercontent.com/LuJhOn67/RyM-rustYoutubeMusic-plugins/main/plugins.json";
/// YouTube video player and universal downloader.
pub const VIDEO: &str = "video";

/// One plugin in the store.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plugin {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub name_ar: String,
    pub version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub description_ar: String,
    /// "windows", "linux", "android".
    #[serde(default)]
    pub platforms: Vec<String>,
}

impl Plugin {
    /// Name and description in the app's language.
    pub fn name(&self) -> &str {
        if crate::i18n::is_arabic() && !self.name_ar.is_empty() { &self.name_ar } else { &self.name }
    }

    pub fn description(&self) -> &str {
        if crate::i18n::is_arabic() && !self.description_ar.is_empty() { &self.description_ar } else { &self.description }
    }

    /// Made for this computer's system.
    pub fn for_this_pc(&self) -> bool {
        let os = if cfg!(windows) { "windows" } else if cfg!(target_os = "linux") { "linux" } else { "macos" };
        self.platforms.is_empty() || self.platforms.iter().any(|p| p == os)
    }
}

#[derive(Deserialize)]
struct Index {
    plugins: Vec<Plugin>,
}

/// The list this build came with, for when GitHub can't be reached.
const BUILT_IN: &str = include_str!("../assets/plugins.json");

/// The store's list, from GitHub (the built-in copy when it can't be reached).
pub async fn store() -> Result<Vec<Plugin>> {
    let online = async {
        crate::updates::github().get(INDEX_URL).send().await?.error_for_status().map_err(|e| anyhow!("plugins: {e}"))?.text().await.map_err(anyhow::Error::from)
    }
    .await;
    let text = online.unwrap_or_else(|_| BUILT_IN.to_string());
    Ok(parse(&text)?.into_iter().filter(Plugin::for_this_pc).collect())
}

fn parse(text: &str) -> Result<Vec<Plugin>> {
    Ok(serde_json::from_str::<Index>(text)?.plugins)
}

/// Installed plugins (id -> version) with a newer version in the store.
pub fn updates<'a>(installed: &BTreeMap<String, String>, store: &'a [Plugin]) -> Vec<&'a Plugin> {
    store.iter().filter(|p| installed.get(&p.id).is_some_and(|v| crate::updates::newer(&p.version, v))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_list_has_the_video_plugin() {
        assert!(parse(BUILT_IN).unwrap().iter().any(|p| p.id == VIDEO));
    }

    #[test]
    fn store_list_and_updates() {
        let list = parse(
            r#"{ "schema": 1, "plugins": [
                { "id": "video", "name": "Video", "name_ar": "فيديو", "version": "1.1.0", "platforms": ["windows", "linux", "android"] },
                { "id": "other", "name": "Other", "version": "0.1.0" }
            ] }"#,
        )
        .unwrap();
        assert_eq!(list.len(), 2);
        let installed = BTreeMap::from([("video".to_string(), "1.0.0".to_string())]);
        assert_eq!(updates(&installed, &list).iter().map(|p| p.id.as_str()).collect::<Vec<_>>(), ["video"]);
        let current = BTreeMap::from([("video".to_string(), "1.1.0".to_string())]);
        assert!(updates(&current, &list).is_empty());
    }
}

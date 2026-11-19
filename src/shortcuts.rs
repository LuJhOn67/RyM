//! Keyboard shortcuts the user can change (backlog 41): each action has a default key, and
//! Settings > Shortcuts sets another (kept in `Config::shortcuts`, action id -> key). A key is
//! written as text: "space", "ctrl+z", "shift+left", "f", "ctrl+=".

use std::collections::HashMap;

pub struct Action {
    pub id: &'static str,
    /// Its name in the settings (English; translated by `i18n`).
    pub name: &'static str,
    pub default: &'static str,
    /// More keys that work too (F1 to F4, as in other music players), unless another action was
    /// given them.
    pub also: &'static [&'static str],
}

/// Like the big music apps' (Spotify's, YouTube Music's): each has its key, and some a few more
/// that work too (F1 to F4, K / J, Shift+N / Shift+P, Ctrl+Up / Ctrl+Down...).
pub const ACTIONS: &[Action] = &[
    Action { id: "play", name: "Play / pause", default: "space", also: &["f2", "k"] },
    Action { id: "next", name: "Next song", default: "ctrl+right", also: &["f3", "shift+n"] },
    Action { id: "previous", name: "Previous song", default: "ctrl+left", also: &["f1", "shift+p"] },
    Action { id: "back", name: "Skip back", default: "left", also: &["j", "shift+left"] },
    Action { id: "forward", name: "Skip forward", default: "right", also: &["shift+right"] },
    Action { id: "vol-up", name: "Volume up", default: "up", also: &["ctrl+up", "=", "+"] },
    Action { id: "vol-down", name: "Volume down", default: "down", also: &["ctrl+down", "-"] },
    Action { id: "vol-max", name: "Full volume", default: "ctrl+shift+up", also: &[] },
    Action { id: "mute", name: "Mute", default: "m", also: &["f4", "ctrl+shift+down"] },
    Action { id: "like", name: "Like", default: "l", also: &["alt+shift+b"] },
    Action { id: "repeat", name: "Repeat", default: "r", also: &["ctrl+r"] },
    Action { id: "lyrics", name: "Show the lyrics", default: "y", also: &[] },
    Action { id: "stage", name: "Full-screen lyrics", default: "f", also: &["f11"] },
    Action { id: "mini", name: "Mini player", default: "ctrl+m", also: &[] },
    Action { id: "search", name: "Search", default: "ctrl+l", also: &["ctrl+k", "/"] },
    Action { id: "home", name: "Home", default: "alt+shift+h", also: &[] },
    Action { id: "now", name: "Now playing", default: "alt+shift+n", also: &[] },
    Action { id: "library", name: "Library", default: "alt+shift+l", also: &[] },
    Action { id: "stats", name: "Your stats", default: "alt+shift+s", also: &[] },
    Action { id: "settings", name: "Settings", default: "ctrl+,", also: &["ctrl+p"] },
    Action { id: "lyrics-sooner", name: "Lyrics sooner", default: ",", also: &[] },
    Action { id: "lyrics-later", name: "Lyrics later", default: ".", also: &[] },
    Action { id: "undo", name: "Undo removing from the queue", default: "ctrl+z", also: &[] },
    Action { id: "zoom-in", name: "Bigger", default: "ctrl+=", also: &[] },
    Action { id: "zoom-out", name: "Smaller", default: "ctrl+-", also: &[] },
    Action { id: "zoom-reset", name: "Normal size", default: "ctrl+0", also: &[] },
    Action { id: "theme", name: "Next theme", default: "ctrl+t", also: &[] },
    Action { id: "quit", name: "Quit RyM", default: "ctrl+q", also: &[] },
];

/// A key press as text. `key` is a character, or a name for the keys without one ("space",
/// "left", "enter"...). Shift is written for letters and named keys; for a symbol it's already
/// in the character ("+" is shift and "=").
pub fn combo(ctrl: bool, alt: bool, shift: bool, key: &str) -> String {
    let key = if key == " " { "space".to_string() } else { key.to_lowercase() };
    let named = key.chars().count() > 1;
    let letter = key.chars().next().is_some_and(|c| c.is_alphabetic());
    let mut out = String::new();
    if ctrl {
        out.push_str("ctrl+");
    }
    if alt {
        out.push_str("alt+");
    }
    if shift && (named || letter) {
        out.push_str("shift+");
    }
    // Ctrl and + (shift and =) is the same as Ctrl and =.
    out.push_str(if key == "+" && ctrl { "=" } else { &key });
    out
}

/// The key set for `id` (the user's, or the default).
pub fn key_of(custom: &HashMap<String, String>, id: &str) -> String {
    custom.get(id).cloned().or_else(|| ACTIONS.iter().find(|a| a.id == id).map(|a| a.default.to_string())).unwrap_or_default()
}

/// The action the key press `combo` does: the set keys first, then the second keys (F1 to F4).
pub fn action_for(custom: &HashMap<String, String>, combo: &str) -> Option<&'static str> {
    ACTIONS
        .iter()
        .find(|a| key_of(custom, a.id) == combo)
        .or_else(|| ACTIONS.iter().find(|a| a.also.contains(&combo)))
        .map(|a| a.id)
}

/// `id` gets `combo`; an action that had it takes `id`'s old key (they swap).
pub fn rebind(custom: &mut HashMap<String, String>, id: &str, combo: &str) {
    let old = key_of(custom, id);
    if let Some(other) = action_for(custom, combo).filter(|o| *o != id) {
        custom.insert(other.to_string(), old);
    }
    custom.insert(id.to_string(), combo.to_string());
    // A key back on its default needn't be kept.
    custom.retain(|k, v| ACTIONS.iter().find(|a| a.id == k).is_some_and(|a| a.default != v));
}

/// A plain key (no Ctrl or Alt) that would type into a text box, so not while one is in use
/// (F1 to F12 type nothing).
pub fn types(combo: &str) -> bool {
    let fkey = combo.strip_prefix('f').is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    !combo.starts_with("ctrl+") && !combo.starts_with("alt+") && !fkey
}

/// How a key is shown: "Ctrl+Z", "Space", "←".
pub fn label(combo: &str) -> String {
    combo
        .split('+')
        .enumerate()
        .map(|(i, part)| {
            let last = i == combo.split('+').count() - 1;
            match part {
                "ctrl" if !last => "Ctrl".to_string(),
                "alt" if !last => "Alt".to_string(),
                "shift" if !last => "Shift".to_string(),
                "space" => "Space".to_string(),
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                "" => "+".to_string(),
                p if p.chars().count() == 1 => p.to_uppercase(),
                p => {
                    let mut c = p.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
        .replace("++", "+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_written_one_way() {
        assert_eq!(combo(false, false, false, " "), "space");
        assert_eq!(combo(true, false, true, "Z"), "ctrl+shift+z");
        assert_eq!(combo(true, false, false, "+"), "ctrl+=");
        assert_eq!(combo(false, false, true, "?"), "?");
        assert_eq!(combo(false, false, true, "left"), "shift+left");
        assert_eq!(label("ctrl+shift+z"), "Ctrl+Shift+Z");
        assert_eq!(label("space"), "Space");
        assert_eq!(label("ctrl+left"), "Ctrl+←");
        assert_eq!(label("ctrl+="), "Ctrl+=");
    }

    #[test]
    fn a_new_key_swaps_with_the_action_that_had_it() {
        let mut m = HashMap::new();
        assert_eq!(action_for(&m, "space"), Some("play"));
        rebind(&mut m, "play", "k");
        assert_eq!(action_for(&m, "k"), Some("play"));
        assert_eq!(action_for(&m, "space"), None);
        // "l" is Like's: Like gets play's old key ("k").
        rebind(&mut m, "play", "l");
        assert_eq!(action_for(&m, "l"), Some("play"));
        assert_eq!(action_for(&m, "k"), Some("like"));
        // Back to the defaults: nothing kept.
        rebind(&mut m, "like", "l");
        rebind(&mut m, "play", "space");
        assert!(m.is_empty(), "{m:?}");
        assert!(types("f") && !types("ctrl+t") && !types("f2"));
        // F1 to F4 work beside the set keys; a key set on purpose wins over them.
        let mut m = HashMap::new();
        assert_eq!((action_for(&m, "f1"), action_for(&m, "f2"), action_for(&m, "f3"), action_for(&m, "f4")), (Some("previous"), Some("play"), Some("next"), Some("mute")));
        rebind(&mut m, "like", "f3");
        assert_eq!(action_for(&m, "f3"), Some("like"));
    }
}

//! Keyboard shortcuts the user can change (backlog 41): each action has a default key, and
//! Settings > Shortcuts sets another (kept in `Config::shortcuts`, action id -> key). A key is
//! written as text: "space", "ctrl+z", "shift+left", "f", "ctrl+=".

use std::collections::HashMap;

pub struct Action {
    pub id: &'static str,
    /// Its name in the settings (English; translated by `i18n`).
    pub name: &'static str,
    pub default: &'static str,
}

pub const ACTIONS: &[Action] = &[
    Action { id: "play", name: "Play / pause", default: "space" },
    Action { id: "next", name: "Next song", default: "ctrl+right" },
    Action { id: "previous", name: "Previous song", default: "ctrl+left" },
    Action { id: "back", name: "Skip back", default: "left" },
    Action { id: "forward", name: "Skip forward", default: "right" },
    Action { id: "vol-up", name: "Volume up", default: "up" },
    Action { id: "vol-down", name: "Volume down", default: "down" },
    Action { id: "like", name: "Like", default: "l" },
    Action { id: "repeat", name: "Repeat", default: "r" },
    Action { id: "lyrics", name: "Show the lyrics", default: "y" },
    Action { id: "stage", name: "Full-screen lyrics", default: "f" },
    Action { id: "lyrics-sooner", name: "Lyrics sooner", default: "," },
    Action { id: "lyrics-later", name: "Lyrics later", default: "." },
    Action { id: "undo", name: "Undo removing from the queue", default: "ctrl+z" },
    Action { id: "zoom-in", name: "Bigger", default: "ctrl+=" },
    Action { id: "zoom-out", name: "Smaller", default: "ctrl+-" },
    Action { id: "zoom-reset", name: "Normal size", default: "ctrl+0" },
    Action { id: "theme", name: "Next theme", default: "ctrl+t" },
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

/// The action the key press `combo` does.
pub fn action_for(custom: &HashMap<String, String>, combo: &str) -> Option<&'static str> {
    ACTIONS.iter().find(|a| key_of(custom, a.id) == combo).map(|a| a.id)
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

/// A plain key (no Ctrl or Alt): it would type into a text box, so not while one is in use.
pub fn types(combo: &str) -> bool {
    !combo.starts_with("ctrl+") && !combo.starts_with("alt+")
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
        assert!(types("f") && !types("ctrl+t"));
    }
}

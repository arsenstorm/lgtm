//! The Lucide icon set, compiled into the binary. `gpui` asks for
//! `icons/<name>.svg`; nothing else is served.

use anyhow::Result;
use gpui::{AssetSource, SharedString};
use std::borrow::Cow;

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        /// Every icon the app draws.
        pub const NAMES: &[&str] = &[$($name),*];

        fn bytes(path: &str) -> Option<&'static [u8]> {
            match path {
                $(concat!("icons/", $name, ".svg") => {
                    Some(include_bytes!(concat!("../assets/icons/", $name, ".svg")))
                })*
                _ => None,
            }
        }
    };
}

icons![
    "activity",
    "arrow-up",
    "check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "circle-dot",
    "cpu",
    "ellipsis",
    "folder",
    "git-branch",
    "lightbulb",
    "list-checks",
    "panel-left",
    "plus",
    "search",
    "server",
    "settings",
    "square-pen",
    "trash-2",
    "x",
];

pub struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(bytes(path).map(Cow::Borrowed))
    }

    fn list(&self, _path: &str) -> Result<Vec<SharedString>> {
        Ok(NAMES
            .iter()
            .map(|name| SharedString::from(format!("icons/{name}.svg")))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_icon_is_embedded_and_is_an_svg() {
        for name in NAMES {
            let bytes = Assets
                .load(&format!("icons/{name}.svg"))
                .unwrap()
                .unwrap_or_else(|| panic!("{name} is missing"));
            assert!(std::str::from_utf8(&bytes).unwrap().contains("<svg"));
        }
        assert!(Assets.load("icons/nope.svg").unwrap().is_none());
    }
}

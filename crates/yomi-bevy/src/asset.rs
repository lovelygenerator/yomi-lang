//! The asset of a script file.

use bevy_asset::io::Reader;
use bevy_asset::{Asset, AssetLoader, LoadContext};
use bevy_reflect::TypePath;
use yomi_core::TextTable;

/// The source of a `.yomi` file.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct YomiScript {
    /// The text of the file (UTF-8).
    pub source: String,
}

/// Reads `.yomi` files as [`YomiScript`].
#[derive(Default, TypePath)]
pub struct YomiScriptLoader;

impl AssetLoader for YomiScriptLoader {
    type Asset = YomiScript;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<YomiScript, std::io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let source = String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        Ok(YomiScript { source })
    }

    fn extensions(&self) -> &[&str] {
        &["yomi"]
    }
}

/// A table of replacement bodies (a `.texts` file), such as the translations of a language.
#[derive(Asset, TypePath, Debug, Clone, PartialEq, Eq)]
pub struct YomiTexts {
    /// The table.
    pub table: TextTable,
}

/// Reads `.texts` files as [`YomiTexts`].
#[derive(Default, TypePath)]
pub struct YomiTextsLoader;

impl AssetLoader for YomiTextsLoader {
    type Asset = YomiTexts;
    type Settings = ();
    type Error = std::io::Error;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &(),
        _load_context: &mut LoadContext<'_>,
    ) -> Result<YomiTexts, std::io::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;
        let source = String::from_utf8(bytes)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let table = TextTable::from_file(&source)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        Ok(YomiTexts { table })
    }

    fn extensions(&self) -> &[&str] {
        &["texts"]
    }
}

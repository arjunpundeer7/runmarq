//! Product naming lives in `packages/config/brand.json` so the name can change
//! without touching application code. Rust reads it at compile time.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Brand {
    pub product_name: String,
    pub cli_name: String,
    pub data_dir_name: String,
    pub bundle_identifier: String,
    pub tagline: String,
}

const BRAND_JSON: &str = include_str!("../../../packages/config/brand.json");

pub fn brand() -> &'static Brand {
    static BRAND: OnceLock<Brand> = OnceLock::new();
    BRAND.get_or_init(|| serde_json::from_str(BRAND_JSON).expect("brand.json is valid"))
}

/// The command name used before the product was renamed. Hook entries that
/// invoke it are still recognised as ours, so setup replaces them and
/// removal takes them out.
pub const LEGACY_CLI_NAME: &str = "runmark";

/// The data directory name used before the rename. An install found there is
/// moved to the current name once, by `migrate` in the collector crate.
pub const LEGACY_DATA_DIR_NAME: &str = "Runmark";

/// Marker written into every configuration entry runmarq manages, so that
/// uninstall removes only what runmarq added.
pub fn managed_marker() -> String {
    format!("{}-managed", brand().cli_name)
}

#[cfg(test)]
mod tests {
    #[test]
    fn brand_parses() {
        let b = super::brand();
        assert!(!b.product_name.is_empty());
        assert!(!b.cli_name.contains(' '));
    }
}

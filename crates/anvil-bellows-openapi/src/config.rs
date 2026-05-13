//! Config file deserialization for `anvil-bellows-openapi`.
//!
//! Config files are JSON with the shape:
//!
//! ```json
//! {
//!   "info": { "title": "My API", "version": "1.0.0" },
//!   "servers": ["https://api.example.com"],
//!   "securitySchemes": {
//!     "bearerAuth": { "type": "http", "scheme": "bearer" }
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// Top-level config loaded from disk.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiConfig {
    /// `OpenAPI` `info` object.
    #[serde(default)]
    pub info: InfoConfig,
    /// Server base URLs (simplified — just strings for static mode).
    #[serde(default)]
    pub servers: Vec<String>,
    /// Named security schemes for `@Security` decorator validation.
    #[serde(default)]
    pub security_schemes: BTreeMap<String, SecuritySchemeConfig>,
}

/// `OpenAPI` `info` object.
#[derive(Debug, Clone, Deserialize)]
pub struct InfoConfig {
    /// API title.
    #[serde(default = "default_title")]
    pub title: String,
    /// API version string.
    #[serde(default = "default_version")]
    pub version: String,
}

impl Default for InfoConfig {
    fn default() -> Self {
        Self {
            title: default_title(),
            version: default_version(),
        }
    }
}

fn default_title() -> String {
    "API".to_owned()
}
fn default_version() -> String {
    "0.0.1".to_owned()
}

/// A security scheme definition.
#[derive(Debug, Clone, Deserialize)]
pub struct SecuritySchemeConfig {
    /// Scheme type: `"http"`, `"apiKey"`, `"openIdConnect"`, `"oauth2"`.
    pub r#type: String,
    /// For `type: "http"`: the scheme name (`"bearer"`, `"basic"`, …).
    pub scheme: Option<String>,
    /// For `type: "apiKey"`: the header/query/cookie name.
    pub name: Option<String>,
    /// For `type: "apiKey"`: where the key appears (`"header"`, `"query"`, `"cookie"`).
    pub r#in: Option<String>,
}

/// Load config from a JSON file, or return defaults if the path is `None`.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_config(path: Option<&Path>) -> anyhow::Result<OpenApiConfig> {
    let Some(p) = path else {
        return Ok(OpenApiConfig::default());
    };
    let raw = std::fs::read_to_string(p)?;
    let cfg: OpenApiConfig = serde_json::from_str(&raw)?;
    Ok(cfg)
}

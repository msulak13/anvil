use std::path::Path;
use anvil_core::ir::{Binding, ParsedFile};
use extism::{Plugin, Manifest, Wasm, Error as ExtismError};
use serde::Serialize;

pub struct PluginRunner {
    plugin: Plugin,
    name: String,
}

#[derive(Serialize)]
struct PluginInput<'a> {
    parsed_file: &'a ParsedFile,
    // Future expansion: config
}

impl PluginRunner {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let wasm = Wasm::file(path);
        let manifest = Manifest::new([wasm]);
        let plugin = Plugin::new(&manifest, [], true)
            .map_err(|e: ExtismError| anyhow::anyhow!("Failed to initialize plugin {}: {}", path.display(), e))?;

        Ok(Self {
            plugin,
            name: path.file_name().unwrap_or_default().to_string_lossy().into_owned(),
        })
    }

    pub fn apply(&mut self, parsed: &ParsedFile) -> anyhow::Result<Vec<Binding>> {
        let input = PluginInput {
            parsed_file: parsed,
        };
        let input_json = serde_json::to_vec(&input)?;
        let output = self.plugin.call::<&[u8], Vec<u8>>("extract_bindings", &input_json);
        match output {
            Ok(bytes) => {
                let bindings: Vec<Binding> = serde_json::from_slice(&bytes)?;
                Ok(bindings)
            }
            Err(e) => {
                // If the function is not found, we can just skip it, 
                // but let's assume valid plugins have `extract_bindings`.
                Err(anyhow::anyhow!("Plugin {} failed during extract_bindings: {}", self.name, e))
            }
        }
    }
}

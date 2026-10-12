use std::path::PathBuf;

use serde::{Deserialize, Deserializer, de::Error};

use crate::flsproject::FlsProjectConfig;

#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Configuration {
    pub(crate) fennel: Fennel,
    pub(crate) fls: FlsProjectConfig,
}

impl Configuration {
    pub(crate) fn is_lint_enabled(&self, lint: &str) -> bool {
        self.fls.lints.get(lint).copied().unwrap_or(true)
    }
}

impl<'de> Deserialize<'de> for Configuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v: serde_json::Value = Deserialize::deserialize(deserializer)?;
        let mut config = Configuration::default();

        if let Some(fennel_val) = v.get("fennel") {
            if let Ok(fennel) = Fennel::deserialize(fennel_val.clone()) {
                config.fennel = fennel;
            }
        } else if let Ok(fennel) = Fennel::deserialize(v.clone()) {
            config.fennel = fennel;
        }

        if let Some(fls_val) = v.get("fennel-ls").or_else(|| v.get("fennelLs")) {
            if let Ok(fls) = FlsProjectConfig::deserialize(fls_val.clone()) {
                config.fls = fls;
            }
        } else if (v.get("fennel-path").is_some()
            || v.get("fennelPath").is_some()
            || v.get("extra-globals").is_some()
            || v.get("extraGlobals").is_some()
            || v.get("lua-version").is_some()
            || v.get("luaVersion").is_some()
            || v.get("lints").is_some())
            && let Ok(fls) = FlsProjectConfig::deserialize(v.clone())
        {
            config.fls = fls;
        }

        Ok(config)
    }
}

#[derive(Deserialize, Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Fennel {
    #[serde(default)]
    pub(crate) workspace: Workspace,
    #[serde(default)]
    pub(crate) diagnostics: Diagnostics,
}

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Workspace {
    pub(crate) library: Vec<Url>,
}

impl<'de> Deserialize<'de> for Workspace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WorkspaceHelper {
            #[serde(default)]
            library: Vec<String>,
        }

        let helper = WorkspaceHelper::deserialize(deserializer)?;
        let library = helper
            .library
            .into_iter()
            .filter_map(|s| {
                tower_lsp::lsp_types::Url::from_directory_path(PathBuf::from(&s))
                    .or_else(|_| tower_lsp::lsp_types::Url::from_file_path(PathBuf::from(&s)))
                    .or_else(|_| tower_lsp::lsp_types::Url::parse(&s))
                    .map(Url)
                    .ok()
            })
            .collect();

        Ok(Workspace { library })
    }
}

#[derive(Deserialize, Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Diagnostics {
    #[serde(default)]
    pub(crate) globals: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Url(pub(crate) tower_lsp::lsp_types::Url);

impl<'de> Deserialize<'de> for Url {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        // Try from_directory_path first, then from_file_path, and fallback gracefully
        tower_lsp::lsp_types::Url::from_directory_path(PathBuf::from(&s))
            .or_else(|_| tower_lsp::lsp_types::Url::from_file_path(PathBuf::from(&s)))
            .or_else(|_| tower_lsp::lsp_types::Url::parse(&s))
            .map(Self)
            .map_err(|_e| D::Error::custom(format!("invalid path or url: {}", s)))
    }
}

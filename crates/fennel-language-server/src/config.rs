use std::path::PathBuf;

use serde::{Deserialize, Deserializer, de::Error};

#[derive(Default, Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Configuration {
    pub(crate) fennel: Fennel,
}

impl<'de> Deserialize<'de> for Configuration {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v: serde_json::Value = Deserialize::deserialize(deserializer)?;
        if v.get("fennel").is_some() {
            #[derive(Deserialize)]
            struct Temp {
                #[serde(default)]
                fennel: Fennel,
            }
            let temp = Temp::deserialize(v).map_err(D::Error::custom)?;
            Ok(Configuration { fennel: temp.fennel })
        } else {
            // Try to deserialize directly as Fennel if the prefix is missing
            let fennel = Fennel::deserialize(v).map_err(D::Error::custom)?;
            Ok(Configuration { fennel })
        }
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
            .map_err(|_| D::Error::custom(format!("invalid path or url: {}", s)))
    }
}

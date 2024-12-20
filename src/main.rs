use std::collections::HashMap;

use crate::services::authentik::AuthentikConfig;
use crate::services::gitlab::GitLabConfig;
use crate::services::keycloak::KeycloakConfig;
use crate::services::Service;
use clap::Parser;
use nextcloud_table::Nextcloud;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tokio;

mod nextcloud_table;
mod services;

fn true_bool() -> bool {
    true
}

fn false_bool() -> bool {
    false
}

#[derive(Parser)]
#[command(
    version,
    author = "Florian Schubert",
    about = "Program um die Benutzer:innen im AStA zu verwalten.",
    name = "benutzerverwaltungstool",
    color = clap::ColorChoice::Always
)]
struct Args {
    #[clap(short, long)]
    config: String,
}

#[derive(Deserialize, Serialize, Debug)]
struct Config {
    users_provider: UserConfigProvider,
    keycloak: Option<KeycloakConfig>,
    authentik: Option<AuthentikConfig>,
    gitlab: Option<GitLabConfig>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserConfigProvider {
    File(String),
    NextcloudTable { nextcloud: Nextcloud, table_id: u64 },
}

#[skip_serializing_none]
#[derive(Deserialize, Serialize, Debug)]
pub(crate) struct UserConfig {
    first_name: Option<String>,
    last_name: Option<String>,
    email: Option<String>,
    matrix_id: Option<String>,
    roles: Vec<String>,
    #[serde(default = "true_bool")]
    enabled: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    //Set Log Level to Info
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .init();

    let args: Args = Args::parse();
    let config = std::fs::read_to_string(args.config)?;
    let config: Config = serde_json::from_str(&config)?;

    let user_configs: HashMap<String, UserConfig> = match config.users_provider {
        UserConfigProvider::NextcloudTable {
            nextcloud,
            table_id,
        } => nextcloud_table::get_user_configs(&nextcloud, table_id).await?,
        UserConfigProvider::File(path) => serde_json::from_str(&std::fs::read_to_string(path)?)?,
    };

    if let Some(keycloak_config) = &config.keycloak {
        keycloak_config.configure(&user_configs).await?;
    }

    if let Some(authentik_config) = &config.authentik {
        authentik_config.configure(&user_configs).await?;
    }

    if let Some(gitlab_config) = &config.gitlab {
        gitlab_config.configure(&user_configs).await?;
    }

    Ok(())
}

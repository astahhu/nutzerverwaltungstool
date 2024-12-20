use std::collections::HashMap;

use crate::UserConfig;

pub mod authentik;
pub mod gitlab;
pub mod keycloak;

pub trait Service {
    async fn configure(&self, users: &HashMap<String, UserConfig>) -> anyhow::Result<()>;
}

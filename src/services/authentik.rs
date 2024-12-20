use std::collections::HashMap;

use crate::true_bool;
use crate::UserConfig;
use log::*;
use oauth2::AccessToken;
use serde_json::json;
use uuid::Uuid;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct AuthentikConfig {
    pub url: String,
    pub token: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct AuthentikResponse {
    pub results: Vec<AuthentikUser>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct AuthentikResponse2 {
    pub results: Vec<AuthentikRole>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct AuthentikUser {
    pk: i64,
    username: String,
    email: Option<String>,
    name: Option<String>,
    #[serde(default = "true_bool")]
    is_active: bool,
    groups_obj: Vec<AuthentikRole>,
}

struct AuthentikClient {
    base_url: String,
    token: AccessToken,
    reqwest_client: reqwest::Client,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq)]
struct AuthentikRole {
    pk: Uuid,
    name: String,
}

impl crate::services::Service for AuthentikConfig {
    async fn configure(&self, users: &HashMap<String, UserConfig>) -> anyhow::Result<()> {
        let client = AuthentikClient::new(self.url.clone(), self.token.clone()).await?;

        let authentik_users = client.get_all_users().await?;

        let users_to_create = users
            .iter()
            .filter(|user| !authentik_users.iter().any(|k| *user.0 == k.username))
            .collect::<HashMap<_, _>>();

        client.create_users(&users_to_create).await?;

        let users_to_update = authentik_users
            .iter()
            .filter(|authentik_user| users.contains_key(&authentik_user.username))
            .collect::<Vec<_>>();

        client.update_users(&users_to_update, &users).await?;
        client.update_roles(&users_to_update, &users).await?;

        let users_to_delete = authentik_users
            .iter()
            .filter(|authentik_user| !users.contains_key(&authentik_user.username))
            .collect::<Vec<_>>();
        client.delete_users(&users_to_delete).await?;

        Ok(())
    }
}

impl AuthentikClient {
    async fn new(base_url: String, token_string: String) -> anyhow::Result<Self> {
        let token = AccessToken::new(token_string);
        // Get a Token with Password Grant

        Ok(AuthentikClient {
            base_url,
            token,
            reqwest_client: reqwest::Client::new(),
        })
    }

    async fn create_users(&self, users: &HashMap<&String, &UserConfig>) -> anyhow::Result<()> {
        for user in users {
            let user = self
                .reqwest_client
                .post(format!("{}/api/v3/core/users/", self.base_url))
                .bearer_auth(self.token.secret())
                .json(&json!(
                    {
                        "username": user.0,
                        "name": format!("{} {}", user.1.first_name.clone().unwrap_or("".to_string()), user.1.last_name.clone().unwrap_or("".to_string())),
                        "email": user.1.email,
                        "is_active": user.1.enabled,
                    }
                ))
                .send()
                .await?
                .text()
                .await?;
            info!("Created User: {:?}", user);
        }
        Ok(())
    }

    async fn get_all_users(&self) -> anyhow::Result<Vec<AuthentikUser>> {
        info!("Getting all users from Authentik");
        // Create a request
        Ok(self
            .reqwest_client
            .get(format!("{}/api/v3/core/users/", self.base_url))
            .bearer_auth(self.token.secret())
            .send()
            .await?
            .json::<AuthentikResponse>()
            .await?
            .results)
    }

    async fn disable_users(&self, users: &Vec<&AuthentikUser>) -> anyhow::Result<()> {
        for user in users {
            info!("Disabling user: {}", user.username);
            let _ = self
                .reqwest_client
                .put(format!("{}/api/v3/core/users/{}", self.base_url, user.pk))
                .bearer_auth(self.token.secret())
                .json(&json!({
                    "is_active": false
                }))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn delete_users(&self, users: &Vec<&AuthentikUser>) -> anyhow::Result<()> {
        for user in users {
            info!("Deleting user: {}", user.username);
            let _ = self
                .reqwest_client
                .delete(format!("{}/api/v3//core/users/{}", self.base_url, user.pk))
                .bearer_auth(self.token.secret())
                .json(&json!({
                    "is_active": false
                }))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn get_all_realm_roles(&self) -> anyhow::Result<Vec<AuthentikRole>> {
        info!("Getting all realm roles from Authentik");
        Ok(self
            .reqwest_client
            .get(format!("{}/api/v3/core/groups/", self.base_url))
            .bearer_auth(self.token.secret())
            .send()
            .await?
            .json::<AuthentikResponse2>()
            .await?
            .results)
    }

    fn roles_to_add(
        config_roles: &Vec<String>,
        authentik_roles: &Vec<AuthentikRole>,
    ) -> Vec<AuthentikRole> {
        authentik_roles
            .iter()
            .filter(|role| config_roles.contains(&role.name))
            .cloned()
            .collect()
    }

    async fn update_roles(
        &self,
        users_authentik: &Vec<&AuthentikUser>,
        user_configs: &HashMap<String, UserConfig>,
    ) -> anyhow::Result<()> {
        info!("Updating roles for users");
        let existing_roles = self.get_all_realm_roles().await?;
        for user in users_authentik {
            let configured_roles = user_configs[&user.username].roles.clone();
            let roles = Self::roles_to_add(&configured_roles, &existing_roles);

            self.update_user_roles(&format!("{:?}", &user.pk), &roles)
                .await?;
        }
        Ok(())
    }

    async fn update_user_roles(
        &self,
        user_id: &String,
        roles: &Vec<AuthentikRole>,
    ) -> anyhow::Result<()> {
        info!("Updating roles for user: {}", user_id);
        let r = &json!({"groups": roles.iter().map(|role| role.pk).collect::<Vec<Uuid>>()});
        println!("{}", r);
        if !roles.is_empty() {
            match self
                .reqwest_client
                .patch(format!("{}/api/v3/core/users/{}/", self.base_url, user_id))
                .bearer_auth(self.token.secret())
                .json(&json!({"groups": roles.iter().map(|role| role.pk).collect::<Vec<Uuid>>()}))
                .send()
                .await?
                .status()
            {
                reqwest::StatusCode::OK => {
                    info!("Added roles: {:?} to {:?}", roles, user_id)
                }
                status => error!("Failed to add roles to user: {}", status),
            }
        }
        Ok(())
    }

    async fn update_users(
        &self,
        users: &Vec<&AuthentikUser>,
        user_configs: &HashMap<String, UserConfig>,
    ) -> anyhow::Result<()> {
        for user in users {
            let user_config = &user_configs[&user.username];
            self.update_user(&user, &user_config).await?;
        }
        Ok(())
    }

    async fn update_user(
        &self,
        user: &AuthentikUser,
        user_config: &UserConfig,
    ) -> anyhow::Result<()> {
        self.reqwest_client
            .put(format!("{}/api/v3//core/users/{}/", self.base_url, user.pk))
            .bearer_auth(self.token.secret())
            .json(&json!(
                {
                    "name": format!("{:?} {:?}", user_config.first_name, user_config.last_name),
                    "email": user_config.email,
                    "is_active": user_config.enabled,
                    "username": user.username
                }
            ))
            .send()
            .await?;
        Ok(())
    }
}

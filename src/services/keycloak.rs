use std::collections::HashMap;

use log::*;
use oauth2::basic::BasicClient;
use oauth2::reqwest::async_http_client;
use oauth2::AccessToken;
use oauth2::ClientId;
use oauth2::TokenResponse;
use serde_json::json;

use crate::services::Service;
use crate::true_bool;
use crate::UserConfig;

#[derive(serde::Deserialize, serde::Serialize, Debug)]
pub struct KeycloakConfig {
    pub url: String,
    pub realm: String,
    pub username: String,
    pub password: String,
    pub client_id: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct KeycloakUser {
    id: String,
    username: String,
    email: Option<String>,
    first_name: Option<String>,
    last_name: Option<String>,
    #[serde(default = "true_bool")]
    enabled: bool,
}

struct KeycloakClient {
    base_url: String,
    realm: String,
    token: AccessToken,
    reqwest_client: reqwest::Client,
}

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone, PartialEq)]
struct KeycloakRole {
    id: String,
    name: String,
}

impl Service for KeycloakConfig {
    async fn configure(&self, users: &HashMap<String, UserConfig>) -> anyhow::Result<()> {
        let client = KeycloakClient::new(
            self.url.clone(),
            self.realm.clone(),
            self.username.clone(),
            self.password.clone(),
            self.client_id.clone(),
        )
        .await?;

        let keycloak_users = client.get_all_users().await?;

        let users_to_create = users
            .iter()
            .filter(|user| !keycloak_users.iter().any(|k| *user.0 == k.username))
            .collect::<HashMap<_, _>>();

        client.create_users(&users_to_create).await?;

        let users_to_update = keycloak_users
            .iter()
            .filter(|keycloak_user| users.contains_key(&keycloak_user.username))
            .collect::<Vec<_>>();

        client.update_users(&users_to_update, &users).await?;
        client.update_roles(&users_to_update, &users).await?;

        let users_to_delete = keycloak_users
            .iter()
            .filter(|keycloak_user| !users.contains_key(&keycloak_user.username))
            .collect::<Vec<_>>();
        client.delete_users(&users_to_delete).await?;

        Ok(())
    }
}

impl KeycloakClient {
    async fn new(
        base_url: String,
        realm: String,
        user: String,
        password: String,
        client_id: String,
    ) -> anyhow::Result<Self> {
        let oauth_client = BasicClient::new(
            ClientId::new(client_id),
            None,
            oauth2::AuthUrl::new(format!(
                "{}/realms/master/protocol/openid-connect/auth",
                base_url
            ))
            .unwrap(),
            Some(
                oauth2::TokenUrl::new(format!(
                    "{}/realms/master/protocol/openid-connect/token",
                    base_url
                ))
                .unwrap(),
            ),
        );
        // Get a Token with Password Grant
        let token = oauth_client
            .exchange_password(
                &oauth2::ResourceOwnerUsername::new(user.clone()),
                &oauth2::ResourceOwnerPassword::new(password.clone()),
            )
            .request_async(async_http_client)
            .await?
            .access_token()
            .clone();

        Ok(KeycloakClient {
            base_url,
            realm,
            token,
            reqwest_client: reqwest::Client::new(),
        })
    }

    async fn create_users(&self, users: &HashMap<&String, &UserConfig>) -> anyhow::Result<()> {
        for user in users {
            let user = self
                .reqwest_client
                .post(format!(
                    "{}/admin/realms/{}/users",
                    self.base_url, self.realm
                ))
                .bearer_auth(&self.token.secret())
                .json(&json!(
                    {
                        "username": user.0,
                        "firstName": user.1.first_name,
                        "lastName": user.1.last_name,
                        "email": user.1.email,
                        "enabled": user.1.enabled,
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

    async fn get_all_users(&self) -> anyhow::Result<Vec<KeycloakUser>> {
        debug!("Getting all users from Keycloak");
        // Create a request
        Ok(self
            .reqwest_client
            .get(format!(
                "{}/admin/realms/{}/users",
                self.base_url, self.realm
            ))
            .bearer_auth(self.token.secret())
            .send()
            .await?
            .json::<Vec<KeycloakUser>>()
            .await?)
    }

    async fn disable_users(&self, users: &Vec<&KeycloakUser>) -> anyhow::Result<()> {
        for user in users {
            debug!("Disabling user: {}", user.username);
            let _ = self
                .reqwest_client
                .put(format!(
                    "{}/admin/realms/{}/users/{}",
                    self.base_url, self.realm, user.id
                ))
                .bearer_auth(self.token.secret())
                .json(&json!({
                    "enabled": false
                }))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn delete_users(&self, users: &Vec<&KeycloakUser>) -> anyhow::Result<()> {
        for user in users {
            info!("Deleting user: {}", user.username);
            let _ = self
                .reqwest_client
                .delete(format!(
                    "{}/admin/realms/{}/users/{}",
                    self.base_url, self.realm, user.id
                ))
                .bearer_auth(self.token.secret())
                .json(&json!({
                    "enabled": false
                }))
                .send()
                .await?;
        }
        Ok(())
    }

    async fn get_all_realm_roles(&self) -> anyhow::Result<Vec<KeycloakRole>> {
        debug!("Getting all realm roles from Keycloak");
        Ok(self
            .reqwest_client
            .get(format!(
                "{}/admin/realms/{}/roles",
                self.base_url, self.realm
            ))
            .bearer_auth(self.token.secret())
            .send()
            .await?
            .json::<Vec<KeycloakRole>>()
            .await?)
    }

    async fn get_realm_roles(&self, user: &KeycloakUser) -> anyhow::Result<Vec<KeycloakRole>> {
        debug!("Getting realm roles for user: {}", user.username);
        Ok(self
            .reqwest_client
            .get(format!(
                "{}/admin/realms/{}/users/{}/role-mappings/realm",
                self.base_url, self.realm, user.id
            ))
            .bearer_auth(self.token.secret())
            .send()
            .await?
            .json::<Vec<KeycloakRole>>()
            .await?)
    }

    async fn create_realm_role(&self, role: String) -> anyhow::Result<()> {
        self.reqwest_client
            .post(format!(
                "{}/admin/realms/{}/roles",
                self.base_url, self.realm
            ))
            .bearer_auth(self.token.secret())
            .json(&json!({ "name": role }))
            .send()
            .await?;
        Ok(())
    }

    fn roles_to_add(
        config_roles: &Vec<String>,
        keycloak_roles: &Vec<KeycloakRole>,
        existing_roles: &Vec<KeycloakRole>,
    ) -> Vec<KeycloakRole> {
        keycloak_roles
            .iter()
            .filter(|role| config_roles.contains(&role.name))
            .filter(|role| !existing_roles.contains(role))
            .cloned()
            .collect()
    }

    fn roles_to_remove(
        config_roles: &Vec<String>,
        keycloak_roles: &Vec<KeycloakRole>,
    ) -> Vec<KeycloakRole> {
        keycloak_roles
            .iter()
            .filter(|role| !config_roles.contains(&role.name))
            .cloned()
            .collect()
    }

    async fn update_roles(
        &self,
        users_keycloak: &Vec<&KeycloakUser>,
        user_configs: &HashMap<String, UserConfig>,
    ) -> anyhow::Result<()> {
        debug!("Updating roles for users");
        let keycloak_roles = self.get_all_realm_roles().await?;
        for roles_to_add in user_configs
            .iter()
            .map(|(_, users)| users.roles.clone())
            .flatten()
            .filter(|r| !keycloak_roles.iter().any(|kr| kr.name == *r))
        {
            info!("Create role {}", roles_to_add);
            self.create_realm_role(roles_to_add).await?;
        }
        let keycloak_roles = self.get_all_realm_roles().await?;

        for user in users_keycloak {
            let configured_roles = user_configs[&user.username].roles.clone();
            let existing_roles = self.get_realm_roles(user).await?;
            let roles_to_add =
                Self::roles_to_add(&configured_roles, &keycloak_roles, &existing_roles);
            let roles_to_remove = Self::roles_to_remove(&configured_roles, &existing_roles);

            self.update_user_roles(&user.id, &roles_to_add, &roles_to_remove)
                .await?;
        }
        Ok(())
    }

    async fn update_user_roles(
        &self,
        user_id: &String,
        roles_to_add: &Vec<KeycloakRole>,
        roles_to_remove: &Vec<KeycloakRole>,
    ) -> anyhow::Result<()> {
        debug!("Updating roles for user: {}", user_id);
        if !roles_to_add.is_empty() {
            match self
                .reqwest_client
                .post(format!(
                    "{}/admin/realms/{}/users/{}/role-mappings/realm",
                    self.base_url, self.realm, user_id
                ))
                .bearer_auth(self.token.secret())
                .json(&json!(roles_to_add))
                .send()
                .await?
                .status()
            {
                reqwest::StatusCode::NO_CONTENT => {
                    info!("Added roles: {:?} to {:?}", roles_to_add, user_id)
                }
                status => error!("Failed to add roles to user: {}", status),
            }
        }
        self.reqwest_client
            .delete(format!(
                "{}/admin/realms/{}/users/{}/role-mappings/realm",
                self.base_url, self.realm, user_id
            ))
            .bearer_auth(self.token.secret())
            .json(&json!(roles_to_remove))
            .send()
            .await?;
        Ok(())
    }

    async fn update_users(
        &self,
        users: &Vec<&KeycloakUser>,
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
        user: &KeycloakUser,
        user_config: &UserConfig,
    ) -> anyhow::Result<()> {
        self.reqwest_client
            .put(format!(
                "{}/admin/realms/{}/users/{}",
                self.base_url, self.realm, user.id
            ))
            .bearer_auth(self.token.secret())
            .json(&json!(
                {
                    "firstName": user_config.first_name,
                    "lastName": user_config.last_name,
                    "email": user_config.email,
                    "enabled": user_config.enabled,
                    "username": user.username
                }
            ))
            .send()
            .await?;
        Ok(())
    }
}

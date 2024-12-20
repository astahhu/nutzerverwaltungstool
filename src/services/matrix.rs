use std::collections::HashMap;

use log::info;
use matrix_sdk::{
    config::SyncSettings,
    room::RoomMember,
    ruma::{OwnedUserId, RoomId, UserId},
    Client,
};
use url::Url;

use crate::UserConfig;

use super::Service;

#[derive(serde::Deserialize, serde::Serialize, Debug, Clone)]
pub struct MatrixConfig {
    pub homeserver: String,
    pub fs_main: String,
    pub fs_koop: String,
    pub fs_dunst: String,
    pub fs_rat: String,
    pub fs_pflicht: String,
}

impl Service for MatrixConfig {
    async fn configure(&self, users: &HashMap<String, UserConfig>) -> anyhow::Result<()> {
        let client = Client::new(Url::parse(&self.homeserver)?).await?;

        client
            .matrix_auth()
            .login_sso(|sso_url| async move {
                info!("Please login to the homeserver: {}", sso_url);
                Ok(())
            })
            .await?;

        info!("Successfully logged in");

        let _ = client.sync_once(SyncSettings::default()).await?;

        let users_in_main =
            get_users_in_room(&client, &RoomId::parse(&self.fs_main).unwrap()).await;
        let users_in_kooptiert =
            get_users_in_room(&client, &RoomId::parse(&self.fs_koop).unwrap()).await;
        let users_in_dunstkreis =
            get_users_in_room(&client, &RoomId::parse(&self.fs_dunst).unwrap()).await;
        let users_in_rat = get_users_in_room(&client, &RoomId::parse(&self.fs_rat).unwrap()).await;
        let users_in_pflicht =
            get_users_in_room(&client, &RoomId::parse(&self.fs_pflicht).unwrap()).await;

        for user in users {
            let roles = user.1.roles.clone();
            if user.1.matrix_id.is_none() {
                continue;
            }
            let matrix_id = UserId::parse(user.1.matrix_id.as_ref().unwrap());
            if roles.contains(&"FS_Rat_Informatik".to_string()) {
                if !users_in_rat
                    .iter()
                    .any(|u| u.user_id() == matrix_id.clone().unwrap())
                {
                    invite_user_to_room(&client, matrix_id.clone()?, &self.fs_main).await;
                    invite_user_to_room(&client, matrix_id.clone()?, &self.fs_dunst).await;
                    invite_user_to_room(&client, matrix_id.clone()?, &self.fs_koop).await;
                    invite_user_to_room(&client, matrix_id.clone()?, &self.fs_rat).await;
                    invite_user_to_room(&client, matrix_id.clone()?, &self.fs_pflicht).await;
                }
            } else if roles.contains(&"FS_Kooptiert_Informatik".to_string())
                && !users_in_kooptiert
                    .iter()
                    .any(|u| u.user_id() == matrix_id.clone().unwrap())
            {
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_main).await;
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_dunst).await;
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_koop).await;
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_pflicht).await;
            } else if roles.contains(&"FS_Dunstkreis_Informatik".to_string())
                && !users_in_dunstkreis
                    .iter()
                    .any(|u| u.user_id() == matrix_id.clone().unwrap())
            {
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_main).await;
                invite_user_to_room(&client, matrix_id.clone()?, &self.fs_dunst).await;
            }
        }

        //get all users that are not in the config
        // let users_to_delete = users.iter().filter_map(|u| {
        //     if u.1.matrix_id.is_none() {
        //         return None;
        //     }
        //     let matrix_id = UserId::parse(u.1.matrix_id.as_ref().unwrap()).unwrap();
        //     if !users_in_main
        //         .iter()
        //         .any(|u| u.user_id() == matrix_id.clone())
        //     {
        //         return Some(matrix_id);
        //     }
        //     None
        // });

        // for user in users_to_delete {
        //     if user.== client.user_id().unwrap() {
        //         continue;
        //     }
        //     kick_user_from_room(&client, user.user_id().into(), &self.fs_main).await;
        //     if users_in_kooptiert //check if user is in kooptiert
        //         .iter()
        //         .any(|u| u.user_id() == user.user_id().clone())
        //     {
        //         kick_user_from_room(&client, user.user_id().into(), &self.fs_koop).await;
        //     }
        //     if users_in_dunstkreis //check if user is in dunstkreis
        //         .iter()
        //         .any(|u| u.user_id() == user.user_id().clone())
        //     {
        //         kick_user_from_room(&client, user.user_id().into(), &self.fs_dunst).await;
        //     }
        //     if users_in_rat //check if user is in rat
        //         .iter()
        //         .any(|u| u.user_id() == user.user_id().clone())
        //     {
        //         kick_user_from_room(&client, user.user_id().into(), &self.fs_rat).await;
        //     }
        //     // if users_in_pflicht //check if user is in pflicht
        //     //     .iter()
        //     //     .any(|u| u.user_id() == user.user_id().clone())
        //     // {
        //     //     kick_user_from_room(&client, user.user_id().into(), &self.fs_pflicht).await;
        //     // }
        // }

        client.sync(SyncSettings::default()).await?;

        client.matrix_auth().logout().await;

        info!("Matrix configuration complete");

        Ok(())
    }
}

async fn get_users_in_room(client: &Client, room_id: &RoomId) -> Vec<RoomMember> {
    let room = client.get_room(room_id).unwrap();
    room.joined_members().await.unwrap()
}

async fn invite_user_to_room(client: &Client, user_id: OwnedUserId, room_id: &str) {
    //check already is invited
    println!("Invite user {} to room {}", user_id, room_id);
    let res = client
        .joined_rooms()
        .iter()
        .find(|r| r.room_id() == room_id)
        .unwrap()
        .invite_user_by_id(&user_id)
        .await;
    println!("Invite result: {:?}", res);
}

async fn kick_user_from_room(client: &Client, user_id: OwnedUserId, room_id: &str) {
    println!("Kick user {} from room {}", user_id, room_id);
    let res = client
        .joined_rooms()
        .iter()
        .find(|r| r.room_id() == room_id)
        .unwrap()
        .kick_user(&user_id, None)
        .await;
    println!("Kick result: {:?}", res);
}

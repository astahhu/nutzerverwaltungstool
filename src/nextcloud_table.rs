use std::collections::HashMap;

use reqwest::Client;

use crate::UserConfig;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct OcsResponse {
    ocs: SchemeResponse,
}

#[derive(Serialize, Deserialize)]
struct SchemeResponse {
    data: TableScheme,
}

#[derive(Serialize, Deserialize, Debug)]
struct TableScheme {
    title: String,
    columns: Vec<ColumnScheme>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ColumnScheme {
    Text {
        id: u64,
        title: String,
    },
    Selection {
        id: u64,
        title: String,
        subtype: SelectionType,
        selectionOptions: Vec<SelectionOptions>,
    },
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum SelectionType {
    #[serde(rename = "")]
    Single,
    Multi,
    Check,
}

impl Default for SelectionType {
    fn default() -> Self {
        SelectionType::Single
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct SelectionOptions {
    id: u64,
    label: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct Column {
    data: Vec<ColumnData>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum ColumnData {
    List {
        #[serde(rename = "columnId")]
        column_id: u64,
        value: Vec<u64>,
    },
    Number {
        #[serde(rename = "columnId")]
        column_id: u64,
        value: i64,
    },
    Text {
        #[serde(rename = "columnId")]
        column_id: u64,
        value: String,
    },
}

impl ColumnData {
    fn column_id(&self) -> u64 {
        match self {
            ColumnData::List { column_id, .. }
            | ColumnData::Number { column_id, .. }
            | ColumnData::Text { column_id, .. } => *column_id,
        }
    }
}

fn parse_nextcloud_table(
    columns: Vec<Column>,
    scheme: SchemeResponse,
) -> Vec<HashMap<String, NextcloudTableCell>> {
    columns
        .into_iter()
        .map(|r| {
            r.data
                .into_iter()
                .filter_map(|c| {
                    let column = scheme.data.columns.iter().find(|cs| match cs {
                        ColumnScheme::Text { id, .. } => *id == c.column_id(),
                        ColumnScheme::Selection { id, .. } => *id == c.column_id(),
                    })?;

                    match (column, c) {
                        (ColumnScheme::Text { title, .. }, ColumnData::Text { value, .. }) => {
                            Some((title.clone(), NextcloudTableCell::String(value)))
                        }
                        (
                            ColumnScheme::Selection {
                                title,
                                subtype: SelectionType::Check,
                                ..
                            },
                            ColumnData::Text { value, .. },
                        ) if value == "true" || value == "false" => Some((
                            title.clone(),
                            NextcloudTableCell::Bool(if value == "true" { true } else { false }),
                        )),
                        (
                            ColumnScheme::Selection {
                                title,
                                subtype: SelectionType::Single,
                                selectionOptions,
                                ..
                            },
                            ColumnData::Number { value, .. },
                        ) => selectionOptions
                            .iter()
                            .find(|o| o.id == value as u64)
                            .map(|s| (title.clone(), NextcloudTableCell::String(s.label.clone()))),
                        (
                            ColumnScheme::Selection {
                                title,
                                subtype: SelectionType::Multi,
                                selectionOptions,
                                ..
                            },
                            ColumnData::List { value, .. },
                        ) => Some((
                            title.clone(),
                            NextcloudTableCell::List(
                                value
                                    .iter()
                                    .filter_map(|v| {
                                        selectionOptions
                                            .iter()
                                            .find(|o| o.id == (*v) as u64)
                                            .map(|s| s.label.clone())
                                    })
                                    .collect::<Vec<_>>(),
                            ),
                        )),
                        _ => None,
                    }
                })
                .collect()
        })
        .collect()
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Nextcloud {
    username: String,
    password: String,
    url: String,
}

async fn get_nextcloud_table(
    nextcloud: &Nextcloud,
    table_id: u64,
) -> anyhow::Result<Vec<HashMap<String, NextcloudTableCell>>> {
    let client = Client::new();

    let scheme = client
        .get(&format!(
            "{}/ocs/v2.php/apps/tables/api/2/tables/scheme/{}",
            nextcloud.url, table_id
        ))
        .header("Accept", "application/json")
        .header("OCS-APIRequest", "true")
        .basic_auth(nextcloud.username.clone(), Some(nextcloud.password.clone()))
        .send()
        .await?
        .json::<OcsResponse>()
        .await?
        .ocs;

    let columns: Vec<Column> = client
        .get(&format!(
            "{}/index.php/apps/tables/api/1/tables/{}/rows",
            nextcloud.url, table_id
        ))
        .header("Accept", "application/json")
        .header("OCS-APIRequest", "true")
        .basic_auth(nextcloud.username.clone(), Some(nextcloud.password.clone()))
        .send()
        .await?
        .json()
        .await?;

    Ok(parse_nextcloud_table(columns, scheme))
}

#[derive(Debug)]
pub enum NextcloudTableCell {
    Bool(bool),
    String(String),
    List(Vec<String>),
}

pub async fn get_user_configs(
    nextcloud: &Nextcloud,
    table_id: u64,
) -> anyhow::Result<HashMap<String, UserConfig>> {
    let a = get_nextcloud_table(&nextcloud, table_id).await?;

    Ok(a.into_iter()
        .filter_map(|mut b| {
            Some((
                if let Some(NextcloudTableCell::String(s)) = b.get("Funktionskennung") {
                    s.clone()
                } else {
                    return None;
                },
                UserConfig {
                    first_name: if let Some(NextcloudTableCell::String(s)) = b.remove("Vorname") {
                        Some(s.clone())
                    } else {
                        return None;
                    },
                    last_name: if let Some(NextcloudTableCell::String(s)) = b.remove("Nachname") {
                        Some(s.clone())
                    } else {
                        return None;
                    },
                    email: if let Some(NextcloudTableCell::String(s)) = b.remove("Funktionskennung")
                    {
                        Some(format!("{}@hhu.de", s))
                    } else {
                        return None;
                    },
                    matrix_id: None,
                    roles: if let Some(NextcloudTableCell::List(mut l)) = b.remove("Funktion") {
                        l.append(
                            &mut l
                                .iter()
                                .filter_map(|role| match b.get("Fachschaft")? {
                                    NextcloudTableCell::String(s) => Some(format!("{s} - {role}")),
                                    _ => None,
                                })
                                .collect(),
                        );
                        match b.get("Fachschaft")? {
                            NextcloudTableCell::String(s) => l.push(s.clone()),
                            _ => {
                                return None;
                            }
                        };
                        l
                    } else {
                        return None;
                    },
                    enabled: true,
                },
            ))
        })
        .fold(HashMap::new(), |mut map, (user_id, mut user_config)| {
            map.entry(user_id)
                .and_modify(|c| c.roles.append(&mut user_config.roles))
                .or_insert(user_config);
            map
        }))
}

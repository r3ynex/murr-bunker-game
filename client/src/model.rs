use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub code: String,
    pub player_id: String,
    pub token: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
    pub code: String,
    pub you: String,
    pub host_id: String,
    pub stage: String,
    pub round: u32,
    pub seats: u32,
    pub catastrophe: String,
    pub bunker: String,
    pub pack_name: String,
    #[serde(default)]
    pub category_labels: HashMap<String, String>,
    pub vote_open: bool,
    pub ballots_count: usize,
    pub vote_results: Vec<VoteResult>,
    pub log: Vec<String>,
    pub connected: usize,
    pub players: Vec<Player>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Player {
    pub id: String,
    pub name: String,
    pub active: bool,
    #[serde(default)]
    pub ready: bool,
    #[serde(default)]
    pub avatar_id: usize,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Card {
    pub kind: String,
    pub value: String,
    pub revealed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct VoteResult {
    pub name: String,
    pub votes: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LocalConfig {
    pub server_url: String,
    pub nickname: String,
    #[serde(default)]
    pub avatar_id: usize,
    #[serde(default)]
    pub profile_complete: bool,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:8787".to_owned(),
            nickname: String::new(),
            avatar_id: 0,
            profile_complete: false,
        }
    }
}

pub fn category_name(key: &str) -> &str {
    match key {
        "gender" => "Пол и возраст",
        "body" => "Телосложение",
        "personality" => "Характер",
        "profession" => "Профессия",
        "health" => "Здоровье",
        "hobby" => "Хобби",
        "fear" => "Фобия",
        "equipment" => "Крупный инвентарь",
        "backpack" => "Рюкзак",
        "fact" => "Доп. сведение",
        "ability" => "Спец. возможность",
        _ => key,
    }
}

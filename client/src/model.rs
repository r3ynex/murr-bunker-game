use serde::{Deserialize, Serialize};

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
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            server_url: "http://127.0.0.1:8787".to_owned(),
            nickname: "Игрок".to_owned(),
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

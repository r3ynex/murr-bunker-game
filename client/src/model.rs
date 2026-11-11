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
    pub bundle: Option<BundleInfo>,
    #[serde(default)]
    pub event_chance: u32,
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
    #[serde(default = "default_avatar")]
    pub avatar: u8,
    pub cards: Vec<Card>,
}

impl Player {
    pub fn revealed_card(&self, kind: &str) -> Option<&Card> {
        self.cards.iter().find(|card| card.kind == kind && card.revealed)
    }
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
    #[serde(default = "default_avatar")]
    pub avatar: u8,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleInfo {
    pub sha256: String,
    pub bytes: usize,
    #[serde(default)]
    pub background: Option<String>,
}

fn default_avatar() -> u8 { 1 }

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            server_url: "https://murr-bunker-api.r3ynex-error.workers.dev".to_owned(),
            nickname: String::new(),
            avatar: 1,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_tables_hide_own_unrevealed_cards() {
        let player: Player = serde_json::from_value(serde_json::json!({
            "id": "me", "name": "Player", "active": true,
            "cards": [
                {"kind": "profession", "value": "Architect", "revealed": false},
                {"kind": "ability", "value": "Repair", "revealed": true},
                {"kind": "ability2", "value": "Trade", "revealed": false}
            ]
        })).unwrap();
        assert!(player.revealed_card("profession").is_none());
        assert!(player.revealed_card("ability2").is_none());
        assert_eq!(player.revealed_card("ability").unwrap().value, "Repair");
    }
}

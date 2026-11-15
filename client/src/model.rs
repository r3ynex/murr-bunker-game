use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub type AvatarId = u32;

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
    avatar: AvatarId,
    #[serde(default, rename = "avatarId")]
    avatar_id: Option<AvatarId>,
    pub cards: Vec<Card>,
}

impl Player {
    pub fn avatar(&self) -> AvatarId {
        self.avatar_id.unwrap_or(self.avatar)
    }

    pub fn revealed_card(&self, kind: &str) -> Option<&Card> {
        self.cards
            .iter()
            .find(|card| card.kind == kind && card.revealed)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Card {
    pub kind: String,
    #[serde(deserialize_with = "deserialize_card_value")]
    pub value: String,
    pub revealed: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub items: Option<Vec<InventoryItem>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct InventoryItem {
    pub id: String,
    pub value: String,
}

impl Card {
    pub fn dossier_text(&self) -> (&str, Option<&str>) {
        if let Some(description) = &self.description {
            if description.is_empty() {
                return (&self.value, None);
            }
            let summary = self
                .value
                .strip_suffix(description.as_str())
                .and_then(|prefix| prefix.strip_suffix('\n'))
                .unwrap_or(&self.value);
            return (summary, Some(description));
        }

        // Older room snapshots append the description after the title and optional age/height.
        let Some((title, rest)) = self.value.split_once('\n') else {
            return (&self.value, None);
        };
        let summary_end = if matches!(self.kind.as_str(), "gender" | "body")
            && is_card_measurement(rest.lines().next().unwrap_or_default())
        {
            title.len() + 1 + rest.lines().next().unwrap_or_default().len()
        } else {
            title.len()
        };
        let description = self
            .value
            .get(summary_end + 1..)
            .filter(|s| !s.trim().is_empty());
        (&self.value[..summary_end], description)
    }
}

fn is_card_measurement(line: &str) -> bool {
    line.strip_suffix(" лет")
        .or_else(|| line.strip_suffix(" см"))
        .is_some_and(|number| number.parse::<u32>().is_ok())
}

fn deserialize_card_value<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = String::deserialize(deserializer)?;
    // Older workers send a generated stage as a separate labeled line.
    let mut lines = value.lines();
    let Some(title) = lines.next() else {
        return Ok(value);
    };
    let mut rest: Vec<_> = lines.collect();
    let index = usize::from(rest.first().is_some_and(|line| is_card_measurement(line)));
    let prefixes = ["Уровень/примечание: ", "Стаж: ", "Степень: "];
    let Some(stage) = rest.get(index).copied().and_then(|line| {
        prefixes
            .iter()
            .find_map(|&prefix| line.strip_prefix(prefix))
            .filter(|stage| !stage.is_empty())
    }) else {
        return Ok(value);
    };
    rest.remove(index);
    let mut compact = format!("{title} ({stage})");
    for line in rest {
        compact.push('\n');
        compact.push_str(line);
    }
    Ok(compact)
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
    pub avatar: AvatarId,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BundleInfo {
    pub sha256: String,
    pub bytes: usize,
    #[serde(default)]
    pub background: Option<String>,
}

fn default_avatar() -> AvatarId {
    1
}

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
    fn legacy_stage_is_shown_in_parentheses() {
        let card: Card = serde_json::from_value(serde_json::json!({
            "kind": "hobby", "value": "Садоводство\nУровень/примечание: Любитель\nЗнает сорта", "revealed": true
        })).unwrap();
        assert_eq!(card.value, "Садоводство (Любитель)\nЗнает сорта");
    }

    #[test]
    fn dossier_separates_description_and_keeps_age() {
        let legacy: Card = serde_json::from_value(serde_json::json!({
            "kind": "gender", "value": "Мужчина\n37 лет\nОписание", "revealed": false
        }))
        .unwrap();
        assert_eq!(legacy.dossier_text(), ("Мужчина\n37 лет", Some("Описание")));

        let current: Card = serde_json::from_value(serde_json::json!({
            "kind": "profession", "value": "Генерал Воитель (Профессионал)\nМеч и доспехи",
            "description": "Меч и доспехи", "revealed": true
        }))
        .unwrap();
        assert_eq!(
            current.dossier_text(),
            ("Генерал Воитель (Профессионал)", Some("Меч и доспехи"))
        );
    }

    #[test]
    fn public_tables_hide_own_unrevealed_cards() {
        let player: Player = serde_json::from_value(serde_json::json!({
            "id": "me", "name": "Player", "active": true,
            "cards": [
                {"kind": "profession", "value": "Architect", "revealed": false},
                {"kind": "ability", "value": "Repair", "revealed": true},
                {"kind": "ability2", "value": "Trade", "revealed": false}
            ]
        }))
        .unwrap();
        assert!(player.revealed_card("profession").is_none());
        assert!(player.revealed_card("ability2").is_none());
        assert_eq!(player.revealed_card("ability").unwrap().value, "Repair");
    }
}

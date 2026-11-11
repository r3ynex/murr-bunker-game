use crate::appearance::portable_path;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

pub const BASE: &str = include_str!("../packs/demo.json");
const MAX_JSON: u64 = 500_000;
const MAX_ZIP: u64 = 2_000_000;
const MAX_UNPACKED: u64 = 6_000_000;
const MAX_PACKS: usize = 24;

#[derive(Deserialize, Serialize)]
struct Selection {
    path: Option<PathBuf>,
    active: bool,
}

pub struct Entry {
    selection: Selection,
    data: Result<Pack, String>,
}

pub struct Pack {
    pub json: Value,
    assets: BTreeMap<String, Vec<u8>>,
}

impl Entry {
    pub fn name(&self) -> &str {
        self.data
            .as_ref()
            .ok()
            .and_then(|pack| pack.name())
            .or_else(|| self.selection.path.as_ref()?.file_name()?.to_str())
            .unwrap_or("Бункер 6")
    }

    pub fn error(&self) -> Option<&str> {
        self.data.as_ref().err().map(String::as_str)
    }
    pub fn active(&self) -> bool {
        self.selection.active
    }
    pub fn toggle(&mut self) {
        self.selection.active = !self.selection.active;
    }
    pub fn reload(&mut self) {
        self.data = Self::read(self.selection.path.as_deref());
    }

    fn read(path: Option<&Path>) -> Result<Pack, String> {
        match path {
            Some(path) => Pack::load(path),
            None => Ok(Pack {
                json: serde_json::from_str(BASE).expect("valid base pack"),
                assets: BTreeMap::new(),
            }),
        }
    }
}

pub fn library() -> Vec<Entry> {
    let saved = read_file(&portable_path("murr-packs.json"), 128_000)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<Selection>>(&bytes).ok())
        .unwrap_or_default();
    let base_active = saved
        .iter()
        .find(|entry| entry.path.is_none())
        .is_none_or(|entry| entry.active);
    std::iter::once(Selection {
        path: None,
        active: base_active,
    })
    .chain(saved.into_iter().filter(|entry| entry.path.is_some()))
    .take(MAX_PACKS)
    .map(|selection| Entry {
        data: Entry::read(selection.path.as_deref()),
        selection,
    })
    .collect()
}

pub fn save_library(entries: &[Entry]) -> Result<(), String> {
    let settings: Vec<_> = entries.iter().map(|entry| &entry.selection).collect();
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    fs::write(portable_path("murr-packs.json"), bytes).map_err(|e| e.to_string())
}

pub fn add(entries: &mut Vec<Entry>, path: &Path) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    let data = Pack::load(&path)?;
    if let Some(existing) = entries
        .iter_mut()
        .find(|entry| entry.selection.path.as_ref() == Some(&path))
    {
        existing.data = Ok(data);
        existing.selection.active = true;
    } else {
        if entries.len() == MAX_PACKS {
            return Err("В списке может быть до 24 паков".into());
        }
        entries.push(Entry {
            selection: Selection {
                path: Some(path),
                active: true,
            },
            data: Ok(data),
        });
    }
    Ok(())
}

fn limited(reader: impl Read, limit: u64) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err("Файл превышает допустимый размер".into());
    }
    Ok(bytes)
}

fn read_file(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    limited(File::open(path).map_err(|e| e.to_string())?, limit)
}

fn parse(bytes: &[u8]) -> Result<Value, String> {
    let data: Value = serde_json::from_slice(bytes.strip_prefix(&[239, 187, 191]).unwrap_or(bytes))
        .map_err(|e| e.to_string())?;
    if !data.is_object() {
        return Err("Пак должен быть объектом JSON".into());
    }
    Ok(data)
}

impl Pack {
    pub fn name(&self) -> Option<&str> {
        self.json
            .pointer("/meta/name")
            .or_else(|| self.json.get("name"))
            .and_then(Value::as_str)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let extension = path
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension == "json" {
            return Ok(Self {
                json: parse(&read_file(path, MAX_JSON)?)?,
                assets: BTreeMap::new(),
            });
        }
        if extension != "zip" {
            return Err("Выбери JSON или ZIP".into());
        }
        let bytes = read_file(path, MAX_ZIP)?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
        if archive.len() > 48 {
            return Err("В ZIP больше 48 файлов".into());
        }
        let mut manifest = None;
        let mut assets = BTreeMap::new();
        let mut remaining = MAX_UNPACKED;
        for index in 0..archive.len() {
            let file = archive.by_index(index).map_err(|e| e.to_string())?;
            if file.is_dir() {
                continue;
            }
            let name = file.name().to_owned();
            if name == "pack.json" {
                if manifest.is_some() {
                    return Err("В ZIP несколько файлов pack.json".into());
                }
                let data = limited(file, remaining.min(MAX_JSON))?;
                remaining -= data.len() as u64;
                manifest = Some(parse(&data)?);
            } else if asset_name(&name) {
                let data = limited(file, remaining)?;
                remaining -= data.len() as u64;
                if assets.insert(name, data).is_some() {
                    return Err("Повтор файла изображения в ZIP".into());
                }
            } else {
                return Err("В ZIP допускаются pack.json и изображения в assets/".into());
            }
        }
        Ok(Self {
            json: manifest.ok_or("В ZIP нет pack.json")?,
            assets,
        })
    }

    pub fn payload(&self) -> Result<(String, Option<Vec<u8>>), String> {
        let json = self.json.to_string();
        if json.len() as u64 > MAX_JSON {
            return Err("Объединённый JSON больше 500 КБ".into());
        }
        if self.assets.is_empty() {
            return Ok((json, None));
        }
        if self.assets.len() >= 48
            || json.len() as u64 + self.assets.values().map(|v| v.len() as u64).sum::<u64>()
                > MAX_UNPACKED
        {
            return Err(
                "Объединённый ZIP превышает лимит файлов или распакованного размера".into(),
            );
        }
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        writer
            .start_file("pack.json", options)
            .map_err(|e| e.to_string())?;
        writer
            .write_all(json.as_bytes())
            .map_err(|e| e.to_string())?;
        for (name, bytes) in &self.assets {
            writer
                .start_file(name, options)
                .map_err(|e| e.to_string())?;
            writer.write_all(bytes).map_err(|e| e.to_string())?;
        }
        let bytes = writer.finish().map_err(|e| e.to_string())?.into_inner();
        if bytes.len() as u64 > MAX_ZIP {
            return Err("Объединённый ZIP больше 2 МБ".into());
        }
        Ok((json, Some(bytes)))
    }
}

fn asset_name(name: &str) -> bool {
    name.strip_prefix("assets/").is_some_and(|relative| {
        !relative.starts_with('/')
            && !relative.contains("..")
            && !relative.contains("//")
            && relative
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_./-".contains(&b))
            && matches!(
                relative
                    .rsplit_once('.')
                    .map(|(_, ext)| ext.to_ascii_lowercase())
                    .as_deref(),
                Some("png" | "jpg" | "jpeg" | "webp")
            )
    })
}

fn append(target: &mut Vec<Value>, source: &Value, limit: usize) -> Result<(), String> {
    let entries = source.as_array().ok_or("Ожидается список карточек")?;
    if target.len() + entries.len() > limit {
        return Err(format!("В одном разделе допускается до {limit} записей"));
    }
    target.extend(entries.iter().cloned());
    Ok(())
}

fn append_categories(target: &mut Map<String, Value>, source: &Value) -> Result<(), String> {
    for (kind, value) in source.as_object().ok_or("Некорректный раздел категорий")?
    {
        let entries = value.get("cards").unwrap_or(value);
        let entry = target
            .entry(kind.clone())
            .or_insert_with(|| json!({"cards": [], "label": value.get("label")}));
        append(
            entry["cards"].as_array_mut().expect("category list"),
            entries,
            500,
        )?;
    }
    Ok(())
}

fn original_cards(section: &Value) -> Value {
    let cards: Vec<_> = section["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|item| {
            if item.is_string() {
                return item.clone();
            }
            let fields = [
                "title",
                "description",
                "stage",
                "age1",
                "age2",
                "height1",
                "height2",
                "timer",
            ];
            Value::Object(
                fields
                    .into_iter()
                    .filter_map(|key| {
                        item.get(key)
                            .filter(|v| !v.is_null())
                            .map(|v| (key.to_owned(), v.clone()))
                    })
                    .collect(),
            )
        })
        .collect();
    json!(cards)
}

pub fn selected_payload(entries: &[Entry]) -> Result<(String, Option<Vec<u8>>), String> {
    let packs = entries
        .iter()
        .filter(|entry| entry.active())
        .map(|entry| {
            entry
                .data
                .as_ref()
                .map_err(|error| format!("{}: {error}", entry.name()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    match packs.as_slice() {
        [] => Err("Активируй хотя бы один пак".into()),
        [pack] => pack.payload(),
        _ => merge(&packs)?.payload(),
    }
}

fn merge(packs: &[&Pack]) -> Result<Pack, String> {
    let mut categories = Map::new();
    let mut catastrophes = Vec::new();
    let mut bunker = Map::new();
    let mut assets = BTreeMap::new();
    let mut has_background = false;
    for pack in packs {
        let root = &pack.json;
        let mut original_world = Map::new();
        let original_catastrophes;
        let world = root.get("world").unwrap_or(root);
        if root["spec"].is_object() {
            for (source, target) in [
                ("sex", "gender"),
                ("body", "body"),
                ("trait", "personality"),
                ("profession", "profession"),
                ("health", "health"),
                ("hobbies", "hobby"),
                ("phobia", "fear"),
                ("inventory", "equipment"),
                ("backpack", "backpack"),
                ("addInfo", "fact"),
                ("spec", "ability"),
            ] {
                if root.get(source).is_some() {
                    let cards = original_cards(&root[source]);
                    if cards.as_array().is_some_and(|v| !v.is_empty()) {
                        let mut section = Map::new();
                        section.insert(target.into(), cards);
                        append_categories(&mut categories, &Value::Object(section))?;
                    }
                }
            }
            for (source, target) in [
                ("bunkerBedroom", "rooms"),
                ("bunkerCreated", "construction"),
                ("bunkerLocation", "location"),
                ("bunkerPresence", "contents"),
                ("bunkerFoodAmount", "supplies"),
                ("bunkerTimeSpent", "duration"),
                ("bunkerSize", "size"),
            ] {
                let cards = original_cards(&root[source]);
                if cards.as_array().is_some_and(|v| !v.is_empty()) {
                    original_world.insert(target.into(), cards);
                }
            }
            original_catastrophes = original_cards(&root["catastrophe"]);
            append(&mut catastrophes, &original_catastrophes, 200)?;
        } else {
            if root
                .get("formatVersion")
                .is_some_and(|version| version.as_u64() != Some(2))
            {
                return Err("Неизвестная версия пака".into());
            }
            for group in [
                "categories",
                "roles",
                "characteristics",
                "items",
                "customCategories",
            ] {
                if let Some(section) = root.get(group) {
                    append_categories(&mut categories, section)?;
                }
            }
            if let Some(section) = world.get("catastrophes") {
                append(&mut catastrophes, section, 200)?;
            }
            if let Some(sections) = world.get("bunker") {
                original_world.extend(
                    sections
                        .as_object()
                        .ok_or("Некорректный раздел бункера")?
                        .clone(),
                );
            } else if let Some(section) = root.get("bunkers") {
                original_world.insert("description".into(), section.clone());
            }
        }
        for (kind, entries) in original_world {
            let section = bunker.entry(kind).or_insert_with(|| json!([]));
            append(section.as_array_mut().expect("bunker list"), &entries, 200)?;
        }
        for (name, data) in &pack.assets {
            let normalized = name.to_ascii_lowercase();
            let background = normalized
                .rsplit_once('.')
                .is_some_and(|(stem, _)| stem == "assets/catastrophe");
            if background && has_background {
                continue;
            }
            has_background |= background;
            assets.entry(normalized).or_insert_with(|| data.clone());
        }
    }
    if categories.len() > 36 {
        return Err("В объединённом паке больше 36 категорий".into());
    }
    let mut result = json!({"formatVersion":2, "name":format!("Сборник · {} паков", packs.len()), "categories":categories});
    if !catastrophes.is_empty() {
        result["catastrophes"] = json!(catastrophes);
    }
    if !bunker.is_empty() {
        result["bunker"] = json!(bunker);
    }
    Ok(Pack {
        json: result,
        assets,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combines_base_and_imported_cards_and_keeps_first_background() {
        let mut base = Entry::read(None).unwrap();
        base.assets.insert("assets/catastrophe.png".into(), vec![1]);
        let extra = Pack {
            json: json!({"spec":{"data":[{"title":"Test ability"}]},"profession":{"data":[{"title":"Test profession","stage":"Expert"}]}}),
            assets: BTreeMap::from([("assets/catastrophe.jpg".into(), vec![2])]),
        };
        let merged = merge(&[&base, &extra]).unwrap();
        let cards = merged.json["categories"]["profession"]["cards"]
            .as_array()
            .unwrap();
        assert!(cards
            .iter()
            .any(|card| card["title"] == "Test profession" && card["stage"] == "Expert"));
        assert!(cards.iter().any(|card| card["title"] == "Купец"));
        assert_eq!(merged.assets.len(), 1);
        assert_eq!(merged.assets["assets/catastrophe.png"], [1]);
        let (_, zip) = merged.payload().unwrap();
        assert!(zip.is_some());
    }
}

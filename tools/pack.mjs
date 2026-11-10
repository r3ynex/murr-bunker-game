#!/usr/bin/env node
// Murr Pack toolkit — no third-party dependencies.
// build: editable directory -> one JSON for the game
// split: JSON (v1/v2 or Bunker Online export) -> editable directory
// check: validate a directory or JSON
import fs from "node:fs";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { importBunkerOnline, normalizePack, CATEGORIES, BUNKER_LABELS } from "../server/src/content.js";

export const CATEGORY_FILES = {
  gender: "characters/identity.json",
  body: "characters/body.json",
  personality: "characters/personality.json",
  profession: "roles/professions.json",
  health: "characters/health.json",
  hobby: "characters/hobbies.json",
  fear: "characters/fears.json",
  equipment: "items/large-equipment.json",
  backpack: "items/backpacks.json",
  fact: "characters/facts.json",
  ability: "roles/abilities.json"
};
export const BUNKER_FILES = {
  description: "world/bunker/description.json",
  location: "world/bunker/location.json",
  construction: "world/bunker/construction.json",
  rooms: "world/bunker/rooms.json",
  contents: "world/bunker/contents.json",
  supplies: "world/bunker/supplies.json",
  duration: "world/bunker/duration.json",
  size: "world/bunker/size.json"
};
const CATASTROPHES_FILE = "world/catastrophes.json";
const readJson = filename => JSON.parse(fs.readFileSync(filename, "utf8").replace(/^\uFEFF/, ""));
function pathInside(root, relative) {
  if (typeof relative !== "string" || !relative.endsWith(".json") ||
      path.isAbsolute(relative) || relative.includes("\\") || relative.includes("\0")) {
    throw Error("Ожидается относительный путь к JSON: " + String(relative));
  }
  const resolved = path.resolve(root, relative);
  const start = path.resolve(root) + path.sep;
  if (!resolved.startsWith(start)) throw Error("Путь выходит за пределы пака: " + relative);
  // Follow symlinks only when their target remains within the pack directory.
  const realRoot = fs.realpathSync(root);
  const real = fs.realpathSync(resolved);
  if (!real.startsWith(realRoot + path.sep)) throw Error("Недопустимая ссылка за пределы пака");
  return real;
}
function contentAt(root, reference) {
  const entry = typeof reference === "string" ? reference : reference?.file;
  return readJson(pathInside(root, entry));
}
export function buildFromDirectory(directory) {
  const root = path.resolve(directory);
  const manifest = readJson(path.join(root, "pack.json"));
  if (manifest.formatVersion !== 2 || !manifest.categories || !manifest.catastrophes || !manifest.bunker) {
    throw Error("pack.json: укажи formatVersion=2, categories, catastrophes и bunker");
  }
  const categories = {};
  for (const [key, value] of Object.entries(manifest.categories)) {
    const cards = contentAt(root, value);
    categories[key] = typeof value === "string" ? cards :
      { label: value.label, cards };
  }
  const bunker = {};
  for (const [key, value] of Object.entries(manifest.bunker)) {
    if (!(key in BUNKER_LABELS)) throw Error("Неизвестный раздел бункера: " + key);
    bunker[key] = contentAt(root, value);
  }
  const bundle = {
    formatVersion: 2, id: manifest.id, name: manifest.name,
    description: manifest.description, categories,
    catastrophes: contentAt(root, manifest.catastrophes), bunker
  };
  normalizePack(bundle); // same validation rules as the live server
  return bundle;
}
function writeJson(filename, data) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, JSON.stringify(data, null, 2) + "\n", "utf8");
}
function safeId(value) {
  return String(value || "custom").toLowerCase().replace(/[^a-z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").slice(0, 60) || "custom";
}
export function splitPack(raw, directory) {
  const root = path.resolve(directory);
  if (fs.existsSync(root) && fs.readdirSync(root).length !== 0) {
    throw Error("Папка назначения не пуста — выбери новую папку: " + root);
  }
  const normalized = normalizePack(raw);
  const manifest = {
    formatVersion: 2, id: safeId(normalized.id), name: normalized.name,
    description: normalized.description, categories: {},
    catastrophes: CATASTROPHES_FILE, bunker: {}
  };
  for (const key of normalized.categoryOrder) {
    const file = CATEGORY_FILES[key] || ("characters/custom-" + safeId(key) + ".json");
    manifest.categories[key] = {
      label: normalized.categoryLabels[key], file
    };
    writeJson(path.join(root, file), normalized.categories[key]);
  }
  for (const [key, entries] of Object.entries(normalized.bunker)) {
    const file = BUNKER_FILES[key];
    manifest.bunker[key] = file;
    writeJson(path.join(root, file), entries);
  }
  writeJson(path.join(root, CATASTROPHES_FILE), normalized.catastrophes);
  writeJson(path.join(root, "pack.json"), manifest);
  return manifest;
}
export function readAnyPack(source) {
  if (fs.statSync(source).isDirectory()) return buildFromDirectory(source);
  return readJson(source);
}
export function validateSource(source) {
  const normalized = normalizePack(readAnyPack(source));
  return {
    name: normalized.name,
    categories: normalized.categoryOrder.length,
    cards: normalized.categoryOrder.reduce((sum, key) => sum + normalized.categories[key].length, 0),
    catastrophes: normalized.catastrophes.length,
    bunkerSections: Object.keys(normalized.bunker).length
  };
}
function usage() {
  return [
    "Murr Pack Toolkit",
    "  node tools/pack.mjs build packs/starter --out out/starter.json",
    "  node tools/pack.mjs check packs/starter",
    "  node tools/pack.mjs check out/starter.json",
    "  node tools/pack.mjs split original.json packs/my-own-pack",
    "  node tools/pack.mjs import original-bunker-online.json packs/imported",
    "  node tools/pack.mjs init packs/new-pack",
    "",
    "Редактируй JSON внутри папки, затем build. В игру загружай итоговый один JSON."
  ].join("\n");
}
function main(argv) {
  const [command, first, second, third] = argv;
  if (!command || ["--help", "help", "-h"].includes(command)) { console.log(usage()); return; }
  if (command === "build") {
    if (!first) throw Error(usage());
    const manifest = buildFromDirectory(first);
    const option = second === "--out" ? third : undefined;
    if (second && !option) throw Error("Используй --out путь/файл.json");
    const target = path.resolve(option || path.join("dist", safeId(manifest.id) + ".json"));
    writeJson(target, manifest);
    console.log("Собран пак: " + target, validateSource(target));
  } else if (command === "check") {
    if (!first) throw Error(usage());
    console.log("Пак корректен:", validateSource(first));
  } else if (command === "split" || command === "import") {
    if (!first || !second) throw Error(usage());
    const input = readJson(first);
    if (command === "import" && !input?.spec) throw Error("Ожидается JSON с объектом spec (экспорт Бункер Онлайн)");
    const converted = command === "import" ? importBunkerOnline(input) : input;
    splitPack(converted, second);
    console.log("Создан редактируемый пак: " + path.resolve(second));
  } else if (command === "init") {
    if (!first) throw Error(usage());
    // Start with a known-valid pack rather than dozens of empty lists.
    splitPack({
      name: "Мой новый пак", id: safeId(path.basename(first)), formatVersion: 2,
      description: "Отредактируй отдельные файлы и собери пак.",
      categories: { profession: { cards: ["Инженер", "Биолог", "Повар"] } },
      catastrophes: ["Мир изменился, и группе нужно безопасное убежище."],
      bunker: { description: ["Небольшой бункер с ограниченными запасами."] }
    }, first);
    console.log("Шаблон создан: " + path.resolve(first));
  } else {
    throw Error(usage());
  }
}
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error("Ошибка пака:", error.message); process.exitCode = 1; }
}

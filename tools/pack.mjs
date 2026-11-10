#!/usr/bin/env node
// Tools for ONE-file, sectioned Murr Pack v2 JSON. No dependencies.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { normalizePack, organizePack } from "../server/src/content.js";

function readJson(filename) {
  if (!fs.existsSync(filename) || !fs.statSync(filename).isFile()) {
    throw Error("Укажи путь к JSON-файлу: " + filename);
  }
  return JSON.parse(fs.readFileSync(filename, "utf8").replace(/^\uFEFF/, ""));
}
function writeJson(filename, value) {
  fs.mkdirSync(path.dirname(filename), { recursive: true });
  fs.writeFileSync(filename, JSON.stringify(value, null, 2) + "\n", "utf8");
}
function summary(value) {
  const pack = normalizePack(value);
  return {
    name: pack.name,
    categories: pack.categoryOrder.length,
    cards: pack.categoryOrder.reduce((count, name) => count + pack.categories[name].length, 0),
    catastrophes: pack.catastrophes.length,
    bunkerSections: Object.keys(pack.bunker).length
  };
}
function usage() {
  return [
    "Murr Pack v2 — один JSON-файл со вложенными секциями",
    "  node tools/pack.mjs check packs/starter.json",
    "  node tools/pack.mjs import original-bunker-online.json --out packs/imported.json",
    "  node tools/pack.mjs convert old-pack.json --out packs/my-pack.json",
    "  node tools/pack.mjs init packs/my-pack.json",
    "",
    "В файле секции meta, roles, characteristics, items, world, customCategories.",
    "Для загрузки в игру не нужна сборка — отправь сам JSON."
  ].join("\n");
}
export function checkPack(source) {
  return summary(readJson(source));
}
export function convertPack(input, output) {
  const converted = organizePack(readJson(input));
  writeJson(output, converted);
  return summary(converted);
}
export function createTemplate(output) {
  if (fs.existsSync(output)) throw Error("Файл уже существует, не перезаписываю: " + output);
  const original = fileURLToPath(new URL("../packs/starter.json", import.meta.url));
  const value = readJson(original);
  value.meta.id = path.basename(output, ".json").toLowerCase().replace(/[^a-z0-9_-]/g, "-") || "my-pack";
  value.meta.name = "Мой пак";
  writeJson(output, value);
  return summary(value);
}
function main(args) {
  const [cmd, first, opt, out] = args;
  if (!cmd || ["help", "--help", "-h"].includes(cmd)) return console.log(usage());
  if (!first) throw Error(usage());
  if (cmd === "check") {
    console.log("Проверка успешна:", checkPack(first));
    return;
  }
  if (cmd === "init") {
    console.log("Создан шаблон:", path.resolve(first), createTemplate(first));
    return;
  }
  if (!["import", "convert"].includes(cmd) || opt !== "--out" || !out) {
    throw Error(usage());
  }
  const original = readJson(first);
  if (cmd === "import" && !original?.spec?.data) {
    throw Error("Ожидается экспорт Бункер Онлайн с полем spec.data");
  }
  if (path.resolve(first) === path.resolve(out)) throw Error("Укажи другой выходной путь");
  console.log("Конвертация успешна:", path.resolve(out), convertPack(first, out));
}
if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  try { main(process.argv.slice(2)); }
  catch (error) { console.error("Ошибка пака:", error.message); process.exitCode = 1; }
}

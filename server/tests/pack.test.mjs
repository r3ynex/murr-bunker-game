import { test } from "node:test";
import { strict as assert } from "node:assert";
import { mkdtempSync, rmSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  CATEGORIES, DEFAULT_PACK, normalizePack, importBunkerOnline,
  organizePack, drawCard, drawScenario
} from "../src/content.js";
import { checkPack, convertPack, createTemplate } from "../../tools/pack.mjs";

const starter = fileURLToPath(new URL("../../packs/starter.json", import.meta.url));
const example = JSON.parse(readFileSync(starter, "utf8"));

test("rooms without an uploaded pack can draw a full demo game", () => {
  assert.equal(CATEGORIES.length, 11);
  for (const kind of CATEGORIES) assert.ok(drawCard(DEFAULT_PACK, kind).value, kind);
  const scenario = drawScenario(DEFAULT_PACK);
  assert.ok(scenario.catastrophe && scenario.bunker);
});

test("legacy v1 simple packs work and fill missing sections", () => {
  const pack = normalizePack({ name: "My pack", categories: { profession: ["Картограф"] } });
  assert.deepEqual(pack.categories.profession, [{ title: "Картограф" }]);
  assert.deepEqual(pack.categories.health, normalizePack({}).categories.health);
  assert.equal(drawCard(pack, "profession").value, "Картограф");
  assert.ok(drawScenario(pack).bunker.includes("Описание"));
});

test("single JSON v2 has meaningful sections", () => {
  assert.equal(example.formatVersion, 2);
  assert.ok(example.meta.name);
  assert.ok(example.roles.profession);
  assert.ok(example.roles.ability);
  assert.ok(example.characteristics.gender);
  assert.ok(example.items.backpack);
  assert.ok(example.world.catastrophes);
  assert.ok(example.world.bunker);
  const pack = normalizePack(example);
  assert.equal(pack.name, "Murr: Первое убежище");
  assert.ok(!pack.categoryOrder.includes("talent"));
  assert.equal(pack.bunker.location.length, 2);
  assert.equal(pack.categories.profession[0].stage, "Опытный");
  assert.equal(pack.categories.equipment[1].weight, 3);
  assert.equal(pack.categories.body[0].height1, 160);
  assert.ok(checkPack(starter).cards > 25);
});

test("age, height, descriptions, custom categories and scenario render", () => {
  const pack = normalizePack({
    formatVersion: 2,
    meta: { name: "Test" },
    roles: { profession: [{ title: "Штурман", stage: "Эксперт", description: "Читает карты" }] },
    characteristics: {
      gender: [{ title: "Исследователь", age1: 31, age2: 31 }],
      body: [{ title: "Подвижный", height1: 170, height2: 170 }],
      hobby: [{ title: "Садоводство", stage: "Любитель" }]
    },
    customCategories: { relic: { label: "Реликвия", cards: [{ title: "Компас", tags: ["редкость"] }] } },
    world: {
      catastrophes: [{ title: "Сбой", description: "Связь нестабильна" }],
      bunker: { location: ["Под горой"], supplies: ["Воды на 30 дней"] }
    }
  });
  assert.equal(pack.categoryLabels.relic, "Реликвия");
  assert.ok(pack.categoryOrder.includes("relic"));
  assert.equal(drawCard(pack, "gender").value, "Исследователь\n31 лет");
  assert.equal(drawCard(pack, "body").value, "Подвижный\n170 см");
  assert.equal(drawCard(pack, "profession").value, "Штурман (Эксперт)\nЧитает карты");
  assert.equal(drawCard(pack, "hobby").value, "Садоводство (Любитель)");
  assert.equal(drawCard(pack, "profession").description, "Читает карты");
  assert.equal(pack.categories.relic[0].tags[0], "редкость");
  const scenario = drawScenario(pack);
  assert.ok(scenario.catastrophe.includes("Связь нестабильна"));
  assert.ok(scenario.bunker.includes("Расположение: Под горой"));
});

test("flat v2 and grouped v2 share the same internal representation", () => {
  const flat = {
    formatVersion: 2, name: "Мой пак", id: "my",
    categories: {
      profession: { label: "Профессии", cards: [{ title: "Геолог", stage: "Мастер" }] },
      talent: { label: "Талант", cards: ["Слышит шёпот"] }
    },
    catastrophes: ["Пыльная буря"],
    bunker: { location: ["На горе"] }
  };
  const organized = organizePack(flat);
  assert.equal(organized.roles.profession.cards[0].stage, "Мастер");
  assert.equal(organized.roles.profession.label, "Профессии");
  assert.equal(organized.customCategories.talent.label, "Талант");
  assert.deepEqual(normalizePack(flat), normalizePack(organized));
  assert.deepEqual(normalizePack(organizePack(example)), normalizePack(example));
});

test("Bunker Online imported from actual root-level category layout", () => {
  const original = {
    spec: { data: [{ id: 10, title: "Обмен картами" }] },
    profession: { data: [{ id: 11, title: "Механик", description: "Чинит оборудование",
      stage: "Мастер", createdAt: "2026-03-03", customPackId: 77 }] },
    sex: { data: [{ title: "Путешественник", age1: 31, age2: 38 }] },
    body: { data: [{ title: "Крепкий", height1: 170, height2: 200 }] },
    bunkerBedroom: { data: [{ title: "Мастерская" }] },
    bunkerFoodAmount: { data: [{ title: "Запасы на год" }] },
    catastrophe: { data: [{ title: "Солнечная буря", timer: 90 }] }
  };
  const pack = normalizePack(importBunkerOnline(original));
  assert.deepEqual(normalizePack(original).categories.profession, pack.categories.profession);
  assert.equal(pack.categories.profession[0].stage, "Мастер");
  assert.equal(pack.categories.profession[0].id, "11");
  assert.equal(pack.categories.profession[0].createdAt, undefined);
  assert.equal(pack.categories.gender[0].age1, 31);
  assert.equal(pack.categories.ability[0].title, "Обмен картами");
  assert.equal(pack.bunker.rooms[0].title, "Мастерская");
  assert.equal(pack.bunker.supplies[0].title, "Запасы на год");
  assert.equal(pack.catastrophes[0].timer, 90);
  assert.equal(organizePack(original).roles.profession[0].stage, "Мастер");
});

test("invalid JSON pack structures rejected", () => {
  assert.throws(() => normalizePack({ categories: { profession: [] } }));
  assert.throws(() => normalizePack({ categories: { unknown: ["X"] } }));
  assert.throws(() => normalizePack({ formatVersion: 8 }));
  assert.throws(() => normalizePack({ formatVersion: 2, roles: { profession: [] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, roles: { fake: ["X"] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, characteristics: { gender: [{
    title: "Персонаж", age1: 40, age2: 20
  }] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, roles: { profession: [{title: "Повар", weight: -2}] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, customCategories: { profession: ["X"] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, items: {}, categories: { profession: ["X"] } }));
  assert.throws(() => normalizePack({ formatVersion: 2, world: { bunker: { unknown: ["X"] } } }));
});

test("toolkit validates, imports and creates ONE file", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "murr-packs-"));
  try {
    const converted = path.join(dir, "converted.json");
    const template = path.join(dir, "template.json");
    const stats = convertPack(starter, converted);
    assert.ok(stats.cards > 25);
    assert.equal(JSON.parse(readFileSync(converted, "utf8")).roles.profession[0].title,
      example.roles.profession[0].title);
    assert.deepEqual(normalizePack(JSON.parse(readFileSync(converted, "utf8"))), normalizePack(example));
    assert.equal(createTemplate(template).name, "Мой пак");
    assert.ok(checkPack(template).categories >= 11);
    assert.throws(() => createTemplate(template), /существует/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

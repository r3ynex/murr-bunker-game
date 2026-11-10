import { test } from "node:test";
import { strict as assert } from "node:assert";
import { mkdtempSync, rmSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  CATEGORIES, DEFAULT_PACK, normalizePack, importBunkerOnline,
  drawCard, drawScenario, pickEntry
} from "../src/content.js";
import { buildFromDirectory, splitPack, validateSource } from "../../tools/pack.mjs";

const starter = fileURLToPath(new URL("../../packs/starter/", import.meta.url));

test("demo pack has all 11 basic categories", () => {
  assert.equal(CATEGORIES.length, 11);
  for (const kind of CATEGORIES) {
    assert.ok(DEFAULT_PACK.categories[kind].length >= 3, kind);
  }
});

test("simple v1 packs still work and fill missing sections", () => {
  const pack = normalizePack({ name: "My pack", categories: { profession: ["Картограф"] } });
  assert.deepEqual(pack.categories.profession, [{ title: "Картограф" }]);
  assert.deepEqual(pack.categories.health, DEFAULT_PACK.categories.health);
  assert.equal(drawCard(pack, "profession").value, "Картограф");
  assert.ok(drawScenario(pack).bunker.includes("Описание"));
});

test("v2 retains profession abilities, custom labels, age, height and weights", () => {
  const pack = normalizePack({
    formatVersion: 2, name: "Космос",
    categories: {
      gender: [{ title: "Исследователь", age1: 31, age2: 31 }],
      body: [{ title: "Подвижный", height1: 170, height2: 170 }],
      profession: [{ title: "Штурман", stage: "Эксперт", description: "Читает карты", weight: 2 }],
      relic: { label: "Реликвия", cards: [{ title: "Старинный компас", tags: ["редкость"] }] }
    },
    catastrophes: [{ title: "Сбой навигации", description: "Связь нестабильна" }],
    bunker: { location: ["Под горой"], supplies: ["Воды на 30 дней"] }
  });
  assert.equal(pack.categoryLabels.relic, "Реликвия");
  assert.ok(pack.categoryOrder.includes("relic"));
  assert.equal(drawCard(pack, "gender").value, "Исследователь\n31 лет");
  assert.equal(drawCard(pack, "body").value, "Подвижный\n170 см");
  assert.equal(drawCard(pack, "profession").value, "Штурман\nСтаж: Эксперт\nЧитает карты");
  assert.equal(pack.categories.profession[0].weight, 2);
  assert.equal(pack.categories.relic[0].tags[0], "редкость");
  const scenario = drawScenario(pack);
  assert.ok(scenario.catastrophe.includes("Связь нестабильна"));
  assert.ok(scenario.bunker.includes("Расположение: Под горой"));
});

test("bad ranges, unknown version, bad categories and unsafe objects rejected", () => {
  assert.throws(() => normalizePack({ categories: { profession: [] } }));
  assert.throws(() => normalizePack({ categories: { unknown: ["X"] } }));
  assert.throws(() => normalizePack({ formatVersion: 8 }));
  assert.throws(() => normalizePack({ formatVersion: 2, categories: { foo: { label: "Foo", cards: [] } } }));
  assert.throws(() => normalizePack({ formatVersion: 2, categories: {
    gender: [{ title: "Персонаж", age1: 40, age2: 20 }]
  } }));
  assert.throws(() => normalizePack({ formatVersion: 2, categories: {
    profession: [{ title: "Повар", weight: -2 }]
  } }));
  assert.throws(() => normalizePack({ formatVersion: 2, bunker: { unknown: ["X"] } }));
});

test("Bunker Online importer preserves relevant metadata, drops service timestamps", () => {
  const original = { spec: {
    spec: { data: [{ id: 10, title: "Обменяться картами" }] },
    profession: { data: [{ id: 11, title: "Механик", description: "Чинит оборудование", stage: "Мастер",
      createdAt: "2026-03-03", customPackId: 77 }] },
    sex: { data: [{ title: "Путешественник", age1: 31, age2: 38 }] },
    body: { data: [{ title: "Крепкий", height1: 170, height2: 200 }] },
    bunkerBedroom: { data: [{ title: "Мастерская" }] },
    bunkerFoodAmount: { data: [{ title: "Запасы на год" }] },
    catastrophe: { data: [{ title: "Солнечная буря", timer: 90 }] }
  }};
  const raw = importBunkerOnline(original);
  const pack = normalizePack(raw);
  const direct = normalizePack(original);
  assert.equal(pack.categories.profession[0].stage, "Мастер");
  assert.equal(pack.categories.profession[0].id, "11");
  assert.equal(pack.categories.profession[0].createdAt, undefined);
  assert.equal(pack.categories.gender[0].age1, 31);
  assert.equal(pack.categories.ability[0].title, "Обменяться картами");
  assert.equal(pack.bunker.rooms[0].title, "Мастерская");
  assert.equal(pack.bunker.supplies[0].title, "Запасы на год");
  assert.equal(pack.catastrophes[0].timer, 90);
  assert.deepEqual(direct.bunker, pack.bunker);
});

test("modular starter builds, validates, and round-trips through split", () => {
  const raw = buildFromDirectory(starter);
  const pack = normalizePack(raw);
  assert.equal(pack.name, "Murr: Первое убежище");
  assert.equal(pack.categoryLabels.talent, "Скрытый талант");
  assert.equal(pack.bunker.location.length, 2);
  assert.ok(validateSource(starter).cards > 30);

  const temporary = mkdtempSync(path.join(tmpdir(), "murr-pack-test-"));
  try {
    const dest = path.join(temporary, "my-pack");
    splitPack(raw, dest);
    const rebuilt = buildFromDirectory(dest);
    assert.deepEqual(normalizePack(rebuilt), pack);
    assert.throws(() => splitPack(raw, dest), /не пуста/);
    // No path traversal from a modifiable pack manifest.
    const manifestPath = path.join(dest, "pack.json");
    const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
    manifest.categories.profession = "../../outside.json";
    writeFileSync(manifestPath, JSON.stringify(manifest));
    assert.throws(() => buildFromDirectory(dest), /за пределы|Ожидается/);
  } finally {
    rmSync(temporary, { recursive: true, force: true });
  }
});

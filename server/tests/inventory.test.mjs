import { strict as assert } from "node:assert";
import { test } from "node:test";
import { addInventoryItem, inventoryItems, replaceInventory, transferInventory } from "../src/inventory.js";

function room(source, target) {
  return { players: [{ id: "from", cards: [source] }, { id: "to", cards: [target] }] };
}

const command = itemIds => ({ playerId: "from", targetId: "to", kind: "backpack", itemIds });

test("moves only the requested IDs and keeps long names, descriptions and card visibility", () => {
  const description = "Описание с запятой, и длинным примечанием\nВторая строка";
  const value = "Аптечка, большая " + "А".repeat(250) + "\n" + description;
  const source = { kind: "backpack", revealed: true };
  const target = { kind: "backpack", revealed: false, value: "Радио, фонарь" };
  const state = room(source, target);
  replaceInventory(state, source, value, description);
  addInventoryItem(state, source, "Верёвка");
  addInventoryItem(state, source, "Ключ");
  const [first, second, third] = source.items;

  transferInventory(state, command(`${first.id}, ${third.id}`));
  assert.deepEqual(source.items, [second]);
  assert.equal(source.value, second.value);
  assert.deepEqual(target.items.slice(1), [first, third]);
  assert.equal(target.items[0].value, "Радио, фонарь");
  assert.equal(target.items[1].value, value);
  assert.equal(target.items[1].description, description);
  assert.equal(target.revealed, false);
  assert.equal(source.revealed, true);
  assert.throws(() => transferInventory(state, command(first.id)), /изменился/);
});

test("validates the whole selection before moving anything and handles legacy cards", () => {
  const source = { kind: "backpack", value: "Молоток, стальной", revealed: true };
  const target = { kind: "backpack", value: "Вода", revealed: true };
  const state = room(source, target);
  const original = structuredClone(state);
  assert.throws(() => transferInventory(state, command("0, 0")), /уникальные/);
  assert.throws(() => transferInventory(state, command("0, 100")), /изменился/);
  assert.throws(() => transferInventory(state, { ...command("0"), targetId: "from" }), /другого игрока/);
  assert.deepEqual(state, original);

  source.revealed = false;
  assert.throws(() => transferInventory(state, command("0")), /Открой карточку/);
  source.revealed = true;
  assert.equal(inventoryItems(source)[0].value, "Молоток, стальной");
  transferInventory(state, command("0"));
  assert.deepEqual(inventoryItems(source), []);
  assert.equal(source.value, "Пусто");
  assert.equal(new Set(target.items.map(item => item.id)).size, 2);
  assert.equal(target.value, "Вода, Молоток, стальной");
});

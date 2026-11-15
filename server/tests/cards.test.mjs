import { strict as assert } from "node:assert";
import { test } from "node:test";
import { healthDegree, setHealthDegree, swapCards } from "../src/cards.js";
import { drawCard } from "../src/content.js";

test("swaps whole cards in any shared category and preserves visibility and item IDs", () => {
  for (const kind of ["gender", "body", "ability2", "equipment", "custom_trait"]) {
    const a = { kind, value: "Карта А\nОписание", description: "Описание", revealed: true };
    const b = { kind, value: "Карта Б", revealed: false };
    if (kind === "equipment") a.items = [{ id: "42", value: a.value }];
    const other = { kind: "profession", value: "Врач" };
    const state = { players: [{ id: "a", cards: [other, a] }, { id: "b", cards: [b] }] };
    swapCards(state, { playerId: "a", targetId: "b", kind });
    assert.deepEqual(state.players[0].cards, [other, b]);
    assert.deepEqual(state.players[1].cards, [a]);
  }
});

test("leaves both players unchanged when the swap has no matching card", () => {
  const state = { players: [{ id: "a", cards: [{ kind: "gender" }] }, { id: "b", cards: [{ kind: "body" }] }] };
  const before = structuredClone(state);
  assert.throws(() => swapCards(state, { playerId: "a", targetId: "b", kind: "gender" }), /категории/);
  assert.throws(() => swapCards(state, { playerId: "a", targetId: "a", kind: "gender" }), /другого игрока/);
  assert.deepEqual(state, before);
});

test("updates and clears the degree without modifying the condition or its description", () => {
  const title = "Аллергия (сезонная)";
  const description = "Нужны таблетки\nУчитывать весной";
  const card = drawCard({ categories: { health: [{ title, stage: "Лёгкая степень", description }] } }, "health");
  assert.equal(healthDegree(card), "Лёгкая степень");
  setHealthDegree(card, " Средняя степень ");
  assert.equal(card.value, title + " (Средняя степень)\n" + description);
  setHealthDegree(card, "");
  assert.equal(card.value, title + "\n" + description);
  assert.equal(card.description, description);
  assert.equal(card.revealed, false);
  assert.throws(() => setHealthDegree(card, "Степень\nОписание"), /одну строку/);
  assert.throws(() => setHealthDegree({ kind: "body" }, "Любая"), /только/);
});

test("recovers degrees from old pack cards and old labeled lines", () => {
  const card = { kind: "health", value: "Астма (Лёгкая степень)\nИнгалятор", description: "Ингалятор" };
  const entries = [{ title: "Астма", stage: "Лёгкая степень" }];
  assert.equal(healthDegree(card, entries), "Лёгкая степень");
  setHealthDegree(card, "Ремиссия", entries);
  assert.equal(card.value, "Астма (Ремиссия)\nИнгалятор");

  const legacy = { kind: "health", value: "Астма\nСтепень: Острая\nИнгалятор" };
  assert.equal(healthDegree(legacy), "Острая");
  setHealthDegree(legacy, "Лёгкая");
  assert.equal(legacy.value, "Астма (Лёгкая)\nИнгалятор");
});

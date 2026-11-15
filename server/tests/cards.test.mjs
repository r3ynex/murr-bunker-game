import { strict as assert } from "node:assert";
import { test } from "node:test";
import { swapCards } from "../src/cards.js";

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

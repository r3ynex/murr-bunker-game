import { strict as assert } from "node:assert";
import { test } from "node:test";
import { publicDiceRoll, rollDice } from "../src/dice.js";

test("the room broadcasts one result and an animation offset to all viewers", () => {
  for (const sides of [6, 20]) {
    const state = { stage: "playing" };
    const value = rollDice(state, { name: "Игрок" }, sides, 1000);
    assert.ok(value >= 1 && value <= sides);
    const first = publicDiceRoll(state, 1000);
    const later = publicDiceRoll(state, 2000);
    assert.equal(first.value, value);
    assert.equal(later.id, first.id);
    assert.equal(later.value, first.value);
    assert.equal(later.elapsedMs - first.elapsedMs, 1000);
    assert.throws(() => rollDice(state, { name: "Другой" }, 20, 2100), /окончания/);
    assert.equal(publicDiceRoll(state, 1000 + first.durationMs - first.elapsedMs), undefined);
  }
});

test("older dice sizes still work through the log without replacing an ongoing animation", () => {
  const state = { stage: "playing" };
  rollDice(state, { name: "Игрок" }, 6, 100);
  const started = state.diceRoll.id;
  assert.ok(rollDice(state, { name: "Игрок" }, 100, 100) <= 100);
  assert.equal(state.diceRoll.id, started);
  state.stage = "finished";
  assert.equal(publicDiceRoll(state, 101), undefined);
});

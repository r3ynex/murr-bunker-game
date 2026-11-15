import { strict as assert } from "node:assert";
import { test } from "node:test";
import { normalizePack } from "../src/content.js";
import { applyHostAction, buildUndo, hostSnapshot, startGame, undoHostAction } from "../src/host.js";

function room() {
  return {
    stage: "lobby", hostId: "a", seats: 2, log: [],
    players: ["a", "b", "c"].map(id => ({ id, active: true, cards: [] })),
    bundle: { background: "assets/catastrophe.png" },
    pack: normalizePack({
      formatVersion: 2,
      roles: { profession: [{ title: "Врач", stage: "Новичок" }] },
      characteristics: { health: [{ title: "Аллергия", stage: "Лёгкая" }] },
      world: {
        catastrophes: [{ title: "Буря", image: "storm.png" }, { title: "Зима" }],
        bunker: { size: ["100 м²"], supplies: ["На год"] },
      },
    }),
  };
}

test("host can run a timed round and edit the world without losing pack backgrounds", () => {
  const state = room();
  startGame(state);
  applyHostAction(state, { action: "timer", seconds: 30 }, 1000);
  assert.equal(state.speechEndsAt, 31000);
  state.voteOpen = true;
  state.ballots = { a: "b" };
  applyHostAction(state, { action: "skip_vote" });
  assert.equal(state.round, 2);
  assert.equal(state.voteOpen, false);
  assert.deepEqual(state.ballots, {});
  assert.ok(state.players.every(player => player.active));

  applyHostAction(state, { action: "catastrophe", index: 0 });
  assert.equal(state.bundle.background, "assets/storm.png");
  applyHostAction(state, { action: "catastrophe", index: 1 });
  assert.equal(state.bundle.background, "assets/catastrophe.png");
  applyHostAction(state, { action: "bunker", kind: "supplies", value: "На два года" });
  assert.match(state.bunker, /100 м²/);
  assert.match(state.bunker, /На два года/);
});

test("batch stages and rotations preserve hidden cards and can be undone", () => {
  const state = room();
  startGame(state);
  applyHostAction(state, { action: "card_stage", kind: "profession", playerId: null, value: "Эксперт" });
  assert.ok(state.players.every(player => player.cards.some(card => card.value === "Врач (Эксперт)")));
  const first = state.players[0].cards.find(card => card.kind === "health");
  first.revealed = true;
  const before = hostSnapshot(state);
  applyHostAction(state, { action: "rotate", kind: "health", direction: 1 });
  const undo = buildUndo(before, state);
  assert.equal(state.players[1].cards.find(card => card.kind === "health").revealed, true);
  assert.equal(state.players[0].cards.find(card => card.kind === "health").revealed, false);
  undoHostAction(state, undo);
  assert.deepEqual(hostSnapshot(state), before);
});

test("undo rejects newer edits without partly rolling back", () => {
  const state = room();
  startGame(state);
  const before = hostSnapshot(state);
  applyHostAction(state, { action: "card_stage", kind: "health", playerId: null, value: "Средняя" });
  const undo = buildUndo(before, state);
  state.players[1].cards.find(card => card.kind === "health").revealed = true;
  assert.throws(() => undoHostAction(state, undo), /обновились/);
  assert.ok(state.players[0].cards.some(card => card.value === "Аллергия (Средняя)"));
});

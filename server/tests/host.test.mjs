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

test("startGame deals distinct ability2 cards and take preserves revealed state", () => {
  const state = room();
  state.pack = normalizePack({
    formatVersion: 2,
    roles: {
      profession: [{ title: "Врач" }],
      ability: [{ title: "Обмен" }, { title: "Щит" }],
    },
    characteristics: {
      gender: [{ title: "Женщина (чайлдфри)" }],
    },
  });
  for (let run = 0; run < 20; run++) {
    startGame(state);
    for (const player of state.players) {
      const first = player.cards.find(card => card.kind === "ability");
      const second = player.cards.find(card => card.kind === "ability2");
      assert.ok(first && second);
      assert.notEqual(first.value, second.value);
    }
  }
  const sourceProf = state.players[0].cards.find(card => card.kind === "profession");
  sourceProf.revealed = true;
  applyHostAction(state, { action: "take", playerId: "a", targetId: "b", kind: "profession" });
  const vacated = state.players[0].cards.find(card => card.kind === "profession");
  assert.equal(vacated.value, "Нет характеристики");
  assert.equal(vacated.revealed, true);

  applyHostAction(state, { action: "effect", effect: "childfree", playerId: "a" });
  const gender = state.players[0].cards.find(card => card.kind === "gender");
  assert.equal(gender.value, "Женщина (чайлдфри)");
});

test("startGame avoids cross-player duplicates when enough cards exist", () => {
  const state = room();
  state.pack = normalizePack({
    formatVersion: 2,
    roles: {
      profession: [{ title: "Врач" }, { title: "Механик" }, { title: "Агроном" }],
      ability: [{ title: "A1" }, { title: "A2" }, { title: "A3" }, { title: "A4" }, { title: "A5" }, { title: "A6" }],
    },
  });
  for (let run = 0; run < 15; run++) {
    startGame(state);
    const profs = state.players.map(p => p.cards.find(c => c.kind === "profession").value);
    assert.equal(new Set(profs).size, 3, "each player gets a unique profession");
    const abilities = state.players.flatMap(p => p.cards.filter(c => c.kind === "ability" || c.kind === "ability2").map(c => c.value));
    assert.equal(new Set(abilities).size, 6, "all 6 abilities are unique across players");
  }
});

test("round events trigger via eventChance, manual trigger/clear, and support undo", () => {
  const state = room();
  state.eventChance = 100;
  startGame(state);
  assert.equal(state.currentEvent, null);
  applyHostAction(state, { action: "skip_vote" });
  assert.ok(state.currentEvent);
  assert.equal(state.currentEvent.round, 2);
  assert.equal(state.eventHistory.length, 1);

  const before = hostSnapshot(state);
  applyHostAction(state, { action: "trigger_event", title: "Тревога", description: "Проверка систем" });
  assert.equal(state.currentEvent.title, "Тревога");
  const undo = buildUndo(before, state);
  undoHostAction(state, undo);
  assert.notEqual(state.currentEvent.title, "Тревога");

  applyHostAction(state, { action: "clear_event" });
  assert.equal(state.currentEvent, null);
});

test("runoff_vote handles ties and exclude_vote_leader removes unambiguous winner", () => {
  const state = room();
  startGame(state);
  state.voteOpen = false;
  state.voteResults = [
    { playerId: "a", name: "A", votes: 2 },
    { playerId: "b", name: "B", votes: 2 },
    { playerId: "c", name: "C", votes: 1 },
  ];
  assert.throws(() => applyHostAction(state, { action: "exclude_vote_leader" }), /Ничья/);
  applyHostAction(state, { action: "runoff_vote" });
  assert.equal(state.voteOpen, true);
  assert.deepEqual(state.voteCandidates, ["a", "b"]);

  state.voteOpen = false;
  state.voteCandidates = null;
  state.voteResults = [
    { playerId: "b", name: "B", votes: 3 },
    { playerId: "a", name: "A", votes: 1 },
  ];
  applyHostAction(state, { action: "exclude_vote_leader" });
  assert.equal(state.players.find(p => p.id === "b").active, false);
  assert.equal(state.players.find(p => p.id === "a").active, true);
});

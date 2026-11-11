import test from "node:test";
import assert from "node:assert/strict";
import { MAX_PLAYERS, AVATAR_COUNT, avatarIdFrom, canStartGame } from "../src/lobby_rules.js";

const state = () => ({
  hostId: "host", stage: "lobby",
  players: [{ id: "host", ready: false }, { id: "guest", ready: true }]
});

test("host starts when every guest is ready", () => {
  assert.equal(canStartGame(state()), null);
});
test("host cannot start when any guest is not ready", () => {
  const s = state();
  s.players.push({ id: "third", ready: false });
  assert.match(canStartGame(s), /Все игроки/);
  s.players[2].ready = true;
  assert.equal(canStartGame(s), null);
});
test("old rooms without ready field cannot skip readiness", () => {
  const s = state();
  delete s.players[1].ready;
  assert.match(canStartGame(s), /Все игроки/);
});
test("lobby must have at least two players", () => {
  const s = state();
  s.players.pop();
  assert.match(canStartGame(s), /два игрока/);
});
test("avatar ids are bounded and old sessions may omit them", () => {
  assert.equal(avatarIdFrom(undefined), 0);
  assert.equal(avatarIdFrom(AVATAR_COUNT - 1), AVATAR_COUNT - 1);
  assert.throws(() => avatarIdFrom(AVATAR_COUNT), /аватар/);
  assert.throws(() => avatarIdFrom(-1), /аватар/);
  assert.equal(MAX_PLAYERS, 12);
});

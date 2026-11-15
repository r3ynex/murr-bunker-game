const SETTLE_MS = 1670;
const DURATION_MS = 3650;
const LEAD_MS = 160;
const animated = sides => sides === 6 || sides === 20;

export function rollDice(state, player, sides, now = Date.now()) {
  if (!Number.isInteger(sides) || sides < 2 || sides > 100) throw Error("От 2 до 100 граней");
  if (animated(sides) && publicDiceRoll(state, now)) throw Error("Дождись окончания броска");

  const random = new Uint32Array(1);
  const ceiling = 2 ** 32 - (2 ** 32 % sides);
  do { crypto.getRandomValues(random); } while (random[0] >= ceiling);
  const value = random[0] % sides + 1;
  if (animated(sides)) {
    state.diceRoll = { id: crypto.randomUUID(), player: player.name, sides, value, startsAt: now + LEAD_MS };
  }
  return value;
}

export function publicDiceRoll(state, now = Date.now()) {
  const roll = state.diceRoll;
  if (state.stage !== "playing" || !roll || now >= roll.startsAt + DURATION_MS) return undefined;
  const { startsAt, ...result } = roll;
  return { ...result, elapsedMs: now - startsAt, settleMs: SETTLE_MS, durationMs: DURATION_MS };
}

// Lobby rules shared by the Cloudflare Durable Object and Node tests.
// Host clicks "Start" instead of toggling ready; all other players must be ready.
export const MAX_PLAYERS = 12;
export const AVATAR_COUNT = 24;

export function avatarIdFrom(value) {
  if (value == null) return 0;
  const n = Number(value);
  if (!Number.isInteger(n) || n < 0 || n >= AVATAR_COUNT) {
    throw Error("Некорректный аватар");
  }
  return n;
}

export function canStartGame(state) {
  if (state.stage !== "lobby") return "Игра уже запущена";
  if (state.players.length < 2) return "Нужно минимум два игрока (тестовый режим)";
  if (state.players.length > MAX_PLAYERS) return "Слишком много игроков";
  if (state.players.some(p => p.id !== state.hostId && p.ready !== true)) {
    return "Все игроки должны нажать «Готов»";
  }
  return null;
}

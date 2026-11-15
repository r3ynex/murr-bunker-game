import { setCardStage, swapCards } from "./cards.js";
import { BUNKER_LABELS, drawBunkerSection, drawCard, drawCatastrophe, drawScenario, formatBunker, normalizePack } from "./content.js";
import { isInventory, replaceInventory } from "./inventory.js";

function applyCatastrophe(state, scenario) {
  state.catastrophe = scenario.catastrophe;
  if (state.bundle) {
    state.bundle.background = scenario.background === undefined
      ? state.bundle.defaultBackground ?? null : scenario.background;
  }
}

function resetVote(state) {
  state.voteOpen = false;
  state.ballots = {};
  state.voteResults = [];
}

function deal(state, kind, revealed = false) {
  const category = kind === "ability2" ? "ability" : kind;
  if (!Object.hasOwn(state.pack.categories, category)) throw Error("Категория не найдена");
  const card = { ...drawCard(state.pack, category), kind, revealed };
  if (isInventory(kind)) replaceInventory(state, card, card.value, card.description);
  return card;
}

export function startGame(state) {
  if (!Array.isArray(state.pack.categoryOrder)) state.pack = normalizePack(state.pack);
  if (state.stage === "lobby" && state.bundle && !Object.hasOwn(state.bundle, "defaultBackground")) {
    state.bundle.defaultBackground = state.bundle.background ?? null;
  }
  const scenario = drawScenario(state.pack);
  applyCatastrophe(state, scenario);
  state.bunker = scenario.bunker;
  state.bunkerSections = scenario.bunkerSections;
  state.stage = "playing";
  state.round = 1;
  state.speechEndsAt = 0;
  resetVote(state);
  for (const player of state.players) {
    player.active = true;
    player.cards = state.pack.categoryOrder.map(kind => deal(state, kind));
    if (state.pack.categories.ability?.length > 1) player.cards.push(deal(state, "ability2"));
  }
}

export function bunkerSections(state) {
  if (state.bunkerSections) return state.bunkerSections;
  const sections = {};
  let current = "description";
  for (const line of (state.bunker ?? "").split("\n")) {
    const section = Object.entries(BUNKER_LABELS).find(([, label]) => line.startsWith(label + ": "));
    if (section) {
      current = section[0];
      sections[current] = line.slice(section[1].length + 2);
    } else {
      sections[current] = sections[current] ? sections[current] + "\n" + line : line;
    }
  }
  return sections;
}

function findPlayer(state, id) {
  const player = state.players.find(candidate => candidate.id === id);
  if (!player) throw Error("Игрок не найден");
  return player;
}

function targets(state, id) {
  return id === null ? state.players.filter(player => player.active) : [findPlayer(state, id)];
}

function cardSlot(player, kind) {
  const index = player.cards.findIndex(card => card.kind === kind);
  if (index < 0) throw Error("У игрока нет этой категории");
  return index;
}

function text(value, limit) {
  if (typeof value !== "string" || value.trim().length > limit) throw Error("Некорректный текст");
  return value.trim();
}

function replace(card, value) {
  card.value = value;
  card.description = "";
  delete card.health;
  delete card.profession;
}

const CHILD_FREE_NOTE = "\nЧайлдфри";
function effect(card, action, value) {
  if (action === "cure_health") replace(card, "Идеально здоров");
  if (action === "cure_fear") replace(card, "Нет фобий");
  if (action === "set_body") {
    if (!value) throw Error("Укажи телосложение");
    const [headline, ...tail] = card.value.split("\n");
    if (headline !== value) card.value = [value, ...tail].join("\n");
  }
  if (action === "childfree") {
    if (!card.value.split("\n").includes("Чайлдфри")) {
      const suffix = card.description ? "\n" + card.description : "";
      const description = card.value.endsWith(suffix) ? suffix : "";
      const end = card.value.length - description.length;
      card.value = card.value.slice(0, end) + CHILD_FREE_NOTE + description;
    }
  }
  if (action === "remove_childfree") {
    card.value = card.value.replace(/\nЧайлдфри(?=\n|$)| \(чайлдфри\)/giu, "");
  }
  if (action === "flip_gender" || action === "set_gender") {
    const match = /^(Мужчина|Женщина|Мужской|Женский)(?=[\s,(:]|$)/iu.exec(card.value);
    if (!match && action === "flip_gender") throw Error("Выбери новый пол карточки вручную");
    const opposite = /^(мужчина|мужской)$/iu.test(match?.[0] ?? "") ? "Женщина" : "Мужчина";
    const replacement = action === "flip_gender" ? opposite : value;
    if (!["Мужчина", "Женщина"].includes(replacement)) throw Error("Выбери пол карточки");
    const prefixLength = match?.[0].length ?? card.value.split("\n", 1)[0].length;
    card.value = replacement + card.value.slice(prefixLength);
  }
}

const EFFECT_CATEGORIES = {
  cure_health: "health", cure_fear: "fear", set_body: "body", childfree: "gender",
  remove_childfree: "gender", flip_gender: "gender", set_gender: "gender",
};
const HEALTH_DEGREES = ["Лёгкая степень", "Средняя степень", "Тяжёлая степень", "Критическая степень"];
const PROFESSION_STAGES = ["Новичок", "Опытный", "Профессионал", "Мастер-гуру"];
const randomEntry = entries => entries[crypto.getRandomValues(new Uint32Array(1))[0] % entries.length];

export function applyHostAction(state, data, now = Date.now()) {
  const { action, playerId, targetId, kind } = data;
  if (state.stage !== "playing" && !(state.stage === "finished" && ["restart", "close", "transfer_host"].includes(action))) {
    throw Error("Команда доступна во время партии");
  }
  switch (action) {
    case "timer":
      if (![0, 15, 30, 60].includes(data.seconds)) throw Error("Таймер на 15, 30 или 60 секунд");
      state.speechEndsAt = data.seconds ? now + data.seconds * 1000 : 0;
      return data.seconds ? "Таймер обсуждения: " + data.seconds + " сек." : "Таймер остановлен";
    case "seats":
      if (!Number.isInteger(data.seats) || data.seats < 1 || data.seats > 15) throw Error("Мест в бункере должно быть от 1 до 15");
      state.seats = data.seats;
      return "Мест в бункере: " + state.seats;
    case "skip_vote":
      resetVote(state);
      state.round += 1;
      return "Круг завершён без изгнания · раунд " + state.round;
    case "catastrophe":
      if (data.index !== undefined && (!Number.isInteger(data.index) || data.index < 0)) throw Error("Неверный выбор катаклизма");
      applyCatastrophe(state, drawCatastrophe(state.pack, data.index));
      return "Ведущий изменил катаклизм";
    case "bunker": {
      if (!Object.hasOwn(BUNKER_LABELS, kind)) throw Error("Секция бункера не найдена");
      const sections = { ...bunkerSections(state) };
      const value = data.value === undefined ? drawBunkerSection(state.pack, kind) : text(data.value, 6000);
      if (value) sections[kind] = value;
      else delete sections[kind];
      state.bunkerSections = sections;
      state.bunker = formatBunker(sections);
      return "Ведущий изменил бункер";
    }
    case "redraw":
      for (const player of targets(state, playerId)) {
        const slot = cardSlot(player, kind);
        player.cards[slot] = deal(state, kind, player.cards[slot].revealed);
      }
      return "Ведущий обновил характеристики";
    case "card_stage":
      for (const player of targets(state, playerId)) {
        const card = player.cards[cardSlot(player, kind)];
        const options = kind === "health" ? HEALTH_DEGREES : PROFESSION_STAGES;
        setCardStage(card, data.random === true ? randomEntry(options) : data.value, state.pack.categories[kind]);
      }
      return "Ведущий изменил стаж или степень";
    case "swap_all": {
      const source = findPlayer(state, playerId);
      const target = findPlayer(state, targetId);
      if (source === target) throw Error("Выбери двух разных игроков");
      [source.cards, target.cards] = [target.cards, source.cards];
      return "Ведущий обменял досье двух игроков";
    }
    case "swap":
      swapCards(state, data);
      return "Ведущий обменял характеристики";
    case "take": {
      const source = findPlayer(state, playerId);
      const target = findPlayer(state, targetId);
      if (source === target) throw Error("Выбери двух разных игроков");
      const sourceIndex = cardSlot(source, kind);
      const targetIndex = cardSlot(target, kind);
      target.cards[targetIndex] = source.cards[sourceIndex];
      source.cards[sourceIndex] = { kind, value: "Нет характеристики", revealed: false, description: "",
        ...(isInventory(kind) && { items: [] }) };
      return "Ведущий переместил карточку между игроками";
    }
    case "rotate": {
      if (![-1, 1].includes(data.direction)) throw Error("Выбери направление");
      const players = targets(state, null);
      if (players.length < 2) throw Error("Нужно минимум два активных игрока");
      const slots = players.map(player => cardSlot(player, kind));
      const cards = players.map((player, i) => player.cards[slots[i]]);
      for (const [index, player] of players.entries()) {
        player.cards[slots[index]] = cards[(index - data.direction + cards.length) % cards.length];
      }
      return "Ведущий передал характеристики по кругу";
    }
    case "void_card":
      for (const player of targets(state, playerId)) {
        const card = player.cards[cardSlot(player, kind)];
        replace(card, "Нет характеристики");
        if (isInventory(kind)) card.items = [];
      }
      return "Ведущий аннулировал характеристики";
    case "effect":
      if (!Object.hasOwn(EFFECT_CATEGORIES, data.effect)) throw Error("Действие не найдено");
      for (const player of targets(state, playerId)) {
        const card = player.cards[cardSlot(player, EFFECT_CATEGORIES[data.effect])];
        effect(card, data.effect, data.value === undefined ? "" : text(data.value, 2000));
      }
      return "Ведущий изменил состояние персонажей";
    case "add_fact": {
      const value = text(data.value, 2000);
      if (!value) throw Error("Введи дополнительное сведение");
      for (const player of targets(state, playerId)) {
        const card = player.cards[cardSlot(player, "fact")];
        replace(card, text(card.value + "\n" + value, 6000));
      }
      return "Ведущий добавил дополнительное сведение";
    }
    case "transfer_host":
      if (targetId === state.hostId) throw Error("Этот игрок уже ведущий");
      state.hostId = findPlayer(state, targetId).id;
      return "Ведущий изменился";
    case "restart":
      startGame(state);
      return "Ведущий начал партию заново";
    case "close":
      state.stage = "closed";
      state.speechEndsAt = 0;
      resetVote(state);
      return "Ведущий закрыл комнату";
    default:
      throw Error("Неизвестная команда ведущего");
  }
}

const UNDO_FIELDS = ["stage", "round", "seats", "catastrophe", "bunker", "bunkerSections",
  "bundle", "voteOpen", "ballots", "voteResults", "speechEndsAt"];
const equal = (a, b) => JSON.stringify(a) === JSON.stringify(b);

export function hostSnapshot(state) {
  return structuredClone({
    room: Object.fromEntries(UNDO_FIELDS.map(key => [key, state[key]])),
    players: state.players.map(({ id, active, cards }) => ({ id, active, cards })),
  });
}

export function buildUndo(before, state) {
  const after = hostSnapshot(state);
  const room = UNDO_FIELDS.filter(key => !equal(before.room[key], after.room[key]))
    .map(key => [key, before.room[key], after.room[key]]);
  const players = before.players.flatMap(player => {
    const next = after.players.find(candidate => candidate.id === player.id);
    if (!next) return [];
    const fields = ["cards", "active"].filter(key => !equal(player[key], next[key]))
      .map(key => [key, player[key], next[key]]);
    return fields.length ? [[player.id, fields]] : [];
  });
  return room.length || players.length ? { room, players } : null;
}

export function undoHostAction(state, undo) {
  if (!undo) throw Error("Нет действия для отмены");
  const changes = [[state, undo.room], ...undo.players.map(([id, changes]) => [findPlayer(state, id), changes])];
  for (const [target, fields] of changes) {
    if (fields.some(([key, , expected]) => !equal(target[key], expected))) {
      throw Error("Изменённые данные уже обновились. Отмена недоступна");
    }
  }
  for (const [target, fields] of changes) {
    for (const [key, previous] of fields) target[key] = previous;
  }
}

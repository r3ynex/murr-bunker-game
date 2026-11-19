import { cardHeading, setCardStage, swapCards } from "./cards.js";
import { BUNKER_LABELS, buildCardFromEntry, drawBunkerSection, drawCard, drawCatastrophe, drawEvent, drawScenario, formatBunker, normalizePack, pickEntry } from "./content.js";
import { isInventory, replaceInventory } from "./inventory.js";

function applyCatastrophe(state, scenario) {
  state.catastrophe = scenario.catastrophe;
  if (state.bundle) {
    state.bundle.background = scenario.background === undefined
      ? state.bundle.defaultBackground ?? null : scenario.background;
  }
}

export function resetVote(state) {
  state.voteOpen = false;
  state.voteCandidates = null;
  state.ballots = {};
  state.voteResults = [];
}

function usedHeadlines(state, category) {
  const used = new Set();
  for (const player of state.players) {
    for (const card of player.cards) {
      const cardCategory = card.kind === "ability2" ? "ability" : card.kind;
      if (cardCategory === category && card.value) {
        used.add(card.value.split("\n", 1)[0]);
      }
    }
  }
  return used;
}

function deal(state, kind, revealed = false) {
  const category = kind === "ability2" ? "ability" : kind;
  if (!Object.hasOwn(state.pack.categories, category)) throw Error("Категория не найдена");
  const card = { ...drawCard(state.pack, category, usedHeadlines(state, category)), kind, revealed };
  if (isInventory(kind)) replaceInventory(state, card, card.value, card.description);
  return card;
}

function drawFromDeck(decks, pack, category, avoidHeadline = null) {
  const all = pack.categories[category] ?? [];
  let deck = decks.get(category);
  if (!deck || deck.length === 0) {
    deck = [...all];
    decks.set(category, deck);
  }
  let candidates = avoidHeadline
    ? deck.filter(entry => cardHeading(entry.title, entry.stage) !== avoidHeadline)
    : deck;
  if (candidates.length === 0) {
    const allFiltered = avoidHeadline
      ? all.filter(entry => cardHeading(entry.title, entry.stage) !== avoidHeadline)
      : all;
    candidates = allFiltered.length > 0 ? allFiltered : deck;
  }
  const entry = pickEntry(candidates);
  const idx = deck.indexOf(entry);
  if (idx >= 0) deck.splice(idx, 1);
  return entry;
}

export function maybeTriggerRoundEvent(state, force = false) {
  const chance = Number(state.eventChance ?? 0);
  if (!force) {
    if (chance <= 0) return null;
    const roll = (crypto.getRandomValues(new Uint32Array(1))[0] / 4294967296) * 100;
    if (roll >= chance) return null;
  }
  const history = Array.isArray(state.eventHistory) ? state.eventHistory : [];
  const drawn = drawEvent(state.pack, history.map(e => e.title));
  const event = { round: state.round, title: drawn.title, description: drawn.description };
  state.currentEvent = event;
  state.eventHistory = [...history, event].slice(-20);
  return event;
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
  state.diceRoll = null;
  state.currentEvent = null;
  state.eventHistory = [];
  resetVote(state);
  const decks = new Map();
  for (const player of state.players) {
    player.active = true;
    player.cards = state.pack.categoryOrder.map(kind => {
      const entry = drawFromDeck(decks, state.pack, kind);
      const card = buildCardFromEntry(entry, kind);
      if (isInventory(kind)) replaceInventory(state, card, card.value, card.description);
      return card;
    });
    if (state.pack.categories.ability?.length > 1) {
      const firstAbility = player.cards.find(card => card.kind === "ability");
      const firstHeadline = firstAbility?.value.split("\n", 1)[0] ?? null;
      const entry = drawFromDeck(decks, state.pack, "ability", firstHeadline);
      player.cards.push({ ...buildCardFromEntry(entry, "ability"), kind: "ability2", revealed: false });
    }
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
    if (!/чайлдфри/iu.test(card.value)) {
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
    case "skip_vote": {
      resetVote(state);
      state.round += 1;
      const event = maybeTriggerRoundEvent(state);
      return event
        ? `Круг завершён без изгнания · раунд ${state.round} · Событие: ${event.title}`
        : "Круг завершён без изгнания · раунд " + state.round;
    }
    case "trigger_event": {
      if (data.title !== undefined) {
        const title = text(data.title, 240);
        if (!title) throw Error("Укажи название события");
        const description = data.description === undefined ? "" : text(data.description, 2000);
        const event = { round: state.round, title, description };
        state.currentEvent = event;
        state.eventHistory = [...(state.eventHistory ?? []), event].slice(-20);
        return "Событие раунда " + state.round + ": " + event.title;
      }
      const event = maybeTriggerRoundEvent(state, true);
      return "Событие раунда " + state.round + ": " + event.title;
    }
    case "clear_event":
      state.currentEvent = null;
      return "Текущее событие скрыто";
    case "runoff_vote": {
      if (state.voteOpen) throw Error("Сначала закрой текущее голосование");
      const results = Array.isArray(state.voteResults) ? state.voteResults : [];
      if (results.length < 2 || !results[0].votes) throw Error("Нет ничьей для переголосования");
      const topVotes = results[0].votes;
      const tied = results
        .filter(r => r.votes === topVotes && r.playerId)
        .map(r => r.playerId)
        .filter(id => state.players.some(p => p.id === id && p.active));
      if (tied.length < 2) throw Error("Для переголосования нужны минимум два активных лидера");
      state.voteOpen = true;
      state.voteCandidates = tied;
      state.ballots = {};
      state.voteResults = [];
      const names = tied.map(id => state.players.find(p => p.id === id)?.name || "?").join(", ");
      return "Открыто переголосование среди лидеров: " + names;
    }
    case "exclude_vote_leader": {
      if (state.voteOpen) throw Error("Сначала закрой голосование");
      const results = Array.isArray(state.voteResults) ? state.voteResults : [];
      if (results.length === 0 || !results[0].votes) throw Error("Нет итогов голосования");
      if (results.length >= 2 && results[0].votes === results[1].votes) {
        throw Error("Ничья по голосам — проведи переголосование или выбери вручную");
      }
      const leaderId = results[0].playerId;
      const leader = state.players.find(p => p.id === leaderId);
      if (!leader || !leader.active) throw Error("Лидер голосования уже выбыл");
      leader.active = false;
      state.voteCandidates = null;
      return `${leader.name} исключён(а) по итогам голосования (${results[0].votes} голос.)`;
    }
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
      const taken = source.cards[sourceIndex];
      target.cards[targetIndex] = taken;
      source.cards[sourceIndex] = { kind, value: "Нет характеристики", revealed: taken.revealed, description: "",
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
  "bundle", "voteOpen", "voteCandidates", "ballots", "voteResults", "speechEndsAt",
  "currentEvent", "eventHistory"];
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

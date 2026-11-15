export const isInventory = kind => kind === "equipment" || kind === "backpack";

const MAX_ITEMS = 30;
const MAX_ITEM_LENGTH = 2000;
const MAX_CELL_LENGTH = 6000;

function newItem(state, value, description = "") {
  state.nextItemId = (state.nextItemId ?? 0) + 1;
  return { id: String(state.nextItemId), value, description };
}

export function inventoryItems(card) {
  // Existing rooms have a single card value, which can contain commas and newlines.
  return card.items ?? (card.value ? [{ id: "0", value: card.value, description: card.description ?? "" }] : []);
}

function setItems(card, items) {
  card.items = items;
  card.value = items.length === 0 ? "Пусто" : items.length === 1 ? items[0].value
    : items.map(item => {
      const suffix = "\n" + item.description;
      return item.description && item.value.endsWith(suffix) ? item.value.slice(0, -suffix.length) : item.value;
    }).join(", ");
  card.description = items.length === 1 ? items[0].description : "";
}

export function replaceInventory(state, card, value, description) {
  setItems(card, [newItem(state, value, description)]);
}

function assignLegacyId(state, item) {
  return item.id === "0" ? newItem(state, item.value, item.description) : item;
}

function checkCapacity(items) {
  if (items.length > MAX_ITEMS || items.reduce((length, item) => length + item.value.length, 0) > MAX_CELL_LENGTH) {
    throw Error("В ячейке может быть до 30 предметов и 6000 символов");
  }
}

export function addInventoryItem(state, card, value) {
  if (typeof value !== "string" || !value.trim() || value.length > MAX_ITEM_LENGTH) {
    throw Error("Название предмета от 1 до 2000 символов");
  }
  const items = inventoryItems(card);
  const text = value.trim();
  checkCapacity([...items, { value: text }]);
  setItems(card, [...items.map(item => assignLegacyId(state, item)), newItem(state, text)]);
}

export function transferInventory(state, { playerId, targetId, kind, itemIds }) {
  if (!isInventory(kind)) throw Error("Перенос доступен только для предметов");
  if (playerId === targetId) throw Error("Выбери другого игрока");
  const source = state.players.find(player => player.id === playerId)?.cards.find(card => card.kind === kind);
  const target = state.players.find(player => player.id === targetId)?.cards.find(card => card.kind === kind);
  if (!source?.revealed || !target) throw Error("Открой карточку с предметами перед переносом");
  if (typeof itemIds !== "string" || itemIds.length > 600) throw Error("Укажи ID через запятую");
  const ids = itemIds.split(",").map(id => id.trim());
  const unique = new Set(ids);
  if (ids.length > MAX_ITEMS || unique.size !== ids.length || ids.some(id => !/^\d{1,16}$/.test(id))) {
    throw Error("Укажи уникальные ID через запятую");
  }
  const items = inventoryItems(source);
  const moved = items.filter(item => unique.has(item.id));
  if (moved.length !== ids.length) throw Error("Список предметов изменился. Проверь ID");
  const destination = inventoryItems(target);
  const received = [...destination, ...moved];
  checkCapacity(received);

  setItems(source, items.filter(item => !unique.has(item.id)));
  setItems(target, received.map(item => assignLegacyId(state, item)));
}

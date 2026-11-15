// Murr Pack v2: compact, data-only bundles produced from editable pack folders.
// No user content is evaluated as code. Legacy v1 and Bunker Online exports remain importable.

import { cardHeading } from "./cards.js";

export const CATEGORIES = [
  "gender", "body", "personality", "profession", "health", "hobby",
  "fear", "equipment", "backpack", "fact", "ability"
];
export const CATEGORY_LABELS = {
  gender: "Пол и возраст", body: "Телосложение", personality: "Характер",
  profession: "Профессия", health: "Здоровье", hobby: "Хобби",
  fear: "Фобия", equipment: "Крупный инвентарь", backpack: "Рюкзак",
  fact: "Дополнительное сведение", ability: "Специальная возможность"
};
export const BUNKER_LABELS = {
  description: "Описание", location: "Расположение",
  construction: "Постройка", rooms: "Комнаты", contents: "Внутри",
  supplies: "Еда и вода", duration: "Время проживания", size: "Размер"
};

const DEFAULT_INPUT = {
  name: "Murr: Demo",
  catastrophes: [
    "После мощной солнечной бури исчезла большая часть связи и электричества.",
    "Затяжные пылевые штормы сделали жизнь на поверхности очень сложной.",
    "Необычная зима и сбои инфраструктуры вынудили людей искать убежища."
  ],
  bunkers: [
    "Научное убежище, 110 м². Запас еды на 18 месяцев. Работает мастерская.",
    "Подземная библиотека, 90 м². Запас воды на год, есть небольшая теплица.",
    "Старое убежище, 145 м². Хорошая вентиляция, но мало инструментов."
  ],
  categories: {
    gender: ["Женщина, 27 лет", "Мужчина, 34 года", "Женщина, 42 года", "Мужчина, 23 года"],
    body: ["Высокий и выносливый", "Невысокий и подвижный", "Средний рост, хорошая координация", "Крепкое телосложение"],
    personality: ["Спокойный дипломат", "Любознательный исследователь", "Скептик", "Командный игрок"],
    profession: ["Инженер по водоснабжению", "Агроном", "Метеоролог", "Медик", "Механик", "Учитель"],
    health: ["Хорошее самочувствие", "Сезонная аллергия", "Частые мигрени", "Нуждается в очках"],
    hobby: ["Выращивание растений", "Радиотехника", "Шахматы", "Столярное дело", "Ориентирование"],
    fear: ["Боится темноты", "Боится высоты", "Боится замкнутого пространства", "Боится собак"],
    equipment: ["Портативная солнечная панель", "Набор инструментов", "Велосипед", "Складная тележка"],
    backpack: ["Термос и блокнот", "Фонарик и батарейки", "Семена растений", "Настольная игра"],
    fact: ["Умеет ремонтировать насосы", "Работал волонтёром", "Знает три языка", "Участвовал в экспедиции"],
    ability: ["Один раз поменять свою закрытую карту", "Один раз продлить обсуждение", "Один раз раскрыть дополнительную карту", "Один раз провести повторное голосование"]
  }
};

const record = x => x !== null && typeof x === "object" && !Array.isArray(x);
function field(value, label, max, required = false) {
  if (value === undefined || value === null) {
    if (required) throw Error("Не заполнено поле: " + label);
    return undefined;
  }
  if (typeof value !== "string" || (required && !value.trim()) || value.length > max) {
    throw Error("Некорректное поле: " + label);
  }
  return value.trim();
}
function integer(value, label, maximum) {
  if (!Number.isInteger(value) || value < 0 || value > maximum) {
    throw Error("Некорректное число: " + label);
  }
  return value;
}
function cleanEntry(raw) {
  const item = typeof raw === "string" ? { title: raw } : raw;
  if (!record(item)) throw Error("Карта должна быть строкой или объектом");
  const title = field(item.title, "title", 6000, true);
  // Longer text is useful for a catastrophe, while limits still guard storage and UI.
  const card = { title };
  for (const [key, max] of [["id", 100], ["description", 6000], ["stage", 160]]) {
    const value = field(item[key], key, max);
    if (value !== undefined) card[key] = value;
  }
  for (const prefix of ["age", "height"]) {
    const low = item[prefix + "1"], high = item[prefix + "2"];
    if (low !== undefined || high !== undefined) {
      card[prefix + "1"] = integer(low, prefix + "1", 10000);
      card[prefix + "2"] = integer(high, prefix + "2", 10000);
      if (card[prefix + "1"] > card[prefix + "2"]) throw Error("Диапазон " + prefix + " задан наоборот");
    }
  }
  if (item.weight !== undefined) {
    if (typeof item.weight !== "number" || !Number.isFinite(item.weight) ||
        item.weight <= 0 || item.weight > 1000) throw Error("weight: число от 0 до 1000");
    card.weight = item.weight;
  }
  if (item.tags !== undefined) {
    if (!Array.isArray(item.tags) || item.tags.length > 12) throw Error("tags: максимум 12");
    card.tags = item.tags.map(tag => field(tag, "tag", 40, true));
  }
  if (item.timer !== undefined && item.timer !== null) {
    card.timer = integer(item.timer, "timer", 604800);
  }
  return card;
}
function cleanEntries(values, name, max = 500) {
  if (!Array.isArray(values) || values.length < 1 || values.length > max) {
    throw Error(name + ": нужен непустой список (не более " + max + ")");
  }
  return values.map(cleanEntry);
}
function categoryPart(raw, key) {
  if (Array.isArray(raw)) return { label: CATEGORY_LABELS[key] || key, cards: raw };
  if (!record(raw)) throw Error("Некорректная категория: " + key);
  return {
    label: field(raw.label, "название категории", 70) || CATEGORY_LABELS[key] || key,
    cards: raw.cards
  };
}
export function importBunkerOnline(input) {
  if (!record(input) || !record(input.spec)) throw Error("Это не экспорт Бункер Онлайн (нет spec)");
  const map = {
    sex: "gender", body: "body", trait: "personality", profession: "profession",
    health: "health", hobbies: "hobby", phobia: "fear", inventory: "equipment",
    backpack: "backpack", addInfo: "fact", spec: "ability"
  };
  // Original exports store categories at root level; "spec" is the ability section.
  const categories = {};
  for (const [source, target] of Object.entries(map)) {
    const entries = input[source]?.data;
    if (Array.isArray(entries) && entries.length) {
      categories[target] = { label: CATEGORY_LABELS[target], cards: entries.map(slimOriginal) };
    }
  }
  const bunkerMap = {
    bunkerBedroom: "rooms", bunkerCreated: "construction",
    bunkerLocation: "location", bunkerPresence: "contents",
    bunkerFoodAmount: "supplies", bunkerTimeSpent: "duration",
    bunkerSize: "size"
  };
  const bunker = {};
  for (const [source, target] of Object.entries(bunkerMap)) {
    const entries = input[source]?.data;
    if (Array.isArray(entries) && entries.length) bunker[target] = entries.map(slimOriginal);
  }
  const catastrophes = input.catastrophe?.data;
  return {
    formatVersion: 2, id: "imported-bunker-online", name: "Импорт Бункер Онлайн",
    description: "Импортированный пользовательский пак. Проверьте содержимое перед публикацией.",
    categories,
    catastrophes: Array.isArray(catastrophes) && catastrophes.length ?
      catastrophes.map(slimOriginal) : DEFAULT_INPUT.catastrophes,
    bunker: Object.keys(bunker).length ? bunker : { description: DEFAULT_INPUT.bunkers }
  };
}
function slimOriginal(entry) {
  if (typeof entry === "string") return entry;
  if (!record(entry)) throw Error("Некорректная запись экспорта");
  const result = { title: entry.title };
  for (const key of ["description", "stage", "age1", "age2", "height1", "height2", "timer", "image"]) {
    if (entry[key] !== undefined && entry[key] !== null) result[key] = entry[key];
  }
  if (entry.id !== undefined) result.id = String(entry.id);
  return result;
}
// Authoring-friendly v2 format: one JSON with sections instead of many files.
const PACK_GROUPS = {
  roles: ["profession", "ability"],
  characteristics: ["gender", "body", "personality", "health", "hobby", "fear", "fact"],
  items: ["equipment", "backpack"]
};
const GROUP_FIELDS = [...Object.keys(PACK_GROUPS), "customCategories", "meta", "world"];
function groupedToFlat(input) {
  if (input.categories !== undefined || input.catastrophes !== undefined ||
      input.bunker !== undefined || input.bunkers !== undefined ||
      input.name !== undefined || input.id !== undefined || input.description !== undefined) {
    throw Error("Не смешивай секции v2 с устаревшими полями categories/catastrophes/name");
  }
  for (const key of Object.keys(input)) {
    if (key !== "formatVersion" && !GROUP_FIELDS.includes(key)) {
      throw Error("Неизвестный раздел пака: " + key);
    }
  }
  const categories = {};
  for (const [group, names] of Object.entries(PACK_GROUPS)) {
    const values = input[group] ?? {};
    if (!record(values)) throw Error(group + ": ожидается объект с категориями");
    for (const [key, cards] of Object.entries(values)) {
      if (!names.includes(key)) throw Error(group + ": неизвестная категория " + key);
      categories[key] = cards;
    }
  }
  const extra = input.customCategories ?? {};
  if (!record(extra)) throw Error("customCategories: ожидается объект");
  for (const [key, value] of Object.entries(extra)) {
    if (key in categories || CATEGORIES.includes(key)) throw Error("Повтор категории: " + key);
    categories[key] = value;
  }
  const meta = input.meta ?? {};
  const world = input.world ?? {};
  if (!record(meta) || !record(world)) throw Error("meta и world должны быть объектами");
  for (const key of Object.keys(meta)) {
    if (!["id", "name", "description"].includes(key)) throw Error("meta: неизвестное поле " + key);
  }
  for (const key of Object.keys(world)) {
    if (!["catastrophes", "bunker"].includes(key)) throw Error("world: неизвестное поле " + key);
  }
  return {
    formatVersion: 2, ...meta, categories,
    catastrophes: world.catastrophes, bunker: world.bunker
  };
}
// Used by the importer to export a ready-to-edit SINGLE document.
export function organizePack(source) {
  const pack = normalizePack(source);
  const grouped = {
    formatVersion: 2,
    meta: { id: pack.id, name: pack.name, description: pack.description },
    roles: {}, characteristics: {}, items: {},
    world: { catastrophes: pack.catastrophes, bunker: pack.bunker }
  };
  for (const [group, names] of Object.entries(PACK_GROUPS)) {
    for (const key of names) {
      if (pack.categories[key]) {
        const cards = pack.categories[key];
        const label = pack.categoryLabels[key];
        grouped[group][key] = label !== CATEGORY_LABELS[key] ? { label, cards } : cards;
      }
    }
  }
  const custom = Object.fromEntries(pack.categoryOrder.filter(key => !CATEGORIES.includes(key))
    .map(key => [key, { label: pack.categoryLabels[key], cards: pack.categories[key] }]));
  if (Object.keys(custom).length) grouped.customCategories = custom;
  return grouped;
}

export function normalizePack(source) {
  if (!record(source)) throw Error("Неверный формат пака");
  if (JSON.stringify(source).length > 500000) throw Error("Пак слишком большой (500 КБ)");
  const converted = record(source.spec) ? importBunkerOnline(source) : source;
  const grouped = converted.formatVersion === 2 &&
    GROUP_FIELDS.some(key => Object.hasOwn(converted, key));
  const input = grouped ? groupedToFlat(converted) : converted;
  if (input.formatVersion !== undefined && input.formatVersion !== 2) throw Error("Неизвестная версия пака");
  const v2 = input.formatVersion === 2;
  const inputCategories = input.categories ?? {};
  if (!record(inputCategories)) throw Error("Некорректные категории");
  if (Object.keys(inputCategories).length > 36) throw Error("Слишком много категорий");
  const categories = {}, categoryLabels = {};
  const keys = v2 ? [...new Set([...CATEGORIES, ...Object.keys(inputCategories)])] : CATEGORIES;
  for (const key of keys) {
    if (!/^[a-z][a-z0-9_-]{0,39}$/.test(key)) throw Error("Недопустимый ID категории: " + key);
    const isCustom = !CATEGORIES.includes(key);
    if (isCustom && !v2) throw Error("Неизвестная категория: " + key);
    const raw = inputCategories[key] ?? (isCustom ? undefined : DEFAULT_INPUT.categories[key]);
    if (raw === undefined) continue;
    const part = categoryPart(raw, key);
    categories[key] = cleanEntries(part.cards, key);
    categoryLabels[key] = part.label;
  }
  // Legacy v1 rejects unknown keys as well.
  if (!v2) for (const key of Object.keys(inputCategories)) {
    if (!CATEGORIES.includes(key)) throw Error("Неизвестная категория: " + key);
  }
  const rawCatastrophes = input.catastrophes ?? DEFAULT_INPUT.catastrophes;
  const catastrophes = cleanEntries(rawCatastrophes, "Катаклизмы", 200);
  for (let i = 0; i < catastrophes.length; i++) {
    const image = field(rawCatastrophes[i]?.image, "изображение катаклизма", 240);
    if (image) {
      const asset = image.startsWith("assets/") ? image : "assets/" + image;
      if (!/^assets\/[a-zA-Z0-9_./-]+\.(png|jpe?g|webp)$/i.test(asset) || asset.includes("..") || asset.includes("//")) {
        throw Error("Изображение катаклизма должно быть PNG, JPG или WebP в assets/");
      }
      catastrophes[i].image = asset;
    }
  }
  let bunkerInput;
  if (v2 && input.bunker !== undefined) {
    if (!record(input.bunker)) throw Error("bunker должен быть объектом");
    bunkerInput = input.bunker;
  } else {
    bunkerInput = { description: input.bunkers ?? DEFAULT_INPUT.bunkers };
  }
  if (!Object.keys(bunkerInput).length) throw Error("Добавь хотя бы одну секцию бункера");
  const bunker = {};
  for (const [key, values] of Object.entries(bunkerInput)) {
    if (!(key in BUNKER_LABELS)) throw Error("Неизвестная секция бункера: " + key);
    bunker[key] = cleanEntries(values, "Бункер: " + key, 200);
  }
  return {
    formatVersion: 2,
    id: field(input.id, "id", 80) || "custom",
    name: field(input.name, "name", 80) || "Без названия",
    description: field(input.description, "description", 800) || "",
    categories, categoryLabels, categoryOrder: Object.keys(categories),
    catastrophes, bunker
  };
}
export const DEFAULT_PACK = normalizePack(DEFAULT_INPUT);

const randomFraction = () => crypto.getRandomValues(new Uint32Array(1))[0] / 4294967296;
export function pickEntry(entries) {
  const total = entries.reduce((sum, item) => sum + (item.weight ?? 1), 0);
  let choice = randomFraction() * total;
  for (const item of entries) {
    choice -= (item.weight ?? 1);
    if (choice < 0) return item;
  }
  return entries[entries.length - 1];
}
function randomInt(a, b) {
  return a + Math.floor(randomFraction() * (b - a + 1));
}
function formatted(entry) {
  return entry.title + (entry.description ? "\n" + entry.description : "");
}
export function drawCard(pack, kind) {
  const entry = pickEntry(pack.categories[kind]);
  const details = [];
  if (kind === "gender" && entry.age1 !== undefined) {
    details.push(randomInt(entry.age1, entry.age2) + " лет");
  }
  if (kind === "body" && entry.height1 !== undefined) {
    details.push(randomInt(entry.height1, entry.height2) + " см");
  }
  const title = cardHeading(entry.title, entry.stage);
  return { kind, value: [title, ...details, entry.description].filter(Boolean).join("\n"),
    revealed: false, description: entry.description ?? "",
    ...(kind === "health" && { health: { title: entry.title, degree: entry.stage ?? "" } }) };
}
export function drawScenario(pack) {
  const selected = pickEntry(pack.catastrophes);
  const catastrophe = formatted(selected);
  const lines = [];
  for (const [key, label] of Object.entries(BUNKER_LABELS)) {
    if (pack.bunker[key]?.length) lines.push(label + ": " + formatted(pickEntry(pack.bunker[key])));
  }
  return { catastrophe, bunker: lines.join("\n"), background: selected.image };
}

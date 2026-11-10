// Original placeholder content: NOT copied from bunker-online.com.
export const CATEGORIES = [
  "gender", "body", "personality", "profession", "health", "hobby",
  "fear", "equipment", "backpack", "fact", "ability"
];

export const DEFAULT_PACK = {
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
    ability: ["Один раз поменять свою закрытую карту", "Один раз попросить ведущего продлить обсуждение", "Один раз раскрыть дополнительную карту", "Один раз провести повторное голосование"]
  }
};

export function normalizePack(input) {
  if (!input || typeof input !== "object" || Array.isArray(input)) throw Error("Неверный формат пака");
  const clean = (v, max=500) => {
    if (!Array.isArray(v) || v.length < 1 || v.length > max) throw Error("Неверный список значений");
    return v.map(x => {
      if (typeof x !== "string" || !x.trim() || x.length > 600) throw Error("Некорректное значение карты");
      return x.trim();
    });
  };
  const categories = { ...DEFAULT_PACK.categories };
  if (input.categories !== undefined) {
    if (!input.categories || typeof input.categories !== "object" || Array.isArray(input.categories)) throw Error("Некорректные категории");
    for (const [key, values] of Object.entries(input.categories)) {
      if (!CATEGORIES.includes(key)) throw Error("Неизвестная категория: " + key);
      categories[key] = clean(values);
    }
  }
  return {
    name: typeof input.name === "string" ? input.name.trim().slice(0, 80) || "Без названия" : "Без названия",
    catastrophes: input.catastrophes ? clean(input.catastrophes, 100) : DEFAULT_PACK.catastrophes,
    bunkers: input.bunkers ? clean(input.bunkers, 100) : DEFAULT_PACK.bunkers,
    categories
  };
}

export function swapCards(state, { playerId, targetId, kind }) {
  if (playerId === targetId) throw Error("Выбери другого игрока");
  const source = state.players.find(player => player.id === playerId);
  const target = state.players.find(player => player.id === targetId);
  const sourceIndex = source?.cards.findIndex(card => card.kind === kind) ?? -1;
  const targetIndex = target?.cards.findIndex(card => card.kind === kind) ?? -1;
  if (sourceIndex < 0 || targetIndex < 0) throw Error("У обоих игроков должна быть карточка этой категории");

  // Visibility belongs to the card. Moving it to a revealed slot must not disclose a hidden value.
  const card = source.cards[sourceIndex];
  source.cards[sourceIndex] = target.cards[targetIndex];
  target.cards[targetIndex] = card;
}

export function cardHeading(title, degree) {
  return degree ? title + " (" + degree + ")" : title;
}

function healthParts(card, entries) {
  if (card.health) {
    const { title, degree } = card.health;
    return { title, degree, tail: card.value.slice(cardHeading(title, degree).length) };
  }

  // Old rooms store the stage in the display text. Match against the active pack
  // first so literal parentheses in the title aren't mistaken for a stage.
  for (const entry of entries) {
    if (typeof entry?.title !== "string") continue;
    const heading = cardHeading(entry.title, entry.stage);
    if (card.value === heading || card.value.startsWith(heading + "\n")) {
      return { title: entry.title, degree: entry.stage ?? "", tail: card.value.slice(heading.length) };
    }
  }
  const newline = card.value.indexOf("\n");
  const title = newline < 0 ? card.value : card.value.slice(0, newline);
  const tail = newline < 0 ? "" : card.value.slice(newline);
  const labeled = /^\n(?:Уровень\/примечание|Степень): ([^\n]+)(?=\n|$)/.exec(tail);
  if (labeled && !card.description?.startsWith(labeled[0].slice(1))) {
    return { title, degree: labeled[1].trim(), tail: tail.slice(labeled[0].length) };
  }
  return { title, degree: "", tail };
}

export function healthDegree(card, entries = []) {
  return healthParts(card, entries).degree;
}

export function setHealthDegree(card, degree, entries = []) {
  if (card.kind !== "health") throw Error("Степень можно изменить только у карты здоровья");
  if (typeof degree !== "string" || degree.trim().length > 160 || /[\r\n]/.test(degree)) {
    throw Error("Степень здоровья должна помещаться в одну строку до 160 символов");
  }
  const { title, tail } = healthParts(card, entries);
  const trimmed = degree.trim();
  card.health = { title, degree: trimmed };
  card.value = cardHeading(title, trimmed) + tail;
}

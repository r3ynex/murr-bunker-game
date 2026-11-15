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

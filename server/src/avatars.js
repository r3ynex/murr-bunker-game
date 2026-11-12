export function avatarFrom(value) {
  return Number.isInteger(value) && value >= 1 && value <= 0xffffffff ? value : 1;
}

export function publicAvatar(value) {
  const avatar = avatarFrom(value);
  // Older desktop clients deserialize avatar as an unsigned byte.
  return avatar <= 255 ? { avatar } : { avatar: 1, avatarId: avatar };
}

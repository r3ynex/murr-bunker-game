import { test } from "node:test";
import { strict as assert } from "node:assert";
import { avatarFrom, publicAvatar } from "../src/avatars.js";

test("avatar IDs above 12 survive room creation and display", () => {
  for (const id of [1, 12, 13, 255]) {
    assert.equal(avatarFrom(id), id);
    assert.deepEqual(publicAvatar(id), { avatar: id });
  }
  for (const id of [256, 40000, 0xffffffff]) {
    assert.equal(avatarFrom(id), id);
    assert.deepEqual(publicAvatar(id), { avatar: 1, avatarId: id });
  }
  for (const invalid of [undefined, "13", 0, -1, 13.5, 0x100000000]) {
    assert.equal(avatarFrom(invalid), 1);
  }
});

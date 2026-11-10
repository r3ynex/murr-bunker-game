import { test } from "node:test";
import { strict as assert } from "node:assert";
import { CATEGORIES, DEFAULT_PACK, normalizePack } from "../src/content.js";

test("demo pack has all character categories", () => {
  assert.equal(CATEGORIES.length, 11);
  for (const category of CATEGORIES) {
    assert.ok(DEFAULT_PACK.categories[category].length >= 3, category);
  }
});
test("custom partial pack retains fallback content", () => {
  const pack = normalizePack({ name: "My pack", categories: { profession: ["Картограф"] } });
  assert.deepEqual(pack.categories.profession, ["Картограф"]);
  assert.deepEqual(pack.categories.health, DEFAULT_PACK.categories.health);
});
test("invalid custom packs are rejected", () => {
  assert.throws(() => normalizePack({ categories: { profession: [] } }));
  assert.throws(() => normalizePack({ categories: { unknown: ["X"] } }));
});

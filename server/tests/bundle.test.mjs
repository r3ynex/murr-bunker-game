import { test } from "node:test";
import { strict as assert } from "node:assert";
import { zipSync, strToU8 } from "fflate";
import { readBundle, MAX_ZIP_BYTES } from "../src/bundle.js";

const pack = JSON.stringify({
  formatVersion: 2,
  meta: { name: "Тест ZIP" },
  roles: { profession: ["Биолог", "Инженер"] },
  world: { catastrophes: ["Сильная буря"], bunker: { location: ["В горах"] } }
});
const transparentPng = new Uint8Array(Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO/oYecAAAAASUVORK5CYII=",
  "base64"
));

test("ZIP sync includes pack.json and optional catastrophe background", () => {
  const bytes = zipSync({
    "pack.json": strToU8(pack),
    "assets/": new Uint8Array(),
    "assets/catastrophe.png": transparentPng
  }, { level: 6 });
  const bundle = readBundle(bytes);
  assert.equal(bundle.pack.name, "Тест ZIP");
  assert.equal(bundle.background, "assets/catastrophe.png");
  assert.equal(bundle.images.length, 1);
});

test("ZIP rejects file traversal and unsupported file types", () => {
  const bad = zipSync({ "pack.json": strToU8(pack), "../evil.exe": strToU8("bad") });
  assert.throws(() => readBundle(bad), /разрешены|пределы|ZIP/);
  const executable = zipSync({ "pack.json": strToU8(pack), "assets/script.js": strToU8("x") });
  assert.throws(() => readBundle(executable), /ZIP/);
  const invalidPack = zipSync({ "pack.json": strToU8("not JSON") });
  assert.throws(() => readBundle(invalidPack));
});

test("ZIP enforces 2 MB compressed cap and checks image signature", () => {
  assert.throws(() => readBundle(new Uint8Array(MAX_ZIP_BYTES + 1)), /2 МБ/);
  const imageAsText = zipSync({
    "pack.json": strToU8(pack), "assets/catastrophe.png": strToU8("fake image")
  });
  assert.throws(() => readBundle(imageAsText), /изображения/);
});

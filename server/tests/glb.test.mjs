import { strict as assert } from "node:assert";
import { test } from "node:test";
import { parseFaceMaterialName, parseGlb } from "../../tools/glb-to-dice.mjs";

function makeSyntheticCubeGlb() {
  // 6 separate quads (24 verts, 12 tris), one primitive per face with material face_01..face_06.
  const faceSpecs = [
    { name: "face_01", verts: [[-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]], color: [0.9, 0.5, 0.1, 1] },
    { name: "face_02", verts: [[-1, -1, -1], [-1, -1, 1], [-1, 1, 1], [-1, 1, -1]], color: [0.2, 0.6, 0.7, 1] },
    { name: "face_03", verts: [[-1, 1, -1], [-1, 1, 1], [1, 1, 1], [1, 1, -1]], color: [0.2, 0.6, 0.7, 1] },
    { name: "face_04", verts: [[-1, -1, -1], [1, -1, -1], [1, -1, 1], [-1, -1, 1]], color: [0.2, 0.6, 0.7, 1] },
    { name: "face_05", verts: [[1, -1, 1], [1, -1, -1], [1, 1, -1], [1, 1, 1]], color: [0.2, 0.6, 0.7, 1] },
    { name: "face_06", verts: [[1, -1, -1], [-1, -1, -1], [-1, 1, -1], [1, 1, -1]], color: [0.2, 0.6, 0.7, 1] },
  ];

  const posFloats = [];
  const idxShorts = [];
  const accessors = [];
  const bufferViews = [];
  const materials = [];
  const primitives = [];

  for (let f = 0; f < faceSpecs.length; f++) {
    const spec = faceSpecs[f];
    const posOffset = posFloats.length * 4;
    for (const v of spec.verts) posFloats.push(...v);
    bufferViews.push({ buffer: 0, byteOffset: posOffset, byteLength: 4 * 3 * 4 });
    const posAcc = accessors.length;
    accessors.push({ bufferView: bufferViews.length - 1, componentType: 5126, count: 4, type: "VEC3" });

    materials.push({ name: spec.name, pbrMetallicRoughness: { baseColorFactor: spec.color } });
    primitives.push({ attributes: { POSITION: posAcc }, indices: null, material: f });
  }

  const posBytes = posFloats.length * 4;
  for (let f = 0; f < faceSpecs.length; f++) {
    const idxOffset = posBytes + idxShorts.length * 2;
    idxShorts.push(0, 1, 2, 0, 2, 3);
    bufferViews.push({ buffer: 0, byteOffset: idxOffset, byteLength: 6 * 2 });
    const idxAcc = accessors.length;
    accessors.push({ bufferView: bufferViews.length - 1, componentType: 5123, count: 6, type: "SCALAR" });
    primitives[f].indices = idxAcc;
  }

  const binUnpadded = posBytes + idxShorts.length * 2;
  const binLen = (binUnpadded + 3) & ~3;
  const bin = new Uint8Array(binLen);
  const binView = new DataView(bin.buffer);
  posFloats.forEach((v, i) => binView.setFloat32(i * 4, v, true));
  idxShorts.forEach((v, i) => binView.setUint16(posBytes + i * 2, v, true));

  const gltf = {
    asset: { version: "2.0" },
    scene: 0,
    scenes: [{ nodes: [0] }],
    nodes: [{ mesh: 0 }],
    meshes: [{ primitives }],
    materials,
    accessors,
    bufferViews,
    buffers: [{ byteLength: binLen }],
  };

  const jsonRaw = new TextEncoder().encode(JSON.stringify(gltf));
  const jsonLen = (jsonRaw.length + 3) & ~3;
  const jsonChunk = new Uint8Array(jsonLen);
  jsonChunk.fill(0x20);
  jsonChunk.set(jsonRaw);

  const totalLen = 12 + 8 + jsonLen + 8 + binLen;
  const out = new Uint8Array(totalLen);
  const dv = new DataView(out.buffer);
  dv.setUint32(0, 0x46546c67, true);
  dv.setUint32(4, 2, true);
  dv.setUint32(8, totalLen, true);
  dv.setUint32(12, jsonLen, true);
  dv.setUint32(16, 0x4e4f534a, true);
  out.set(jsonChunk, 20);
  const binHeader = 20 + jsonLen;
  dv.setUint32(binHeader, binLen, true);
  dv.setUint32(binHeader + 4, 0x004e4942, true);
  out.set(bin, binHeader + 8);
  return out;
}

test("parseFaceMaterialName extracts face_01..face_20 and ignores non-face materials", () => {
  assert.equal(parseFaceMaterialName("face_01"), 1);
  assert.equal(parseFaceMaterialName("face_20"), 20);
  assert.equal(parseFaceMaterialName("Die_Face-07.001"), 7);
  assert.equal(parseFaceMaterialName("body_bevel"), null);
});

test("parseGlb extracts vertices, triangles, colors, and outward face anchors", () => {
  const mesh = parseGlb(makeSyntheticCubeGlb(), 6);
  assert.equal(mesh.faces.length, 6);
  assert.equal(mesh.triangles.length, 12);
  for (let v = 1; v <= 6; v++) {
    const face = mesh.faces.find(f => f.value === v);
    assert.ok(face, `face ${v} exists`);
    const outward = face.normal[0] * face.center[0] + face.normal[1] * face.center[1] + face.normal[2] * face.center[2];
    assert.ok(outward > 0.2, `face ${v} normal points outward`);
  }
  const f1Tri = mesh.triangles.find(t => t.face === 1);
  assert.ok(f1Tri?.color && f1Tri.color[0] > 200);
});

test("repository d6.glb and d20.glb parse with all numbered faces and UVs", async () => {
  const { readFileSync } = await import("node:fs");
  const { fileURLToPath } = await import("node:url");
  const path = await import("node:path");
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

  for (const [file, sides, tris] of [["d6.glb", 6, 12], ["d20.glb", 20, 20]]) {
    const parsed = parseGlb(readFileSync(path.join(root, file)), sides);
    assert.equal(parsed.faces.length, sides, `${file} face count`);
    assert.equal(parsed.triangles.length, tris, `${file} triangle count`);
    assert.equal(parsed.uvs?.length, parsed.vertices.length, `${file} UV count`);
    for (let v = 1; v <= sides; v++) {
      const face = parsed.faces.find(f => f.value === v);
      assert.ok(face, `${file} face ${v} exists`);
      const outward = face.normal[0] * face.center[0] + face.normal[1] * face.center[1] + face.normal[2] * face.center[2];
      assert.ok(outward > 0.2, `${file} face ${v} normal points outward`);
      const ortho = face.right[0] * face.normal[0] + face.right[1] * face.normal[1] + face.right[2] * face.normal[2];
      assert.ok(Math.abs(ortho) < 1e-3, `${file} face ${v} right orthogonal to normal`);
    }
  }
});


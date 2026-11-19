#!/usr/bin/env node
// Converts glTF 2.0 binary (.glb) dice models with material names face_01..face_20
// into the shared dice-models.json format (or validates them).
// Usage: node tools/glb-to-dice.mjs <d6.glb> <d20.glb> [output.json]
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const GLB_MAGIC = 0x46546c67; // "glTF"
const CHUNK_JSON = 0x4e4f534a; // "JSON"
const CHUNK_BIN = 0x004e4942; // "BIN\0"

const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const mul = (a, s) => [a[0] * s, a[1] * s, a[2] * s];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0]
];
const len = a => Math.hypot(a[0], a[1], a[2]);
const unit = a => {
  const l = len(a);
  return l > 1e-8 ? mul(a, 1 / l) : [0, 0, 1];
};

function identityMat4() {
  return [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
}

function mulMat4(a, b) {
  const out = new Array(16).fill(0);
  for (let col = 0; col < 4; col++) {
    for (let row = 0; row < 4; row++) {
      let sum = 0;
      for (let k = 0; k < 4; k++) sum += a[k * 4 + row] * b[col * 4 + k];
      out[col * 4 + row] = sum;
    }
  }
  return out;
}

function nodeLocalMat4(node) {
  if (Array.isArray(node.matrix) && node.matrix.length === 16) return node.matrix;
  const [tx, ty, tz] = node.translation ?? [0, 0, 0];
  const [qx, qy, qz, qw] = node.rotation ?? [0, 0, 0, 1];
  const [sx, sy, sz] = node.scale ?? [1, 1, 1];
  const x2 = qx + qx, y2 = qy + qy, z2 = qz + qz;
  const xx = qx * x2, xy = qx * y2, xz = qx * z2;
  const yy = qy * y2, yz = qy * z2, zz = qz * z2;
  const wx = qw * x2, wy = qw * y2, wz = qw * z2;
  return [
    (1 - (yy + zz)) * sx, (xy + wz) * sx, (xz - wy) * sx, 0,
    (xy - wz) * sy, (1 - (xx + zz)) * sy, (yz + wx) * sy, 0,
    (xz + wy) * sz, (yz - wx) * sz, (1 - (xx + yy)) * sz, 0,
    tx, ty, tz, 1
  ];
}

function transformPoint(m, [x, y, z]) {
  return [
    m[0] * x + m[4] * y + m[8] * z + m[12],
    m[1] * x + m[5] * y + m[9] * z + m[13],
    m[2] * x + m[6] * y + m[10] * z + m[14]
  ];
}

function linearToSrgbByte(c) {
  const clamped = Math.max(0, Math.min(1, c));
  const srgb = clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * (clamped ** (1 / 2.4)) - 0.055;
  return Math.round(Math.max(0, Math.min(1, srgb)) * 255);
}

export function parseFaceMaterialName(name) {
  if (typeof name !== "string") return null;
  const match = /(?:^|[^a-z0-9])face[_-]?0*(\d+)(?:[^a-z0-9]|$)/i.exec(name.trim());
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isInteger(value) && value >= 1 && value <= 100 ? value : null;
}

function readAccessorVec3(gltf, bin, accessorIndex) {
  const accessor = gltf.accessors?.[accessorIndex];
  if (!accessor || accessor.type !== "VEC3" || accessor.componentType !== 5126) {
    throw new Error("POSITION accessor must be float32 VEC3");
  }
  const view = gltf.bufferViews?.[accessor.bufferView];
  if (!view) throw new Error("Missing bufferView for POSITION");
  const stride = view.byteStride || 12;
  const base = (view.byteOffset || 0) + (accessor.byteOffset || 0);
  const dv = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  const out = [];
  for (let i = 0; i < accessor.count; i++) {
    const off = base + i * stride;
    out.push([dv.getFloat32(off, true), dv.getFloat32(off + 4, true), dv.getFloat32(off + 8, true)]);
  }
  return out;
}

function readAccessorVec2(gltf, bin, accessorIndex) {
  const accessor = gltf.accessors?.[accessorIndex];
  if (!accessor || accessor.type !== "VEC2" || accessor.componentType !== 5126) {
    return null;
  }
  const view = gltf.bufferViews?.[accessor.bufferView];
  if (!view) return null;
  const stride = view.byteStride || 8;
  const base = (view.byteOffset || 0) + (accessor.byteOffset || 0);
  const dv = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  const out = [];
  for (let i = 0; i < accessor.count; i++) {
    const off = base + i * stride;
    out.push([dv.getFloat32(off, true), dv.getFloat32(off + 4, true)]);
  }
  return out;
}

function readAccessorIndices(gltf, bin, accessorIndex) {
  const accessor = gltf.accessors?.[accessorIndex];
  if (!accessor) throw new Error("Missing indices accessor");
  const view = gltf.bufferViews?.[accessor.bufferView];
  if (!view) throw new Error("Missing bufferView for indices");
  const base = (view.byteOffset || 0) + (accessor.byteOffset || 0);
  const dv = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  const out = [];
  for (let i = 0; i < accessor.count; i++) {
    if (accessor.componentType === 5121) out.push(dv.getUint8(base + i));
    else if (accessor.componentType === 5123) out.push(dv.getUint16(base + i * 2, true));
    else if (accessor.componentType === 5125) out.push(dv.getUint32(base + i * 4, true));
    else throw new Error("Unsupported index componentType: " + accessor.componentType);
  }
  return out;
}

export function parseGlb(bytes, expectedSides) {
  const buf = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  if (buf.byteLength < 20) throw new Error("GLB file too small");
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  if (dv.getUint32(0, true) !== GLB_MAGIC) throw new Error("Not a valid GLB file");
  if (dv.getUint32(4, true) !== 2) throw new Error("Only glTF 2.0 GLB is supported");

  let gltf = null;
  let bin = null;
  let offset = 12;
  while (offset + 8 <= buf.byteLength) {
    const chunkLen = dv.getUint32(offset, true);
    const chunkType = dv.getUint32(offset + 4, true);
    offset += 8;
    const chunk = buf.subarray(offset, offset + chunkLen);
    if (chunkType === CHUNK_JSON) {
      gltf = JSON.parse(new TextDecoder("utf-8").decode(chunk));
    } else if (chunkType === CHUNK_BIN) {
      bin = chunk;
    }
    offset += chunkLen;
  }
  if (!gltf || !bin) throw new Error("GLB is missing JSON or BIN chunk");

  const nodes = gltf.nodes ?? [];
  const childSet = new Set();
  for (const node of nodes) {
    for (const c of node.children ?? []) childSet.add(c);
  }
  const sceneIdx = gltf.scene ?? 0;
  const roots = gltf.scenes?.[sceneIdx]?.nodes ?? nodes.map((_, i) => i).filter(i => !childSet.has(i));

  const vertices = [];
  const uvs = [];
  const triangles = [];
  let hasAnyUvs = false;

  function visit(nodeIdx, parentMat) {
    const node = nodes[nodeIdx];
    if (!node) return;
    const world = mulMat4(parentMat, nodeLocalMat4(node));
    if (node.mesh !== undefined) {
      const mesh = gltf.meshes?.[node.mesh];
      for (const prim of mesh?.primitives ?? []) {
        if (prim.mode !== undefined && prim.mode !== 4) continue; // TRIANGLES only
        if (prim.attributes?.POSITION === undefined) continue;
        const positions = readAccessorVec3(gltf, bin, prim.attributes.POSITION);
        const primUvs = prim.attributes.TEXCOORD_0 !== undefined
          ? readAccessorVec2(gltf, bin, prim.attributes.TEXCOORD_0)
          : null;
        const validUvs = primUvs && primUvs.length === positions.length ? primUvs : null;
        if (validUvs) hasAnyUvs = true;
        const baseVertex = vertices.length;
        for (let k = 0; k < positions.length; k++) {
          vertices.push(transformPoint(world, positions[k]));
          uvs.push(validUvs ? validUvs[k] : [0, 0]);
        }
        const indices = prim.indices !== undefined
          ? readAccessorIndices(gltf, bin, prim.indices)
          : positions.map((_, i) => i);
        const mat = prim.material !== undefined ? gltf.materials?.[prim.material] : null;
        const faceValue = parseFaceMaterialName(mat?.name ?? "");
        const baseColor = mat?.pbrMetallicRoughness?.baseColorFactor;
        const rgb = Array.isArray(baseColor) && baseColor.length >= 3
          ? [linearToSrgbByte(baseColor[0]), linearToSrgbByte(baseColor[1]), linearToSrgbByte(baseColor[2])]
          : null;
        for (let i = 0; i + 2 < indices.length; i += 3) {
          triangles.push({
            i: [baseVertex + indices[i], baseVertex + indices[i + 1], baseVertex + indices[i + 2]],
            face: faceValue,
            color: rgb
          });
        }
      }
    }
    for (const child of node.children ?? []) visit(child, world);
  }

  for (const root of roots) visit(root, identityMat4());
  if (vertices.length === 0 || triangles.length === 0) {
    throw new Error("GLB contains no triangle meshes");
  }

  // Center at origin and normalize max radius to 0.96 so it matches bundled die scale.
  const min = [Infinity, Infinity, Infinity];
  const max = [-Infinity, -Infinity, -Infinity];
  for (const v of vertices) {
    for (let k = 0; k < 3; k++) {
      if (v[k] < min[k]) min[k] = v[k];
      if (v[k] > max[k]) max[k] = v[k];
    }
  }
  const center = mul(add(min, max), 0.5);
  let maxDist = 0;
  for (let i = 0; i < vertices.length; i++) {
    vertices[i] = sub(vertices[i], center);
    const d = len(vertices[i]);
    if (d > maxDist) maxDist = d;
  }
  const scale = maxDist > 1e-6 ? 0.96 / maxDist : 1;
  for (let i = 0; i < vertices.length; i++) {
    vertices[i] = mul(vertices[i], scale).map(n => Number(n.toFixed(5)));
  }

  // Group face triangles by material face number (face_01 -> 1 .. face_N -> N).
  const byFace = new Map();
  for (const tri of triangles) {
    if (tri.face !== null) {
      if (!byFace.has(tri.face)) byFace.set(tri.face, []);
      byFace.get(tri.face).push(tri);
    }
  }

  const faces = [];
  const count = expectedSides ?? byFace.size;
  for (let value = 1; value <= count; value++) {
    const tris = byFace.get(value);
    if (!tris || tris.length === 0) {
      throw new Error(`Missing material face_${String(value).padStart(2, "0")} in GLB`);
    }
    let normalSum = [0, 0, 0];
    let centerSum = [0, 0, 0];
    let tangentSum = [0, 0, 0];
    let areaSum = 0;
    for (const tri of tris) {
      const a = vertices[tri.i[0]], b = vertices[tri.i[1]], c = vertices[tri.i[2]];
      const n = cross(sub(b, a), sub(c, a));
      const area = len(n) * 0.5;
      if (area < 1e-8) continue;
      const mid = mul(add(add(a, b), c), 1 / 3);
      const oriented = dot(n, mid) >= 0 ? n : mul(n, -1);
      normalSum = add(normalSum, oriented);
      centerSum = add(centerSum, mul(mid, area));
      areaSum += area;

      const uv0 = uvs[tri.i[0]], uv1 = uvs[tri.i[1]], uv2 = uvs[tri.i[2]];
      const du1 = uv1[0] - uv0[0], dv1 = uv1[1] - uv0[1];
      const du2 = uv2[0] - uv0[0], dv2 = uv2[1] - uv0[1];
      const det = du1 * dv2 - du2 * dv1;
      if (Math.abs(det) > 1e-8) {
        const dp1 = sub(b, a), dp2 = sub(c, a);
        const tangent = mul(sub(mul(dp1, dv2), mul(dp2, dv1)), 1 / det);
        if (len(tangent) > 1e-8) tangentSum = add(tangentSum, mul(unit(tangent), area));
      }
    }
    const normal = unit(normalSum);
    const faceCenter = areaSum > 1e-8 ? mul(centerSum, 1 / areaSum) : [0, 0, 0];
    const tProj = sub(tangentSum, mul(normal, dot(tangentSum, normal)));
    let right;
    if (len(tProj) > 1e-5) {
      right = unit(tProj);
    } else {
      const ref = Math.abs(normal[1]) < 0.9 ? [0, 1, 0] : [0, 0, 1];
      right = unit(cross(ref, normal));
    }
    const up = unit(cross(normal, right));
    faces.push({
      value,
      center: faceCenter.map(n => Number(n.toFixed(5))),
      normal: normal.map(n => Number(n.toFixed(5))),
      right: right.map(n => Number(n.toFixed(5))),
      up: up.map(n => Number(n.toFixed(5))),
      triCount: tris.length
    });
  }

  // Ensure outward triangle winding for rendering.
  const outTriangles = triangles.map(tri => {
    const [i0, i1, i2] = tri.i;
    const a = vertices[i0], b = vertices[i1], c = vertices[i2];
    const n = cross(sub(b, a), sub(c, a));
    const mid = mul(add(add(a, b), c), 1 / 3);
    const indices = dot(n, mid) >= 0 ? [i0, i1, i2] : [i0, i2, i1];
    const entry = { indices };
    if (tri.face !== null) entry.face = tri.face;
    if (tri.color) entry.color = tri.color;
    return entry;
  });

  return hasAnyUvs
    ? { vertices, uvs: uvs.map(u => u.map(n => Number(n.toFixed(5)))), faces, triangles: outTriangles }
    : { vertices, faces, triangles: outTriangles };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [d6Path, d20Path, outPath = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../web/dice-models.json")] = process.argv.slice(2);
  if (!d6Path || !d20Path) {
    console.error("Usage: node tools/glb-to-dice.mjs <d6.glb> <d20.glb> [output.json]");
    process.exit(1);
  }
  const d6 = parseGlb(readFileSync(d6Path), 6);
  const d20 = parseGlb(readFileSync(d20Path), 20);
  writeFileSync(outPath, JSON.stringify({ d6, d20 }) + "\n", "utf8");
  console.log(`Converted GLB models -> ${outPath}`);
  console.log(`  D6:  ${d6.vertices.length} verts, ${d6.triangles.length} tris, ${d6.faces.length} faces`);
  console.log(`  D20: ${d20.vertices.length} verts, ${d20.triangles.length} tris, ${d20.faces.length} faces`);
}

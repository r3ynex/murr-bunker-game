const canvas = document.getElementById("diceOverlay");
const drawing = canvas.getContext("2d");
let meshes;
let active;
let frame = 0;

const GLB_MAGIC = 0x46546c67;
const CHUNK_JSON = 0x4e4f534a;
const CHUNK_BIN = 0x004e4942;

const add = (a, b) => [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
const mul = (a, scalar) => [a[0] * scalar, a[1] * scalar, a[2] * scalar];
const sub = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
const len = a => Math.hypot(a[0], a[1], a[2]);
const unit = a => {
  const l = len(a);
  return l > 1e-8 ? mul(a, 1 / l) : [0, 0, 1];
};
const cross = ([x, y, z], [a, b, c]) => [y * c - z * b, z * a - x * c, x * b - y * a];
const clamp = value => Math.max(0, Math.min(1, value));
const smooth = value => { const t = clamp(value); return t * t * (3 - 2 * t); };
const color = (rgb, alpha = 1) => `rgba(${rgb.join(",")},${alpha})`;

const SPIN_AXIS = unit([0.32, 0.92, 0.22]);

function rotateEuler([x, y, z], rx, ry, rz) {
  const sx = Math.sin(rx), cx = Math.cos(rx), sy = Math.sin(ry), cy = Math.cos(ry);
  const sz = Math.sin(rz), cz = Math.cos(rz);
  const y1 = cx * y - sx * z, z1 = sx * y + cx * z;
  const x2 = cy * x + sy * z1;
  return [cz * x2 - sz * y1, sz * x2 + cz * y1, cy * z1 - sy * x];
}

// Rodrigues' single-axis rotation: strictly unidirectional spin with zero gimbal wobble.
function rotateAxis(v, axis, angle) {
  const c = Math.cos(angle);
  const s = Math.sin(angle);
  const axv = cross(axis, v);
  const adv = dot(axis, v);
  return [
    v[0] * c + axv[0] * s + axis[0] * adv * (1 - c),
    v[1] * c + axv[1] * s + axis[1] * adv * (1 - c),
    v[2] * c + axv[2] * s + axis[2] * adv * (1 - c),
  ];
}

function basis(points) {
  const center = mul(points.reduce((sum, p) => add(sum, p), [0, 0, 0]), 1 / points.length);
  const normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])));
  const top = points.length === 4 ? mul(add(points[0], points[1]), 0.5) : points[0];
  const right = unit(cross(sub(top, center), normal));
  return { center, normal, right, up: cross(normal, right) };
}

function faceAnchor(mesh, face) {
  if (face.normal && face.right && face.up && face.center) {
    return { center: face.center, normal: face.normal, right: face.right, up: face.up };
  }
  return basis(face.indices.map(index => mesh.vertices[index]));
}

function orientPoint(v, anchor, t) {
  // Align winning face straight toward viewer (+Z), apply gentle 3D resting tilt,
  // then spin monotonically in ONE direction around a fixed axis and settle cleanly at angle = 0.
  const aligned = [dot(v, anchor.right), dot(v, anchor.up), dot(v, anchor.normal)];
  const resting = rotateEuler(aligned, 0.14, -0.18, 0.04);
  const remaining = (1 - clamp(t)) ** 3;
  const angle = remaining * Math.PI * 2 * 3;
  return rotateAxis(resting, SPIN_AXIS, angle);
}

function fillPolygon(points, fill) {
  drawing.beginPath();
  points.forEach(([x, y], i) => i ? drawing.lineTo(x, y) : drawing.moveTo(x, y));
  drawing.closePath();
  drawing.fillStyle = fill;
  drawing.fill();
}

function screenDoubleArea(p0, p1, p2) {
  return (p1[0] - p0[0]) * (p2[1] - p0[1]) - (p1[1] - p0[1]) * (p2[0] - p0[0]);
}

function drawTexturedTriangle(img, p0, p1, p2, uv0, uv1, uv2, opacity) {
  const w = img.width;
  const h = img.height;
  const s0 = uv0[0] * w, t0 = uv0[1] * h;
  const s1 = uv1[0] * w, t1 = uv1[1] * h;
  const s2 = uv2[0] * w, t2 = uv2[1] * h;

  const ds1 = s1 - s0, dt1 = t1 - t0;
  const ds2 = s2 - s0, dt2 = t2 - t0;
  const det = ds1 * dt2 - ds2 * dt1;
  if (Math.abs(det) < 1e-6) return;
  const inv = 1 / det;

  const dx1 = p1[0] - p0[0], dy1 = p1[1] - p0[1];
  const dx2 = p2[0] - p0[0], dy2 = p2[1] - p0[1];
  const a = (dx1 * dt2 - dx2 * dt1) * inv;
  const b = (dy1 * dt2 - dy2 * dt1) * inv;
  const c = (dx2 * ds1 - dx1 * ds2) * inv;
  const d = (dy2 * ds1 - dy1 * ds2) * inv;
  const e = p0[0] - a * s0 - c * t0;
  const f = p0[1] - b * s0 - d * t0;

  // Slightly expand the clip triangle outward from its 2D centroid to prevent AA hairline seams.
  const cx = (p0[0] + p1[0] + p2[0]) / 3;
  const cy = (p0[1] + p1[1] + p2[1]) / 3;
  const expand = p => {
    const vx = p[0] - cx, vy = p[1] - cy;
    const l = Math.hypot(vx, vy);
    return l > 1e-4 ? [p[0] + (vx / l) * 0.45, p[1] + (vy / l) * 0.45] : p;
  };
  const e0 = expand(p0), e1 = expand(p1), e2 = expand(p2);

  const sx = Math.max(0, Math.floor(Math.min(s0, s1, s2)) - 2);
  const sy = Math.max(0, Math.floor(Math.min(t0, t1, t2)) - 2);
  const sx2 = Math.min(w, Math.ceil(Math.max(s0, s1, s2)) + 2);
  const sy2 = Math.min(h, Math.ceil(Math.max(t0, t1, t2)) + 2);
  const sw = sx2 - sx;
  const sh = sy2 - sy;
  if (sw <= 0 || sh <= 0) return;

  drawing.save();
  drawing.globalAlpha = opacity;
  drawing.beginPath();
  drawing.moveTo(e0[0], e0[1]);
  drawing.lineTo(e1[0], e1[1]);
  drawing.lineTo(e2[0], e2[1]);
  drawing.closePath();
  drawing.clip();
  drawing.transform(a, b, c, d, e, f);
  drawing.drawImage(img, sx, sy, sw, sh, sx, sy, sw, sh);
  drawing.restore();
}

function label(text, x, y, size, fill, bold = false) {
  drawing.font = `${bold ? 700 : 500} ${size}px Manrope, system-ui, sans-serif`;
  drawing.fillStyle = fill;
  drawing.textAlign = "center";
  drawing.textBaseline = "middle";
  drawing.fillText(text, x, y);
}

function pips(value) {
  const dots = [];
  if (value % 2) dots.push([0, 0]);
  if (value >= 2) dots.push([-0.29, 0.29], [0.29, -0.29]);
  if (value >= 4) dots.push([0.29, 0.29], [-0.29, -0.29]);
  if (value === 6) dots.push([-0.29, 0], [0.29, 0]);
  return dots;
}

function drawDie(mesh, roll, x, y, radius, t, opacity) {
  const targetFace = mesh.faces.find(face => face.value === roll.value);
  if (!targetFace) return;
  const anchor = faceAnchor(mesh, targetFace);
  const transform = v => orientPoint(v, anchor, t);
  const vertices = mesh.vertices.map(transform);
  const project = ([a, b, z]) => {
    const denom = Math.max(0.5, 3.8 - z);
    const lens = radius * 3.8 / denom;
    return [x + a * lens, y - b * lens];
  };
  const defaultBase = roll.sides === 6 ? [43, 120, 139] : [188, 112, 34];
  const light = unit([-0.44, 0.62, 0.80]);

  if (Array.isArray(mesh.triangles) && mesh.triangles.length > 0) {
    const hasTextures = Array.isArray(mesh.textures) && mesh.textures.some(Boolean) && Array.isArray(mesh.uvs) && mesh.uvs.length === mesh.vertices.length;

    if (hasTextures && Array.isArray(mesh.faceGroups) && mesh.faceGroups.length > 0) {
      // Render textured faces back-to-front with ZERO strokes and ONE unified shading fill per face.
      const visibleGroups = [];
      for (const group of mesh.faceGroups) {
        const fc = transform(group.center);
        const fn = unit(sub(transform(add(group.center, mul(group.normal, 0.2))), fc));
        if (dot(sub([0, 0, 3.8], fc), fn) <= 1e-4) continue;
        visibleGroups.push({ group, z: fc[2], normal: fn });
      }
      visibleGroups.sort((a, b) => a.z - b.z);

      for (const { group, normal } of visibleGroups) {
        let drewAny = false;
        for (const tri of group.subTriangles) {
          const [i0, i1, i2] = tri.indices;
          const p0 = project(vertices[i0]);
          const p1 = project(vertices[i1]);
          const p2 = project(vertices[i2]);
          if (screenDoubleArea(p0, p1, p2) > -0.25) continue;
          const img = tri.image !== null && tri.image !== undefined ? mesh.textures[tri.image] : null;
          if (img) {
            drawTexturedTriangle(img, p0, p1, p2, mesh.uvs[i0], mesh.uvs[i1], mesh.uvs[i2], opacity);
            drewAny = true;
          } else {
            const strength = Math.min(1, 0.34 + Math.max(0, dot(normal, light)) * 0.72);
            const base = tri.color || defaultBase;
            fillPolygon([p0, p1, p2], color(base.map(c => Math.round(c * strength)), opacity));
          }
        }
        // Apply unified diffuse shading once over the entire face boundary (no diagonal/sub-tri seams).
        if (drewAny && group.boundary && group.boundary.length >= 3) {
          const strength = Math.min(1, 0.64 + Math.max(0, dot(normal, light)) * 0.36);
          const shadeAlpha = (1 - strength) * opacity;
          if (shadeAlpha > 0.004) {
            fillPolygon(group.boundary.map(idx => project(vertices[idx])), `rgba(0,0,0,${shadeAlpha.toFixed(4)})`);
          }
        }
      }
      // Textured models use only the model's own texture markings (no overlay numbers or pips).
      return;
    }

    // Fallback for untextured triangulated GLB meshes: strokeless depth-sorted triangles.
    const visible = [];
    for (const tri of mesh.triangles) {
      const [i0, i1, i2] = tri.indices;
      const a = vertices[i0], b = vertices[i1], c = vertices[i2];
      const center = mul(add(add(a, b), c), 1 / 3);
      const crossN = cross(sub(b, a), sub(c, a));
      if (len(crossN) <= 1e-7) continue;
      const normal = unit(crossN);
      if (dot(sub([0, 0, 3.8], center), normal) <= 1e-4) continue;
      const p0 = project(a), p1 = project(b), p2 = project(c);
      if (screenDoubleArea(p0, p1, p2) > -0.25) continue;
      visible.push({ points: [p0, p1, p2], z: center[2], normal, color: tri.color });
    }
    visible.sort((a, b) => a.z - b.z);
    for (const tri of visible) {
      const strength = Math.min(1, 0.34 + Math.max(0, dot(tri.normal, light)) * 0.72);
      const base = tri.color || defaultBase;
      fillPolygon(tri.points, color(base.map(c => Math.round(c * strength)), opacity));
    }
    if (roll.sides === 6) {
      for (const face of mesh.faces) {
        if ((face.triCount ?? 0) > 6) continue;
        const fa = faceAnchor(mesh, face);
        const fc = transform(fa.center);
        const fn = unit(sub(transform(add(fa.center, mul(fa.normal, 0.2))), fc));
        if (fn[2] <= 0.18) continue;
        const fr = unit(sub(transform(add(fa.center, mul(fa.right, 0.2))), fc));
        const fu = unit(sub(transform(add(fa.center, mul(fa.up, 0.2))), fc));
        const mark = color([255, 238, 200], opacity * (0.6 + 0.4 * clamp(fn[2])));
        for (const [px, py] of pips(face.value)) {
          const mid = add(add(fc, mul(fr, px * 0.85)), mul(fu, py * 0.85));
          const perimeter = Array.from({ length: 12 }, (_, i) => {
            const angle = i * Math.PI / 6;
            return project(add(add(mid, mul(fr, Math.cos(angle) * 0.07)), mul(fu, Math.sin(angle) * 0.07)));
          });
          fillPolygon(perimeter, mark);
        }
      }
    }
    return;
  }

  // Legacy JSON polyhedron fallback (strokeless).
  const faces = mesh.faces.map(face => {
    const points = face.indices.map(index => vertices[index]);
    return { ...basis(points), points, value: face.value };
  }).filter(face => dot(sub([0, 0, 3.8], face.center), face.normal) > 1e-4)
    .sort((a, b) => a.center[2] - b.center[2]);

  for (const face of faces) {
    const strength = Math.min(1, 0.32 + Math.max(0, dot(face.normal, light)) * 0.73);
    const shade = factor => color(defaultBase.map(c => Math.round(c * strength * factor)), opacity);
    fillPolygon(face.points.map(project), shade(0.62));
    fillPolygon(face.points.map(p => project(add(face.center, mul(sub(p, face.center), 0.93)))), shade(1));
    if (roll.sides === 6) {
      const mark = color([255, 238, 200], opacity * (0.57 + 0.43 * clamp(face.normal[2])));
      for (const [px, py] of pips(face.value)) {
        const mid = add(add(face.center, mul(face.right, px)), mul(face.up, py));
        const perimeter = Array.from({ length: 12 }, (_, i) => {
          const angle = i * Math.PI / 6;
          return project(add(add(mid, mul(face.right, Math.cos(angle) * 0.075)), mul(face.up, Math.sin(angle) * 0.075)));
        });
        fillPolygon(perimeter, mark);
      }
    }
  }
}

function parseFaceMaterialName(name) {
  if (typeof name !== "string") return null;
  const match = /(?:^|[^a-z0-9])face[_-]?0*(\d+)(?:[^a-z0-9]|$)/i.exec(name.trim());
  if (!match) return null;
  const value = Number(match[1]);
  return Number.isInteger(value) && value >= 1 && value <= 100 ? value : null;
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
  const clamped = clamp(c);
  const srgb = clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * (clamped ** (1 / 2.4)) - 0.055;
  return Math.round(clamp(srgb) * 255);
}

function extractBoundaryLoop(tris) {
  const edgeMap = new Map();
  for (const tri of tris) {
    const [a, b, c] = tri.indices;
    for (const [u, v] of [[a, b], [b, c], [c, a]]) {
      const rev = `${v}:${u}`;
      if (edgeMap.has(rev)) {
        edgeMap.delete(rev);
      } else {
        edgeMap.set(`${u}:${v}`, [u, v]);
      }
    }
  }
  const nextByStart = new Map();
  for (const [u, v] of edgeMap.values()) {
    if (!nextByStart.has(u)) nextByStart.set(u, v);
  }
  const first = nextByStart.keys().next().value;
  if (first === undefined) return null;
  const loop = [first];
  let curr = nextByStart.get(first);
  const limit = nextByStart.size + 1;
  while (curr !== undefined && curr !== first && loop.length <= limit) {
    loop.push(curr);
    curr = nextByStart.get(curr);
  }
  return curr === first && loop.length >= 3 ? loop : null;
}

function subdivideTriangles(vertices, uvs, triangles, levels) {
  let tris = triangles;
  for (let lvl = 0; lvl < levels; lvl++) {
    const next = [];
    const midCache = new Map();
    const midpoint = (a, b) => {
      const key = a < b ? `${a}:${b}` : `${b}:${a}`;
      if (midCache.has(key)) return midCache.get(key);
      const idx = vertices.length;
      vertices.push(mul(add(vertices[a], vertices[b]), 0.5));
      uvs.push([(uvs[a][0] + uvs[b][0]) * 0.5, (uvs[a][1] + uvs[b][1]) * 0.5]);
      midCache.set(key, idx);
      return idx;
    };
    for (const tri of tris) {
      const [i0, i1, i2] = tri.indices;
      const m01 = midpoint(i0, i1);
      const m12 = midpoint(i1, i2);
      const m20 = midpoint(i2, i0);
      for (const indices of [[i0, m01, m20], [m01, i1, m12], [m20, m12, i2], [m01, m12, m20]]) {
        next.push({ indices, face: tri.face, color: tri.color, image: tri.image });
      }
    }
    tris = next;
  }
  return tris;
}

async function decodeEmbeddedImage(bytes, mimeType) {
  const blob = new Blob([bytes], { type: mimeType || "image/png" });
  if (typeof createImageBitmap === "function") {
    try {
      return await createImageBitmap(blob);
    } catch {
      // Fall back to HTMLImageElement below.
    }
  }
  return await new Promise(resolve => {
    const url = URL.createObjectURL(blob);
    const img = new Image();
    img.onload = () => { URL.revokeObjectURL(url); resolve(img); };
    img.onerror = () => { URL.revokeObjectURL(url); resolve(null); };
    img.src = url;
  });
}

async function parseGlbBuffer(buffer, expectedSides) {
  const buf = new Uint8Array(buffer);
  if (buf.byteLength < 20) return null;
  const dv = new DataView(buf.buffer, buf.byteOffset, buf.byteLength);
  if (dv.getUint32(0, true) !== GLB_MAGIC || dv.getUint32(4, true) !== 2) return null;
  let gltf = null, bin = null, offset = 12;
  while (offset + 8 <= buf.byteLength) {
    const chunkLen = dv.getUint32(offset, true);
    const chunkType = dv.getUint32(offset + 4, true);
    offset += 8;
    const chunk = buf.subarray(offset, offset + chunkLen);
    if (chunkType === CHUNK_JSON) gltf = JSON.parse(new TextDecoder("utf-8").decode(chunk));
    else if (chunkType === CHUNK_BIN) bin = chunk;
    offset += chunkLen;
  }
  if (!gltf || !bin) return null;

  const binView = new DataView(bin.buffer, bin.byteOffset, bin.byteLength);
  const readPos = idx => {
    const acc = gltf.accessors?.[idx];
    const bv = gltf.bufferViews?.[acc?.bufferView];
    if (!acc || !bv || acc.componentType !== 5126 || acc.type !== "VEC3") return [];
    const stride = bv.byteStride || 12;
    const base = (bv.byteOffset || 0) + (acc.byteOffset || 0);
    const out = [];
    for (let i = 0; i < acc.count; i++) {
      const o = base + i * stride;
      out.push([binView.getFloat32(o, true), binView.getFloat32(o + 4, true), binView.getFloat32(o + 8, true)]);
    }
    return out;
  };
  const readUv = idx => {
    const acc = gltf.accessors?.[idx];
    const bv = gltf.bufferViews?.[acc?.bufferView];
    if (!acc || !bv || acc.componentType !== 5126 || acc.type !== "VEC2") return null;
    const stride = bv.byteStride || 8;
    const base = (bv.byteOffset || 0) + (acc.byteOffset || 0);
    const out = [];
    for (let i = 0; i < acc.count; i++) {
      const o = base + i * stride;
      out.push([binView.getFloat32(o, true), binView.getFloat32(o + 4, true)]);
    }
    return out;
  };
  const readIdx = idx => {
    const acc = gltf.accessors?.[idx];
    const bv = gltf.bufferViews?.[acc?.bufferView];
    if (!acc || !bv) return [];
    const base = (bv.byteOffset || 0) + (acc.byteOffset || 0);
    const out = [];
    for (let i = 0; i < acc.count; i++) {
      if (acc.componentType === 5121) out.push(binView.getUint8(base + i));
      else if (acc.componentType === 5123) out.push(binView.getUint16(base + i * 2, true));
      else if (acc.componentType === 5125) out.push(binView.getUint32(base + i * 4, true));
    }
    return out;
  };

  // Extract embedded binary images referenced by glTF images[].bufferView.
  const rawImages = (gltf.images ?? []).map(img => {
    const bv = img?.bufferView !== undefined ? gltf.bufferViews?.[img.bufferView] : null;
    if (!bv) return null;
    const start = bv.byteOffset || 0;
    const bytes = bin.slice(start, start + bv.byteLength);
    return { bytes, mimeType: img.mimeType || "image/png" };
  });

  const nodes = gltf.nodes ?? [];
  const children = new Set(nodes.flatMap(n => n.children ?? []));
  const roots = gltf.scenes?.[gltf.scene ?? 0]?.nodes ?? nodes.map((_, i) => i).filter(i => !children.has(i));
  const vertices = [], uvs = [], rawTris = [];

  const visit = (nodeIdx, parent) => {
    const node = nodes[nodeIdx];
    if (!node) return;
    const world = mulMat4(parent, nodeLocalMat4(node));
    if (node.mesh !== undefined) {
      for (const prim of gltf.meshes?.[node.mesh]?.primitives ?? []) {
        if ((prim.mode !== undefined && prim.mode !== 4) || prim.attributes?.POSITION === undefined) continue;
        const pos = readPos(prim.attributes.POSITION);
        const primUvs = prim.attributes.TEXCOORD_0 !== undefined ? readUv(prim.attributes.TEXCOORD_0) : null;
        const validUvs = primUvs && primUvs.length === pos.length ? primUvs : null;
        const baseV = vertices.length;
        for (let k = 0; k < pos.length; k++) {
          vertices.push(transformPoint(world, pos[k]));
          uvs.push(validUvs ? validUvs[k] : [0, 0]);
        }
        const idx = prim.indices !== undefined ? readIdx(prim.indices) : pos.map((_, i) => i);
        const mat = prim.material !== undefined ? gltf.materials?.[prim.material] : null;
        const face = parseFaceMaterialName(mat?.name ?? "");
        const pbr = mat?.pbrMetallicRoughness;
        const bc = pbr?.baseColorFactor;
        const rgb = Array.isArray(bc) && bc.length >= 3 ? [linearToSrgbByte(bc[0]), linearToSrgbByte(bc[1]), linearToSrgbByte(bc[2])] : null;
        let image = null;
        if (validUvs && pbr?.baseColorTexture?.index !== undefined) {
          const src = gltf.textures?.[pbr.baseColorTexture.index]?.source;
          if (src !== undefined && rawImages[src]) image = src;
        }
        for (let i = 0; i + 2 < idx.length; i += 3) {
          rawTris.push({ i: [baseV + idx[i], baseV + idx[i + 1], baseV + idx[i + 2]], face, color: rgb, image });
        }
      }
    }
    for (const c of node.children ?? []) visit(c, world);
  };
  for (const r of roots) visit(r, [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
  if (!vertices.length || !rawTris.length) return null;

  const min = [Infinity, Infinity, Infinity], max = [-Infinity, -Infinity, -Infinity];
  for (const v of vertices) for (let k = 0; k < 3; k++) { min[k] = Math.min(min[k], v[k]); max[k] = Math.max(max[k], v[k]); }
  const mid = mul(add(min, max), 0.5);
  let maxD = 0;
  for (let i = 0; i < vertices.length; i++) { vertices[i] = sub(vertices[i], mid); maxD = Math.max(maxD, len(vertices[i])); }
  const scale = maxD > 1e-6 ? 0.96 / maxD : 1;
  for (let i = 0; i < vertices.length; i++) vertices[i] = mul(vertices[i], scale);

  const byFace = new Map();
  for (const tri of rawTris) if (tri.face !== null) {
    if (!byFace.has(tri.face)) byFace.set(tri.face, []);
    byFace.get(tri.face).push(tri);
  }
  const faces = [];
  for (let val = 1; val <= expectedSides; val++) {
    const tris = byFace.get(val);
    if (!tris?.length) return null;
    let nSum = [0, 0, 0], cSum = [0, 0, 0], tSum = [0, 0, 0], aSum = 0;
    for (const tri of tris) {
      const a = vertices[tri.i[0]], b = vertices[tri.i[1]], c = vertices[tri.i[2]];
      const n = cross(sub(b, a), sub(c, a));
      const area = len(n) * 0.5;
      if (area < 1e-8) continue;
      const m = mul(add(add(a, b), c), 1 / 3);
      nSum = add(nSum, dot(n, m) >= 0 ? n : mul(n, -1));
      cSum = add(cSum, mul(m, area));
      aSum += area;

      // Derive face right axis from UV +u tangent (dP/du) so textured faces land upright.
      const uv0 = uvs[tri.i[0]], uv1 = uvs[tri.i[1]], uv2 = uvs[tri.i[2]];
      const du1 = uv1[0] - uv0[0], dv1 = uv1[1] - uv0[1];
      const du2 = uv2[0] - uv0[0], dv2 = uv2[1] - uv0[1];
      const det = du1 * dv2 - du2 * dv1;
      if (Math.abs(det) > 1e-8) {
        const dp1 = sub(b, a), dp2 = sub(c, a);
        const tangent = mul(sub(mul(dp1, dv2), mul(dp2, dv1)), 1 / det);
        if (len(tangent) > 1e-8) tSum = add(tSum, mul(unit(tangent), area));
      }
    }
    const normal = unit(nSum);
    const center = aSum > 1e-8 ? mul(cSum, 1 / aSum) : [0, 0, 0];
    const tProj = sub(tSum, mul(normal, dot(tSum, normal)));
    let right;
    if (len(tProj) > 1e-5) {
      right = unit(tProj);
    } else {
      const ref = Math.abs(normal[1]) < 0.9 ? [0, 1, 0] : [0, 0, 1];
      right = unit(cross(ref, normal));
    }
    const up = unit(cross(normal, right));
    faces.push({ value: val, center, normal, right, up, triCount: tris.length });
  }

  const orientedTris = rawTris.map(tri => {
    const [i0, i1, i2] = tri.i;
    const a = vertices[i0], b = vertices[i1], c = vertices[i2];
    const n = cross(sub(b, a), sub(c, a));
    const m = mul(add(add(a, b), c), 1 / 3);
    return { indices: dot(n, m) >= 0 ? [i0, i1, i2] : [i0, i2, i1], face: tri.face, color: tri.color, image: tri.image };
  });

  const hasAnyTexture = orientedTris.some(t => t.image !== null);
  const textures = hasAnyTexture
    ? await Promise.all(rawImages.map(img => (img ? decodeEmbeddedImage(img.bytes, img.mimeType) : null)))
    : [];

  let triangles = orientedTris;
  let faceGroups = null;
  if (hasAnyTexture && textures.some(Boolean)) {
    const levels = orientedTris.length <= 16 ? 2 : orientedTris.length <= 40 ? 1 : 0;
    const origByFace = new Map();
    for (const tri of orientedTris) {
      const key = tri.face ?? -1;
      if (!origByFace.has(key)) origByFace.set(key, []);
      origByFace.get(key).push(tri);
    }
    triangles = levels > 0 ? subdivideTriangles(vertices, uvs, orientedTris, levels) : orientedTris;
    const subByFace = new Map();
    for (const tri of triangles) {
      const key = tri.face ?? -1;
      if (!subByFace.has(key)) subByFace.set(key, []);
      subByFace.get(key).push(tri);
    }
    faceGroups = faces.map(face => ({
      value: face.value,
      center: face.center,
      normal: face.normal,
      boundary: extractBoundaryLoop(origByFace.get(face.value) ?? []),
      subTriangles: subByFace.get(face.value) ?? [],
    }));
  }

  return { vertices, uvs, faces, triangles, textures, faceGroups };
}

async function tryLoadGlb(urls, sides) {
  for (const url of urls) {
    try {
      const res = await fetch(url);
      if (!res.ok) continue;
      const parsed = await parseGlbBuffer(await res.arrayBuffer(), sides);
      if (parsed) return parsed;
    } catch {
      // Optional GLB override; fall back cleanly.
    }
  }
  return null;
}

async function loadMeshes() {
  const [fallback, d6Glb, d20Glb] = await Promise.all([
    fetch("/dice-models.json").then(r => (r.ok ? r.json() : null)).catch(() => null),
    tryLoadGlb(["/d6.glb", "/models/d6.glb"], 6),
    tryLoadGlb(["/d20.glb", "/models/d20.glb"], 20),
  ]);
  const result = {
    d6: d6Glb || fallback?.d6,
    d20: d20Glb || fallback?.d20,
  };
  if (!result.d6 && !result.d20) throw new Error("Dice models unavailable");
  return result;
}

function animate(now) {
  if (!active || !drawing) return;
  const { roll, mesh, start } = active;
  const elapsed = now - start;
  if (elapsed < 0) {
    frame = requestAnimationFrame(animate);
    return;
  }
  if (elapsed >= roll.durationMs) {
    canvas.hidden = true;
    return;
  }
  canvas.hidden = false;
  const { clientWidth: width, clientHeight: height } = canvas;
  const ratio = Math.min(devicePixelRatio || 1, 2);
  if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(height * ratio);
  }
  drawing.setTransform(ratio, 0, 0, ratio, 0, 0);
  drawing.clearRect(0, 0, width, height);
  const fade = smooth(elapsed / 180) * smooth((roll.durationMs - elapsed) / 450);
  const t = clamp(elapsed / roll.settleMs);
  const flight = 1 - (1 - clamp(t / 0.78)) ** 3;
  const radius = Math.min(height * 0.23, width * 0.34, 235);
  const x = width / 2, y = height / 2 - 21;
  drawing.fillStyle = color([0, 0, 0], fade * 0.85);
  drawing.fillRect(0, 0, width, height);
  const glow = drawing.createRadialGradient(x, y, 0, x, y, radius * 1.95);
  const glowColor = roll.sides === 6 ? [38, 94, 112] : [180, 102, 24];
  glow.addColorStop(0, color(glowColor, fade * 0.56));
  glow.addColorStop(1, color(glowColor, 0));
  drawing.fillStyle = glow;
  drawing.fillRect(0, 0, width, height);
  const bottom = height + radius * 1.6;
  drawDie(
    mesh,
    roll,
    x + (1 - flight) * radius * 0.45,
    bottom + (y - bottom) * flight - Math.sin(Math.PI * flight) * radius * 0.38,
    radius,
    t,
    fade
  );
  label(`${roll.player}  ·  D${roll.sides}`, x, y + radius * 1.19 + 9, 17, color([230, 223, 207], fade * flight));
  const result = smooth((elapsed - roll.settleMs) / 180);
  if (result > 0) label(roll.value, x, y + radius * 1.39 + 20, 39, color([255, 218, 148], fade * result), true);
  frame = requestAnimationFrame(animate);
}

export function syncDiceRoll(roll) {
  if (!roll || ![6, 20].includes(roll.sides) || roll.value < 1 || roll.value > roll.sides) {
    cancelAnimationFrame(frame);
    active = undefined;
    canvas.hidden = true;
    return;
  }
  if (active?.roll.id === roll.id) return;
  cancelAnimationFrame(frame);
  canvas.hidden = true;
  const event = { roll, start: performance.now() - roll.elapsedMs };
  active = event;
  meshes ??= loadMeshes().catch(() => { meshes = undefined; });
  meshes.then(data => {
    if (active !== event || !data) return;
    event.mesh = data["d" + roll.sides];
    if (event.mesh) frame = requestAnimationFrame(animate);
  });
}

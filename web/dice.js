const canvas = document.getElementById("diceOverlay");
const drawing = canvas.getContext("2d");
let meshes;
let active;
let frame = 0;

const add = (a, b) => a.map((x, i) => x + b[i]);
const mul = (a, scalar) => a.map(x => x * scalar);
const sub = (a, b) => add(a, mul(b, -1));
const dot = (a, b) => a.reduce((sum, x, i) => sum + x * b[i], 0);
const unit = a => mul(a, 1 / Math.hypot(...a));
const cross = ([x, y, z], [a, b, c]) => [y * c - z * b, z * a - x * c, x * b - y * a];
const clamp = value => Math.max(0, Math.min(1, value));
const smooth = value => { const t = clamp(value); return t * t * (3 - 2 * t); };
const color = (rgb, alpha = 1) => `rgba(${rgb.join(",")},${alpha})`;

function basis(points) {
  const center = mul(points.reduce((sum, p) => add(sum, p), [0, 0, 0]), 1 / points.length);
  const normal = unit(cross(sub(points[1], points[0]), sub(points[2], points[0])));
  const top = points.length === 4 ? mul(add(points[0], points[1]), 0.5) : points[0];
  const right = unit(cross(sub(top, center), normal));
  return { center, normal, right, up: cross(normal, right) };
}

function rotate([x, y, z], rx, ry, rz) {
  const sx = Math.sin(rx), cx = Math.cos(rx), sy = Math.sin(ry), cy = Math.cos(ry);
  const sz = Math.sin(rz), cz = Math.cos(rz);
  const y1 = cx * y - sx * z, z1 = sx * y + cx * z;
  const x2 = cy * x + sy * z1;
  return [cz * x2 - sz * y1, sz * x2 + cz * y1, cy * z1 - sy * x];
}

function polygon(points, fill, outline) {
  drawing.beginPath();
  points.forEach(([x, y], i) => i ? drawing.lineTo(x, y) : drawing.moveTo(x, y));
  drawing.closePath();
  drawing.fillStyle = fill;
  drawing.fill();
  if (outline) {
    drawing.strokeStyle = outline;
    drawing.lineWidth = 1.2;
    drawing.stroke();
  }
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
  const face = mesh.faces.find(face => face.value === roll.value);
  const { normal, right, up } = basis(face.indices.map(index => mesh.vertices[index]));
  const spin = (1 - t) ** 1.75;
  const direction = roll.value % 2 ? 1 : -1;
  const vertices = mesh.vertices.map(v => rotate(
    [dot(v, right), dot(v, up), dot(v, normal)],
    0.18 + spin * Math.PI * 7.2, -0.25 + direction * spin * Math.PI * 8.4, 0.09 + spin * Math.PI * 3.6,
  ));
  const project = ([a, b, z]) => [x + a * radius * 3.8 / (3.8 - z), y - b * radius * 3.8 / (3.8 - z)];
  const faces = mesh.faces.map(face => {
    const points = face.indices.map(index => vertices[index]);
    return { ...basis(points), points, value: face.value };
  }).filter(face => dot(sub([0, 0, 3.8], face.center), face.normal) > 0)
    .sort((a, b) => a.center[2] - b.center[2]);
  const base = roll.sides === 6 ? [43, 120, 139] : [102, 66, 142];
  const light = unit([-0.44, 0.62, 0.80]);
  for (const face of faces) {
    const front = face.value === roll.value && t > 0.84;
    const strength = Math.min(1, 0.32 + Math.max(0, dot(face.normal, light)) * 0.73);
    const edge = front ? color([246, 198, 111], 0.96 * opacity) : color([173, 166, 183], 0.57 * opacity);
    const shade = factor => color(base.map(c => Math.round(c * strength * factor)), opacity);
    polygon(face.points.map(project), shade(0.54), edge);
    polygon(face.points.map(p => project(add(face.center, mul(sub(p, face.center), 0.91)))), shade(1));
    const mark = color([255, 235, 194], opacity * (0.57 + 0.43 * clamp(face.normal[2])));
    if (roll.sides === 6) {
      for (const [px, py] of pips(face.value)) {
        const mid = add(add(face.center, mul(face.right, px)), mul(face.up, py));
        const perimeter = Array.from({ length: 12 }, (_, i) => {
          const angle = i * Math.PI / 6;
          return project(add(add(mid, mul(face.right, Math.cos(angle) * 0.075)), mul(face.up, Math.sin(angle) * 0.075)));
        });
        polygon(perimeter, mark);
      }
    } else if (face.normal[2] > 0.15) {
      const [cx, cy] = project(face.center);
      const text = [6, 9].includes(face.value) ? face.value + "." : face.value;
      label(text, cx, cy, radius * 0.3 * (0.56 + face.normal[2] * 0.44), mark, true);
    }
  }
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
  const flight = 1 - (1 - clamp(t / 0.82)) ** 3;
  const radius = Math.min(height * 0.23, width * 0.34, 235);
  const x = width / 2, y = height / 2 - 21;
  drawing.fillStyle = color([0, 0, 0], fade * 0.85);
  drawing.fillRect(0, 0, width, height);
  const glow = drawing.createRadialGradient(x, y, 0, x, y, radius * 1.95);
  const glowColor = roll.sides === 6 ? [38, 94, 112] : [91, 57, 119];
  glow.addColorStop(0, color(glowColor, fade * 0.63));
  glow.addColorStop(1, color(glowColor, 0));
  drawing.fillStyle = glow;
  drawing.fillRect(0, 0, width, height);
  const bottom = height + radius * 1.6;
  drawDie(mesh, roll, x + (1 - flight) * radius * 0.52,
    bottom + (y - bottom) * flight - Math.sin(Math.PI * flight) * radius * 0.44, radius, t, fade);
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
  meshes ??= fetch("/dice-models.json").then(response => {
    if (!response.ok) throw Error("Dice models unavailable");
    return response.json();
  }).catch(() => { meshes = undefined; });
  meshes.then(data => {
    if (active !== event || !data) return;
    event.mesh = data["d" + roll.sides];
    frame = requestAnimationFrame(animate);
  });
}

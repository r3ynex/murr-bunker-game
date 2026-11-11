// Small, vetted room media bundles: pack.json + optional images under assets/.
// Untrusted zip entries are checked in the CENTRAL DIRECTORY before decompression.
import { unzipSync, strFromU8 } from "fflate";
import { normalizePack } from "./content.js";

export const MAX_ZIP_BYTES = 2_000_000;
const MAX_UNPACKED_BYTES = 6_000_000;
const MAX_FILES = 48;
const MAGIC_CENTRAL = 0x02014b50;
const MAGIC_END = 0x06054b50;
const td = new TextDecoder("utf-8", { fatal: true });

function inspectZip(bytes) {
  if (!(bytes instanceof Uint8Array) || bytes.length < 22 ||
      bytes.length > MAX_ZIP_BYTES) throw Error("ZIP должен быть меньше 2 МБ");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let eocd = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 65557); i--) {
    if (view.getUint32(i, true) === MAGIC_END) { eocd = i; break; }
  }
  if (eocd < 0) throw Error("Не найден список файлов ZIP");
  const count = view.getUint16(eocd + 10, true);
  let offset = view.getUint32(eocd + 16, true);
  if (view.getUint16(eocd + 4, true) !== 0 ||
      view.getUint16(eocd + 6, true) !== 0 ||
      count < 1 || count > MAX_FILES ||
      view.getUint16(eocd + 8, true) !== count ||
      offset >= eocd) throw Error("Неподдерживаемый архив ZIP");
  let uncompressed = 0;
  const names = new Set();
  for (let index = 0; index < count; index++) {
    if (offset + 46 > eocd || view.getUint32(offset, true) !== MAGIC_CENTRAL) {
      throw Error("Повреждён каталог ZIP");
    }
    const method = view.getUint16(offset + 10, true);
    const compressed = view.getUint32(offset + 20, true);
    const rawSize = view.getUint32(offset + 24, true);
    const nameLength = view.getUint16(offset + 28, true);
    const extraLength = view.getUint16(offset + 30, true);
    const commentLength = view.getUint16(offset + 32, true);
    const flags = view.getUint16(offset + 8, true);
    if (offset + 46 + nameLength + extraLength + commentLength > eocd ||
        ![0, 8].includes(method) || flags & 1 ||
        compressed === 0xffffffff || rawSize === 0xffffffff) {
      throw Error("ZIP содержит неподдерживаемые или зашифрованные файлы");
    }
    const name = td.decode(bytes.subarray(offset + 46, offset + 46 + nameLength));
    const allowed = name === "pack.json" ||
      /^assets\/[a-zA-Z0-9_./-]+\.(png|jpg|jpeg|webp)$/i.test(name);
    if (!allowed || name.includes("..") || name.includes("//") ||
        name.startsWith("/") || names.has(name)) {
      throw Error("ZIP: разрешены только pack.json и изображения assets/*.png|jpg|webp");
    }
    names.add(name);
    uncompressed += rawSize;
    if (uncompressed > MAX_UNPACKED_BYTES) throw Error("ZIP слишком велик после распаковки");
    // No executable/symlink entries or archives with unusual traversal names.
    const unixMode = (view.getUint32(offset + 38, true) >>> 16) & 0xffff;
    if ((unixMode & 0xf000) === 0xa000) throw Error("ZIP не должен содержать ссылки");
    offset += 46 + nameLength + extraLength + commentLength;
  }
  if (!names.has("pack.json")) throw Error("В ZIP нет pack.json");
  return names;
}

export function readBundle(input) {
  const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
  const names = inspectZip(bytes);
  const extracted = unzipSync(bytes);
  const textBytes = extracted["pack.json"];
  if (!textBytes || textBytes.length > 500000) throw Error("pack.json слишком велик");
  const pack = normalizePack(JSON.parse(td.decode(textBytes)));
  const images = [];
  for (const [name, value] of Object.entries(extracted)) {
    if (!names.has(name)) throw Error("Неподдерживаемый файл в ZIP");
    if (name !== "pack.json") {
      const png = name.toLowerCase().endsWith(".png");
      const jpg = /\.jpe?g$/i.test(name);
      const webp = name.toLowerCase().endsWith(".webp");
      const signatureOk = (png && value.length >= 8 && value[0] === 137 && value[1] === 80) ||
        (jpg && value.length >= 3 && value[0] === 255 && value[1] === 216 && value[2] === 255) ||
        (webp && value.length >= 12 && td.decode(value.subarray(0, 4)) === "RIFF" &&
         td.decode(value.subarray(8, 12)) === "WEBP");
      if (!signatureOk) throw Error("Неверный формат изображения: " + name);
      images.push(name);
    }
  }
  const background = images.find(x => /^assets\/catastrophe\.(png|jpe?g|webp)$/i.test(x)) || null;
  return { pack, background, images };
}
export async function sha256Hex(bytes) {
  const hash = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
  return Array.from(hash, x => x.toString(16).padStart(2, "0")).join("");
}

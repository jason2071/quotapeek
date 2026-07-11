// Generate a macOS *template* tray icon: transparent background + opaque
// gauge-ring shape. macOS renders template images using the alpha channel and
// recolors them to match the menubar (light/dark), so only alpha matters.
// Output: src-tauri/icons/tray-template.png

import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const SIZE = 64;
const cx = SIZE / 2;
const cy = SIZE / 2;
const rOuter = SIZE * 0.42;
const rInner = SIZE * 0.26;

const stride = SIZE * 4;
const raw = Buffer.alloc(SIZE * (stride + 1));

for (let y = 0; y < SIZE; y++) {
  const rowStart = y * (stride + 1);
  raw[rowStart] = 0; // filter: none
  for (let x = 0; x < SIZE; x++) {
    const dx = x - cx;
    const dy = y - cy;
    const dist = Math.sqrt(dx * dx + dy * dy);
    const angle = Math.atan2(dy, dx);
    const inGap = angle > Math.PI * 0.25 && angle < Math.PI * 0.75;
    const inRing = dist >= rInner && dist <= rOuter && !inGap;
    const p = rowStart + 1 + x * 4;
    // White, opaque only on the ring (color ignored by macOS template rendering).
    raw[p] = 255;
    raw[p + 1] = 255;
    raw[p + 2] = 255;
    raw[p + 3] = inRing ? 255 : 0;
  }
}

const CRC_TABLE = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return t;
})();
function crc32(buf) {
  let c = 0xffffffff;
  for (let i = 0; i < buf.length; i++) c = CRC_TABLE[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
  return (c ^ 0xffffffff) >>> 0;
}
function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const body = Buffer.concat([Buffer.from(type, "ascii"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8;
ihdr[9] = 6;
const idat = deflateSync(raw, { level: 9 });
const png = Buffer.concat([sig, chunk("IHDR", ihdr), chunk("IDAT", idat), chunk("IEND", Buffer.alloc(0))]);

const out = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "icons", "tray-template.png");
writeFileSync(out, png);
console.log(`Wrote ${out} (${png.length} bytes)`);

// Generate a 1024x1024 RGBA PNG app icon with zero external dependencies.
// Design: Claude-coral background with a white usage-gauge ring.
// Output: scripts/app-icon.png  → feed to `npx tauri icon`.

import { deflateSync } from "node:zlib";
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const SIZE = 1024;
const BG = [217, 119, 87]; // #D97757 Claude coral
const FG = [255, 255, 255];

const cx = SIZE / 2;
const cy = SIZE / 2;
const rOuter = SIZE * 0.40;
const rInner = SIZE * 0.28;

// Raw image: each row is 1 filter byte (0) + SIZE*4 RGBA bytes.
const stride = SIZE * 4;
const raw = Buffer.alloc(SIZE * (stride + 1));

for (let y = 0; y < SIZE; y++) {
  const rowStart = y * (stride + 1);
  raw[rowStart] = 0; // filter: none
  for (let x = 0; x < SIZE; x++) {
    const dx = x - cx;
    const dy = y - cy;
    const dist = Math.sqrt(dx * dx + dy * dy);
    // Ring with a gauge gap at the bottom (~90° opening).
    const angle = Math.atan2(dy, dx); // -PI..PI, 0 = +x, PI/2 = down
    const inGap = angle > Math.PI * 0.25 && angle < Math.PI * 0.75;
    const inRing = dist >= rInner && dist <= rOuter && !inGap;
    const [r, g, b] = inRing ? FG : BG;
    const p = rowStart + 1 + x * 4;
    raw[p] = r;
    raw[p + 1] = g;
    raw[p + 2] = b;
    raw[p + 3] = 255;
  }
}

// --- Minimal PNG encoder ---
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
  const typeBuf = Buffer.from(type, "ascii");
  const body = Buffer.concat([typeBuf, data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body), 0);
  return Buffer.concat([len, body, crc]);
}

const sig = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(SIZE, 0);
ihdr.writeUInt32BE(SIZE, 4);
ihdr[8] = 8; // bit depth
ihdr[9] = 6; // color type: RGBA
ihdr[10] = 0; // compression
ihdr[11] = 0; // filter
ihdr[12] = 0; // interlace

const idat = deflateSync(raw, { level: 9 });
const png = Buffer.concat([
  sig,
  chunk("IHDR", ihdr),
  chunk("IDAT", idat),
  chunk("IEND", Buffer.alloc(0)),
]);

const outPath = join(dirname(fileURLToPath(import.meta.url)), "app-icon.png");
writeFileSync(outPath, png);
console.log(`Wrote ${outPath} (${png.length} bytes)`);

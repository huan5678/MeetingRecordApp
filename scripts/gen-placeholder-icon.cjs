// Generate a 1024x1024 placeholder PNG (no external deps) as a source for `tauri icon`.
// Solid indigo background with a lighter rounded square — enough for a valid icon set.
const zlib = require('zlib');
const fs = require('fs');

const W = 1024, H = 1024;

// crc32 (PNG)
const CRC = (() => {
  const t = new Uint32Array(256);
  for (let n = 0; n < 256; n++) {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    t[n] = c >>> 0;
  }
  return (buf) => {
    let c = 0xffffffff;
    for (let i = 0; i < buf.length; i++) c = t[(c ^ buf[i]) & 0xff] ^ (c >>> 8);
    return (c ^ 0xffffffff) >>> 0;
  };
})();

function chunk(type, data) {
  const len = Buffer.alloc(4);
  len.writeUInt32BE(data.length, 0);
  const typeBuf = Buffer.from(type, 'ascii');
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(CRC(Buffer.concat([typeBuf, data])), 0);
  return Buffer.concat([len, typeBuf, data, crc]);
}

const bg = [79, 70, 229];   // indigo-600
const fg = [129, 140, 248]; // indigo-400
const raw = Buffer.alloc(H * (1 + W * 4));
let p = 0;
const m = 220, r = 120; // margin + corner radius for a rounded square
for (let y = 0; y < H; y++) {
  raw[p++] = 0; // filter: none
  for (let x = 0; x < W; x++) {
    let inside = x >= m && x < W - m && y >= m && y < H - m;
    if (inside) {
      // knock out the rounded corners
      const cx = x < m + r ? m + r : x > W - m - r ? W - m - r : x;
      const cy = y < m + r ? m + r : y > H - m - r ? H - m - r : y;
      if ((x - cx) ** 2 + (y - cy) ** 2 > r * r) inside = false;
    }
    const c = inside ? fg : bg;
    raw[p++] = c[0]; raw[p++] = c[1]; raw[p++] = c[2]; raw[p++] = 255;
  }
}

const sig = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]);
const ihdr = Buffer.alloc(13);
ihdr.writeUInt32BE(W, 0);
ihdr.writeUInt32BE(H, 4);
ihdr[8] = 8;  // bit depth
ihdr[9] = 6;  // color type RGBA
const png = Buffer.concat([
  sig,
  chunk('IHDR', ihdr),
  chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
  chunk('IEND', Buffer.alloc(0)),
]);

const out = process.argv[2] || '/tmp/mra-icon-src.png';
fs.writeFileSync(out, png);
console.log('wrote', out, png.length, 'bytes');

// Fetch the sherpa-onnx diarization models the `diarize` feature needs and drop
// them into src-tauri/resources/models/ so the diarize build can bundle them.
//
// Models (per docs/superpowers/specs/2026-07-04-speaker-attribution-core-design.md):
//   - segmentation: pyannote-segmentation-3.0  -> sherpa-segmentation.onnx
//   - embedding:    3D-Speaker CAM++ zh-cn      -> sherpa-embedding.onnx
//
// Idempotent: skips a file that already exists. Run before a diarize build:
//   npm run models:diarize
//
// Requires Node 18+ (global fetch) and a system `tar` that reads .tar.bz2
// (macOS tar and Windows 10+ bsdtar both do).

import { execSync } from "node:child_process";
import { existsSync, mkdirSync, readdirSync, rmSync, statSync, writeFileSync, copyFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const REL = "https://github.com/k2-fsa/sherpa-onnx/releases/download";
const SEG_TARBALL = `${REL}/speaker-segmentation-models/sherpa-onnx-pyannote-segmentation-3-0.tar.bz2`;
const EMB_ONNX = `${REL}/speaker-recongition-models/3dspeaker_speech_campplus_sv_zh-cn_16k-common.onnx`;

const outDir = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "resources", "models");
const segOut = join(outDir, "sherpa-segmentation.onnx");
const embOut = join(outDir, "sherpa-embedding.onnx");

async function download(url, dest) {
  process.stdout.write(`↓ ${url}\n`);
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url} → ${res.status} ${res.statusText}`);
  writeFileSync(dest, Buffer.from(await res.arrayBuffer()));
}

/** Recursively find the best .onnx in a dir (prefer full-precision model.onnx). */
function findOnnx(dir) {
  const hits = [];
  const walk = (d) => {
    for (const name of readdirSync(d)) {
      const p = join(d, name);
      if (statSync(p).isDirectory()) walk(p);
      else if (name.endsWith(".onnx")) hits.push(p);
    }
  };
  walk(dir);
  return hits.find((p) => p.endsWith("model.onnx")) ?? hits[0];
}

async function main() {
  mkdirSync(outDir, { recursive: true });

  if (existsSync(embOut)) console.log(`✓ ${embOut} (exists)`);
  else await download(EMB_ONNX, embOut);

  if (existsSync(segOut)) {
    console.log(`✓ ${segOut} (exists)`);
  } else {
    const tar = join(outDir, "_seg.tar.bz2");
    const tmp = join(outDir, "_seg");
    await download(SEG_TARBALL, tar);
    mkdirSync(tmp, { recursive: true });
    execSync(`tar -xf "${tar}" -C "${tmp}"`, { stdio: "inherit" });
    const model = findOnnx(tmp);
    if (!model) throw new Error("no .onnx found in segmentation tarball");
    copyFileSync(model, segOut);
    rmSync(tar, { force: true });
    rmSync(tmp, { recursive: true, force: true });
    console.log(`✓ ${segOut}`);
  }
  console.log("diarization models ready.");
}

main().catch((e) => {
  console.error(`fetch-diarize-models failed: ${e.message}`);
  process.exit(1);
});

// Reapply after reinstalling pi-keep-going@1.1.0; package files are ignored by Git.
import { readFileSync, writeFileSync } from 'node:fs';

const path = process.argv[2] ?? new URL('../npm/node_modules/pi-keep-going/src/limits/detect.ts', import.meta.url);
const original = '/hit your ChatGPT usage limit/i.test(message) ||';
const patched = '/hit your ChatGPT usage limit|the usage limit has been reached/i.test(message) ||';
const source = readFileSync(path, 'utf8');
if (!source.includes(patched)) {
  if (!source.includes(original)) throw new Error('pi-keep-going detector changed; review before patching');
  writeFileSync(path, source.replace(original, patched));
}

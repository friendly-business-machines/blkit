import { test } from 'node:test';
import { strict as assert } from 'node:assert';
import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const globalRoot = execFileSync('npm', ['root', '-g'], { encoding: 'utf8' }).trim();
const { transformSync } = require(`${globalRoot}/@earendil-works/pi-coding-agent/node_modules/esbuild`);
const source = readFileSync(new URL('../npm/node_modules/pi-keep-going/src/limits/detect.ts', import.meta.url), 'utf8');
const compiled = transformSync(source, { loader: 'ts', format: 'esm' }).code;
const { detectUsageLimit } = await import(`data:text/javascript;base64,${Buffer.from(compiled).toString('base64')}`);

test('Codex exhausted usage window triggers reset lookup', () => {
  assert.deepEqual(detectUsageLimit({
    provider: 'openai-codex',
    stopReason: 'error',
    errorMessage: 'Codex error: The usage limit has been reached',
    cached429: null,
    now: 0,
  }), { provider: 'codex', reset: null });
});

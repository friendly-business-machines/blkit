import { test } from 'node:test';
import { strict as assert } from 'node:assert';
import { detectUsageLimit } from '../npm/node_modules/pi-keep-going/src/limits/detect.ts';

test('Codex exhausted usage window triggers reset lookup', () => {
  assert.deepEqual(detectUsageLimit({
    provider: 'openai-codex',
    stopReason: 'error',
    errorMessage: 'Codex error: The usage limit has been reached',
    cached429: null,
    now: 0,
  }), { provider: 'codex', reset: null });
});

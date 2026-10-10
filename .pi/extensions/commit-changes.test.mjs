import assert from 'node:assert/strict';
import { test } from 'node:test';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync, readFileSync, readdirSync, mkdirSync, symlinkSync, chmodSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createJiti } from '../npm/node_modules/jiti/lib/jiti.cjs';
const localPi = fileURLToPath(new URL('../npm/node_modules/@earendil-works/pi-coding-agent/', import.meta.url));
const piRoot = existsSync(join(localPi, 'dist/core/extensions/loader.js'))
  ? localPi : join(execFileSync('npm', ['root', '-g'], {encoding:'utf8'}).trim(), '@earendil-works/pi-coding-agent');
const { discoverAndLoadExtensions } = await import(pathToFileURL(join(piRoot, 'dist/core/extensions/loader.js')).href);

// Pi should discover the extension entry point, not its supporting Git module.
test('project extension discovery loads only the commit-changes factory', async () => {
  const root = fileURLToPath(new URL('../..', import.meta.url));
  const { extensions, errors } = await discoverAndLoadExtensions([], root, join(root, '.pi', 'nonexistent-agent'));
  assert.deepEqual(errors, []);
  assert.deepEqual(extensions.map(ext => ext.path), [join(root, '.pi/extensions/commit-changes.ts')]);
});

const gitModule = createJiti(import.meta.url)('./commit-changes/git.ts');
const { inventory, validateGroups, stageGroup, skipGroup } = gitModule;
const git = (root, ...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' });
const repo = (fn) => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-'));
  try {
    git(root, 'init', '-q');
    git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'base.txt'), 'base\n'); git(root, 'add', 'base.txt'); git(root, 'commit', '-qm', 'init');
    fn(root);
  } finally { rmSync(root, { recursive: true, force: true }); }
};

const remoteRepo = fn => repo(root => {
  const bare = join(root, '..', `bare-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  const peer = join(root, '..', `peer-${Date.now()}-${Math.random().toString(16).slice(2)}`);
  try {
    git(root, 'branch', '-M', 'main'); git(root, 'init', '--bare', '-q', bare);
    git(root, 'remote', 'add', 'origin', bare); git(root, 'push', '-qu', 'origin', 'main');
    git(root, 'clone', '-q', '--branch', 'main', bare, peer);
    git(peer, 'config', 'user.name', 'Peer'); git(peer, 'config', 'user.email', 'peer@example.org');
    fn(root, peer, bare);
  } finally { rmSync(bare, {recursive:true,force:true}); rmSync(peer, {recursive:true,force:true}); }
});

test('push never changes a ref other than the approved upstream', () => remoteRepo((root, peer) => {
  writeFileSync(join(root, 'base.txt'), 'local\n'); git(root, 'add', 'base.txt'); git(root, 'commit', '-qm', 'local');
  const hash = git(root, 'rev-parse', 'HEAD').trim();
  const plan = gitModule.pushPlan(root, [hash]);
  assert.equal(plan.target, 'refs/heads/main');
  git(root, 'config', 'branch.main.remote', 'other');
  assert.equal(gitModule.pushApproved(root, plan).route, 'blocked');
  git(root, 'config', 'branch.main.remote', 'origin');
  assert.equal(gitModule.pushApproved(root, plan).route, 'confirmed');
  git(peer, 'fetch', '-q', 'origin');
  assert.equal(git(peer, 'rev-parse', 'origin/main').trim(), git(root, 'rev-parse', 'HEAD').trim());
}));

test('push approval is invalidated when the resolved destination changes', () => remoteRepo((root, peer, bare) => {
  writeFileSync(join(root, 'base.txt'), 'local\n'); git(root, 'commit', '-qam', 'local');
  const plan = gitModule.pushPlan(root, [git(root, 'rev-parse', 'HEAD').trim()]);
  const alternate = join(root, '..', `other-bare-${Date.now()}`);
  try {
    git(root, 'init', '--bare', '-q', alternate);
    git(root, 'config', 'remote.origin.pushurl', alternate);
    assert.equal(gitModule.pushApproved(root, plan).route, 'blocked');
    git(root, 'config', '--unset', 'remote.origin.pushurl');
    git(root, 'config', 'remote.origin.url', alternate);
    assert.equal(gitModule.pushApproved(root, plan).route, 'blocked');
    git(root, 'config', 'remote.origin.url', bare);
    assert.equal(gitModule.pushApproved(root, plan).route, 'confirmed');
  } finally { rmSync(alternate, {recursive:true,force:true}); }
}));

test('rejected push offers fetch only with clean worktree and merge approval', () => remoteRepo((root, peer) => {
  writeFileSync(join(root, 'local.txt'), 'ours\n'); git(root, 'add', 'local.txt'); git(root, 'commit', '-qm', 'local');
  writeFileSync(join(peer, 'peer.txt'), 'theirs\n'); git(peer, 'add', 'peer.txt'); git(peer, 'commit', '-qm', 'peer'); git(peer, 'push', '-q', 'origin', 'main');
  const plan = gitModule.pushPlan(root, [git(root, 'rev-parse', 'HEAD').trim()]);
  assert.equal(gitModule.pushApproved(root, plan).route, 'reconcile');
  writeFileSync(join(root, 'scratch'), 'untouched\n');
  assert.equal(gitModule.reconcile(root, plan).route, 'blocked');
  rmSync(join(root, 'scratch'));
  assert.equal(gitModule.reconcile(root, plan).route, 'mergeApproval');
  const receipt = gitModule.prepareMerge(root, git(root, 'diff', '--cached', '--binary'));
  assert.equal(receipt.message, 'Merge upstream into current branch');
  assert.match(receipt.staged, /peer.txt/);
  assert.equal(gitModule.commitMerge(root, receipt), git(root, 'rev-parse', 'HEAD').trim());
  assert.equal(gitModule.pushApproved(root, {...plan, head: git(root,'rev-parse','HEAD').trim()}).route, 'confirmed');
}));

test('merge commit rejects a hook-rewritten approved message', () => remoteRepo((root, peer) => {
  writeFileSync(join(root, 'local.txt'), 'ours\n'); git(root, 'add', 'local.txt'); git(root, 'commit', '-qm', 'local');
  writeFileSync(join(peer, 'peer.txt'), 'theirs\n'); git(peer, 'add', 'peer.txt'); git(peer, 'commit', '-qm', 'peer'); git(peer, 'push', '-q', 'origin', 'main');
  const plan = gitModule.pushPlan(root, [git(root, 'rev-parse', 'HEAD').trim()]);
  assert.equal(gitModule.pushApproved(root, plan).route, 'reconcile');
  assert.equal(gitModule.reconcile(root, plan).route, 'mergeApproval');
  const receipt = gitModule.prepareMerge(root, git(root, 'diff', '--cached', '--binary'));
  writeFileSync(join(root, '.git', 'hooks', 'commit-msg'), '#!/bin/sh\nprintf "chore: Rewritten\\n" > "$1"\n', {mode:0o755});
  assert.throws(() => gitModule.commitMerge(root, receipt), /message/i);
}));

test('a conflicted rejected push requires reviewed merge before another push', () => remoteRepo((root, peer) => {
  writeFileSync(join(root, 'base.txt'), 'ours\n'); git(root, 'add', 'base.txt'); git(root, 'commit', '-qm', 'ours');
  writeFileSync(join(peer, 'base.txt'), 'theirs\n'); git(peer, 'add', 'base.txt'); git(peer, 'commit', '-qm', 'theirs'); git(peer, 'push', '-q', 'origin', 'main');
  const plan = gitModule.pushPlan(root, [git(root, 'rev-parse', 'HEAD').trim()]);
  assert.equal(gitModule.pushApproved(root, plan).route, 'reconcile');
  const conflict = gitModule.reconcile(root, plan);
  assert.equal(conflict.route, 'conflicts'); assert.deepEqual(conflict.paths, ['base.txt']);
  writeFileSync(join(root, 'base.txt'), 'ours and theirs\n');
  const merge = gitModule.prepareMerge(root, git(root, 'diff', '--cached', '--binary'));
  assert.match(merge.staged, /ours and theirs/);
  writeFileSync(join(root, 'base.txt'), 'unexpected edit\n');
  assert.throws(() => gitModule.commitMerge(root, merge), /changed|merge/i);
}));

test('merge review rejects unrelated content staged after conflict resolution begins', () => remoteRepo((root, peer) => {
  writeFileSync(join(root, 'base.txt'), 'ours\n'); git(root, 'commit', '-qam', 'ours');
  writeFileSync(join(peer, 'base.txt'), 'theirs\n'); git(peer, 'commit', '-qam', 'theirs'); git(peer, 'push', '-q', 'origin', 'main');
  const plan = gitModule.pushPlan(root, [git(root, 'rev-parse', 'HEAD').trim()]);
  assert.equal(gitModule.pushApproved(root, plan).route, 'reconcile');
  assert.equal(gitModule.reconcile(root, plan).route, 'conflicts');
  const baseline = git(root, 'diff', '--cached', '--binary');
  writeFileSync(join(root, 'unrelated.txt'), 'do not merge\n'); git(root, 'add', 'unrelated.txt');
  writeFileSync(join(root, 'base.txt'), 'ours and theirs\n');
  assert.throws(() => gitModule.prepareMerge(root, baseline), /index|staged|unrelated/i);
  assert.equal(git(root, 'ls-files', '-u').length > 0, true);
}));

test('new run refuses to treat an unfinished merge as an ordinary staged commit', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-pending-merge-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'base.txt'), 'base\n'); git(root, 'add', 'base.txt'); git(root, 'commit', '-qm', 'init');
    git(root, 'branch', 'other'); writeFileSync(join(root, 'base.txt'), 'main\n'); git(root, 'commit', '-qam', 'main');
    git(root, 'checkout', '-q', 'other'); writeFileSync(join(root, 'other.txt'), 'other\n'); git(root, 'add', 'other.txt'); git(root, 'commit', '-qm', 'other');
    git(root, 'merge', '--no-commit', '--no-ff', 'master');
    const kit = setupExtension(root);
    await assert.rejects(kit.call('start'), /merge/i);
    assert.equal(git(root, 'rev-parse', 'MERGE_HEAD').trim().length, 40);
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test('commit-msg hook cannot silently rewrite the approved exact message', () => repo(root => {
  writeFileSync(join(root, 'base.txt'), 'changed\n');
  const receipt = stageGroup(root, inventory(root), {source:'working',reason:'Change',changes:[{path:'base.txt'}]});
  writeFileSync(join(root, '.git', 'hooks', 'commit-msg'), '#!/bin/sh\nprintf "chore: Rewritten\\n" > "$1"\n', {mode:0o755});
  const verdict = gitModule.commitGroup(root, receipt, 'fix: Approved text');
  assert.equal(verdict.route, 'blocked');
  assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'chore: Rewritten');
}));

test('commit verifies exact message and hook failure preserves reviewable index', () => repo(root => {
  writeFileSync(join(root, 'base.txt'), 'new\n');
  const snap = inventory(root), group = {source:'working', reason:'Change', changes:[{path:'base.txt'}]};
  const receipt = stageGroup(root, snap, group);
  assert.equal(gitModule.commitGroup(root, receipt, 'fix: Update base').route, 'verify');
  assert.equal(git(root, 'log', '-1', '--format=%B').trim(), 'fix: Update base');
  assert.equal(gitModule.verifyCommit(root, receipt), git(root, 'rev-parse', 'HEAD').trim());
  writeFileSync(join(root, 'base.txt'), 'broken\n');
  const next = stageGroup(root, inventory(root), group);
  const hook = join(root, '.git', 'hooks', 'pre-commit');
  writeFileSync(hook, '#!/bin/sh\nexit 1\n', {mode:0o755});
  const failed = gitModule.commitGroup(root, next, 'fix: Broken hook');
  assert.equal(failed.route, 'hookRepair');
  assert.equal(git(root, 'diff', '--cached', '--binary'), next.staged);
}));

test('hook repair stages scoped patch only and invalidates previous message approval', () => repo(root => {
  writeFileSync(join(root, 'base.txt'), 'fix old\n');
  const receipt = stageGroup(root, inventory(root), {source:'working',reason:'Fix',changes:[{path:'base.txt'}]});
  writeFileSync(join(root, 'base.txt'), 'fix new\n');
  const patch = git(root, 'diff', '--binary', '--', 'base.txt');
  const repaired = gitModule.stageRepair(root, receipt, patch);
  assert.match(repaired.staged, /fix new/);
  assert.throws(() => gitModule.commitGroup(root, receipt, 'fix: Old'), /changed|stale/i);
  writeFileSync(join(root, 'unrelated.txt'), 'no\n');
  let other;
  try { git(root, 'diff', '--no-index', '--binary', '--', '/dev/null', 'unrelated.txt'); }
  catch (error) { other = error.stdout; }
  assert.throws(() => gitModule.stageRepair(root, repaired, other), /path|scope/i);
}));

test('staged first group remains intact on skip and cannot mix unrelated working files', () => repo(root => {
  writeFileSync(join(root, 'base.txt'), 'staged\n'); git(root, 'add', 'base.txt');
  writeFileSync(join(root, 'extra.txt'), 'not approved\n');
  const before = inventory(root);
  const staged = { source: 'staged', reason: 'Initial index', changes: [{ path: 'base.txt' }] };
  assert.throws(() => validateGroups(before, { groups: [{ ...staged, source: 'working' }], excluded: ['extra.txt: scratch'] }));
  validateGroups(before, { groups: [staged], excluded: ['extra.txt: scratch'] });
  const receipt = stageGroup(root, before, staged);
  skipGroup(root, receipt);
  assert.match(git(root, 'diff', '--cached'), /staged/);
  assert.equal(git(root, 'status', '--porcelain', '--', 'extra.txt'), '?? extra.txt\n');
}));

test('partial hunk and individual untracked path stage exactly the approved content', () => repo(root => {
  const original = Array.from({ length: 25 }, (_, i) => `line ${i + 1}`).join('\n') + '\n';
  writeFileSync(join(root, 'base.txt'), original); git(root, 'add', 'base.txt'); git(root, 'commit', '-qm', 'seed');
  writeFileSync(join(root, 'base.txt'), original.replace('line 1\n', 'first edit\n').replace('line 25\n', 'second edit\n'));
  writeFileSync(join(root, 'file with spaces.txt'), 'approved new file\n');
  mkdirSync(join(root, 'untracked-dir')); writeFileSync(join(root, 'untracked-dir', 'hidden.txt'), 'ignore me\n');
  const before = inventory(root);
  assert.deepEqual(before.hunks['base.txt'].length, 2);
  assert.ok(before.untrackedPaths.includes('untracked-dir/hidden.txt'));
  assert.ok(!before.untrackedPaths.includes('untracked-dir'));
  const partial = { source: 'working', reason: 'One hunk', changes: [{ path: 'base.txt', hunks: [before.hunks['base.txt'][0]] }] };
  validateGroups(before, { groups: [partial], excluded: ['base.txt (other hunk): unrelated', 'file with spaces.txt: later', 'untracked-dir/hidden.txt: scratch'] });
  const receipt = stageGroup(root, before, partial);
  assert.match(receipt.staged, /first edit/); assert.doesNotMatch(receipt.staged, /second edit/);
  skipGroup(root, receipt);
  assert.equal(git(root, 'diff', '--cached'), '');
  const newFile = { source: 'working', reason: 'New file', changes: [{ path: 'file with spaces.txt' }] };
  validateGroups(before, { groups: [newFile], excluded: ['base.txt: later', 'untracked-dir/hidden.txt: scratch'] });
  const added = stageGroup(root, before, newFile);
  assert.match(added.staged, /approved new file/);
  assert.doesNotMatch(added.staged, /ignore me/);
  skipGroup(root, added);
  assert.equal(git(root, 'diff', '--cached'), '');
}));

test('partial hunk cannot stage an unapproved mode change', () => repo(root => {
  const original = Array.from({length:25}, (_, i) => `line ${i+1}`).join('\n') + '\n';
  writeFileSync(join(root, 'base.txt'), original); git(root, 'commit', '-qam', 'seed');
  writeFileSync(join(root, 'base.txt'), original.replace('line 1\n', 'first\n').replace('line 25\n', 'last\n'));
  chmodSync(join(root, 'base.txt'), 0o755);
  const snapshot = inventory(root);
  const group = {source:'working',reason:'Partial',changes:[{path:'base.txt',hunks:[snapshot.hunks['base.txt'][0]]}]};
  assert.throws(() => stageGroup(root, snapshot, group), /mode|metadata|partial/i);
  assert.equal(git(root, 'diff', '--cached'), '');
}));

test('changed worktree, changed index, invalid hunk and symlink identity fail closed', () => repo(root => {
  writeFileSync(join(root, 'base.txt'), 'changed\n');
  symlinkSync('base.txt', join(root, 'link'));
  const before = inventory(root);
  const group = { source: 'working', reason: 'Base', changes: [{ path: 'base.txt' }] };
  assert.throws(() => validateGroups(before, { groups: [{...group, changes:[{path:'base.txt', hunks:['bogus'] }]}], excluded: ['link: later'] }), /hunk/i);
  validateGroups(before, { groups: [group], excluded: ['link: later'] });
  writeFileSync(join(root, 'base.txt'), 'changed again\n');
  assert.throws(() => stageGroup(root, before, group), /changed/i);
  writeFileSync(join(root, 'base.txt'), 'changed\n');
  git(root, 'add', 'base.txt');
  assert.throws(() => stageGroup(root, before, group), /index|changed/i);
  git(root, 'reset', '-q', 'HEAD', '--', 'base.txt');
  rmSync(join(root, 'link')); symlinkSync('missing.txt', join(root, 'link'));
  const link = { source: 'working', reason: 'Link', changes: [{ path: 'link' }] };
  assert.throws(() => stageGroup(root, before, link), /changed/i);
}));

const extensionPath = fileURLToPath(new URL('./commit-changes.ts', import.meta.url));
const typeboxPath = fileURLToPath(new URL('../npm/node_modules/typebox/build/index.mjs', import.meta.url));
const localTui = fileURLToPath(new URL('../npm/node_modules/@earendil-works/pi-tui/dist/index.js', import.meta.url));
const tuiPath = existsSync(localTui) ? localTui : join(piRoot, 'node_modules/@earendil-works/pi-tui/dist/index.js');
const setupExtension = (root, decisions = [], replies = []) => {
  const factory = createJiti(import.meta.url, { alias: { typebox: typeboxPath, '@earendil-works/pi-tui': tuiPath } })(extensionPath).default;
  const commands = {}, events = [], handlers = {};
  let tool, lastResult;
  const pi = {
    registerCommand: (name, value) => { commands[name] = value; },
    registerTool: value => { tool = value; },
    on: (name, handler) => { handlers[name] = handler; },
    sendUserMessage: text => { events.push(['prompt', text]); },
    sendMessage: message => { events.push(['message', message.content]); },
  };
  factory(pi);
  const ui = {
    select: async (title, options) => { events.push(['menu', title, options]); return decisions.shift(); },
    input: async () => replies.shift(),
    notify: (...args) => events.push(['notice', ...args]),
  };
  const ctx = { cwd: root, mode: 'tui', hasUI: true, isIdle: () => true, ui };
  const call = async (action, extra = {}) => {
    const result = await tool.execute('test', { action, ...extra }, undefined, undefined, ctx);
    lastResult = result;
    events.push(['message', result.content[0].text]);
    return result.content[0].text;
  };
  return { commands, events, handlers, pi, ctx, call,
    rendered: () => tool.renderResult(lastResult, {expanded:false}, undefined).render(120).join('\n') };
};

test('push needs separate approval and displays the exact configured target', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-push-'));
  const bare = mkdtempSync(join(tmpdir(), 'commit-ext-bare-'));
  try {
    git(root, 'init', '-q'); git(bare, 'init', '--bare', '-q');
    git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    git(root, 'branch', '-M', 'main'); git(root, 'remote', 'add', 'origin', bare); git(root, 'push', '-qu', 'origin', 'main');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root, ['Approve groups', 'Commit', 'Stop']);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    await kit.call('propose', {proposal:{groups:[{source:'working',reason:'Change',changes:[{path:'file.txt'}]}],excluded:[]}});
    await kit.call('stage');
    assert.doesNotMatch(kit.rendered(), /diff --git/);
    await kit.call('present_message', {message:'fix: Update file'}); await kit.call('decide_message');
    await kit.call('commit');
    await kit.call('review_push');
    assert.ok(kit.events.some(e => e[0] === 'menu' && e[1].includes('refs/heads/main')));
    assert.equal(git(bare, 'rev-parse', 'main').trim(), git(root, 'rev-parse', 'HEAD^').trim());
  } finally { rmSync(root, {recursive:true,force:true}); rmSync(bare, {recursive:true,force:true}); }
});

test('hook fix restages only the repair and demands a new message decision', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-hook-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'invalid\n');
    const hook = join(root, '.git', 'hooks', 'pre-commit');
    writeFileSync(hook, '#!/bin/sh\ngrep -qx valid file.txt || exit 1\n', {mode:0o755});
    const kit = setupExtension(root, ['Approve groups', 'Commit', 'Fix', 'Commit']);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    await kit.call('propose', {proposal:{groups:[{source:'working',reason:'Change',changes:[{path:'file.txt'}]}],excluded:[]}});
    await kit.call('stage'); await kit.call('present_message', {message:'fix: Old attempt'}); await kit.call('decide_message');
    assert.match(await kit.call('commit'), /hook/i);
    assert.equal((await kit.handlers.tool_call({toolName:'edit',input:{path:join(root,'file.txt')}}))?.block, undefined);
    assert.equal((await kit.handlers.tool_call({toolName:'edit',input:{path:join(root,'other.txt')}})).block, true);
    writeFileSync(join(root, 'file.txt'), 'valid\n');
    const patch = git(root, 'diff', '--binary', '--', 'file.txt');
    await kit.call('repair', {patch});
    await assert.rejects(kit.call('decide_message'), /present|message|step/i);
    await kit.call('present_message', {message:'fix: Correct file'}); await kit.call('decide_message');
    await kit.call('commit');
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'fix: Correct file');
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test('revision instructions and skip preserve only approved staged content', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-revise-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root, ['Revise groups', 'Approve groups', 'Revise message', 'Skip group'], ['include the file', 'say why']);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    const group = {source:'working', reason:'Change', changes:[{path:'file.txt'}]};
    assert.match(await kit.call('propose', {proposal:{groups:[group],excluded:[]}}), /include the file/);
    await kit.call('propose', {proposal:{groups:[group],excluded:[]}});
    await kit.call('stage');
    await kit.call('present_message', {message:'fix: Update file'});
    assert.match(await kit.call('decide_message'), /say why/);
    await kit.call('present_message', {message:'fix: Explain file update'});
    await kit.call('decide_message');
    assert.equal(git(root, 'diff', '--cached'), '');
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'init');
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test('message is printed before commit menu and cancellation does not commit', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-ui-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root, ['Approve groups', undefined]);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    const group = { source: 'working', reason: 'Change', changes: [{path:'file.txt'}] };
    await kit.call('propose', {proposal:{groups:[group], excluded:[]}});
    await kit.call('stage');
    await kit.call('present_message', {message:'fix: Update file\n\nExplain the change.'});
    await kit.call('decide_message');
    const printed = kit.events.findIndex(e => e[0] === 'message' && e[1].includes('fix: Update file'));
    const menu = kit.events.findIndex(e => e[0] === 'menu' && e[1].startsWith('Commit 1:'));
    assert.ok(printed !== -1 && menu > printed);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'init');
    assert.match(git(root, 'diff', '--cached'), /new/);
  } finally { rmSync(root, { recursive:true, force:true }); }
});

test('stale or premature decisions and session change fail closed', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-stale-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root, ['Approve groups']);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    await assert.rejects(kit.call('decide_message'), /present|message|step/i);
    writeFileSync(join(root, 'file.txt'), 'external edit\n');
    await assert.rejects(kit.call('propose', {proposal:{groups:[{source:'working',reason:'Change',changes:[{path:'file.txt'}]}],excluded:[]}}), /changed/i);
    await kit.handlers.session_tree();
    await assert.rejects(kit.call('stage'), /No current/i);
    assert.equal(git(root, 'diff', '--cached'), '');
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test('explicit worktree run commits there without touching the session checkout', async () => {
  const parent = mkdtempSync(join(tmpdir(), 'commit-ext-worktrees-'));
  const main = join(parent, 'main'), linked = join(parent, 'linked');
  try {
    mkdirSync(main);
    git(main, 'init', '-q'); git(main, 'config', 'user.name', 'Tester'); git(main, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(main, 'file.txt'), 'old\n'); git(main, 'add', 'file.txt'); git(main, 'commit', '-qm', 'init');
    git(main, 'worktree', 'add', '-qb', 'feature', linked);
    writeFileSync(join(linked, 'file.txt'), 'new\n');
    const before = git(main, 'rev-parse', 'HEAD').trim();
    const kit = setupExtension(main, ['Approve groups', 'Commit', 'Stop']);
    assert.match(await kit.call('start', {worktree: linked}), /file.txt/);
    await assert.rejects(kit.call('inventory', {worktree: main}), /worktree.*start/i);
    assert.match(await kit.call('inventory'), /new/);
    assert.match(await kit.call('propose', {proposal:{groups:[{source:'working',reason:'Change',changes:[{path:'file.txt'}]}],excluded:[]}}), /approved/i);
    await kit.call('stage'); await kit.call('present_message', {message:'fix: Update linked file'});
    await kit.call('decide_message');
    assert.match(await kit.call('commit'), /local_merge|merge.*main/i);
    await assert.rejects(kit.call('review_push'), /No current interactive commit run/i);
    kit.ctx.cwd = linked;
    assert.match(await kit.call('local_merge', {proposal:{worktree:linked}}), /Stopped/);
    kit.ctx.cwd = main;
    assert.ok(kit.events.some(e => e[0] === 'menu' && e[1].includes(`into ${main}`)));
    assert.equal(git(linked, 'log', '-1', '--format=%s').trim(), 'fix: Update linked file');
    assert.equal(git(main, 'rev-parse', 'HEAD').trim(), before);
    assert.equal(readFileSync(join(main, 'file.txt'), 'utf8'), 'old\n');
    writeFileSync(join(linked, 'file.txt'), 'another edit\n');
    await kit.commands['commit-changes'].handler(linked, kit.ctx);
    assert.match(await kit.call('inventory'), /another edit/);
    assert.ok(kit.events.some(e => e[0] === 'prompt' && e[1].includes(linked)));
  } finally { rmSync(parent, {recursive:true,force:true}); }
});

test('API-exposed proposal.worktree starts and commits in the linked worktree', async () => {
  const parent = mkdtempSync(join(tmpdir(), 'commit-ext-api-target-'));
  const main = join(parent, 'main'), linked = join(parent, 'linked');
  try {
    mkdirSync(main);
    git(main, 'init', '-q'); git(main, 'config', 'user.name', 'Tester'); git(main, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(main, 'file.txt'), 'old\n'); git(main, 'add', 'file.txt'); git(main, 'commit', '-qm', 'init');
    git(main, 'worktree', 'add', '-qb', 'feature', linked);
    writeFileSync(join(main, 'root-only.txt'), 'keep me\n');
    writeFileSync(join(linked, 'file.txt'), 'new\n');
    const head = git(main, 'rev-parse', 'HEAD').trim();
    const kit = setupExtension(main, ['Approve groups', 'Commit']);
    const started = JSON.parse(await kit.call('start', {proposal:{worktree: linked}}));
    assert.equal(started.root, linked);
    await kit.call('propose', {proposal:{groups:[{source:'working', reason:'Fix linked file', changes:[{path:'file.txt'}]}], excluded:[]}});
    await kit.call('stage');
    await kit.call('present_message', {message:'fix: Update linked file\n\nKeep changes in the selected worktree.'});
    await kit.call('decide_message');
    assert.match(await kit.call('commit'), /local_merge/);
    assert.equal(git(linked, 'log', '-1', '--format=%s').trim(), 'fix: Update linked file');
    assert.equal(git(main, 'rev-parse', 'HEAD').trim(), head);
    assert.equal(readFileSync(join(main, 'file.txt'), 'utf8'), 'old\n');
  } finally { rmSync(parent, {recursive:true, force:true}); }
});

test('API-exposed proposal.worktree still rejects a different repository', async () => {
  const main = mkdtempSync(join(tmpdir(), 'commit-ext-api-main-'));
  const other = mkdtempSync(join(tmpdir(), 'commit-ext-api-other-'));
  try {
    for (const root of [main, other]) {
      git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
      writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    }
    writeFileSync(join(other, 'file.txt'), 'new\n');
    const kit = setupExtension(main);
    await assert.rejects(kit.call('start', {proposal:{worktree: other}}), /worktree|repository/i);
    assert.equal(git(other, 'diff', '--cached'), '');
  } finally { rmSync(main, {recursive:true, force:true}); rmSync(other, {recursive:true, force:true}); }
});

test('explicit worktree run rejects a different repository', async () => {
  const main = mkdtempSync(join(tmpdir(), 'commit-ext-main-'));
  const other = mkdtempSync(join(tmpdir(), 'commit-ext-other-'));
  try {
    for (const root of [main, other]) {
      git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
      writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    }
    writeFileSync(join(other, 'file.txt'), 'new\n');
    const kit = setupExtension(main);
    await assert.rejects(kit.call('start', {worktree: other}), /worktree|repository/i);
    assert.equal(git(other, 'diff', '--cached'), '');
  } finally { rmSync(main, {recursive:true,force:true}); rmSync(other, {recursive:true,force:true}); }
});

test('local merge reviews a clean branch and removes only the merged worktree with separate approvals', async () => {
  const parent = mkdtempSync(join(tmpdir(), 'commit-ext-local-'));
  const main = join(parent, 'main'), linked = join(parent, 'linked');
  try {
    mkdirSync(main);
    git(main, 'init', '-q'); git(main, 'config', 'user.name', 'Tester'); git(main, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(main, 'base.txt'), 'base\n'); git(main, 'add', 'base.txt'); git(main, 'commit', '-qm', 'init');
    git(main, 'branch', '-M', 'experiment-01');
    git(main, 'worktree', 'add', '-qb', 'feature', linked);
    writeFileSync(join(linked, 'plan.txt'), 'new plan\n'); git(linked, 'add', 'plan.txt'); git(linked, 'commit', '-qm', 'feature');
    writeFileSync(join(main, 'base.txt'), 'target change\n'); git(main, 'commit', '-qam', 'target');
    const kit = setupExtension(main, ['Merge locally', 'Commit merge', 'Remove worktree']);
    await kit.call('local_merge', {proposal:{worktree:linked}});
    assert.equal(existsSync(join(main, 'plan.txt')), false);
    assert.match(await kit.call('perform_local_merge'), /review_merge/);
    assert.equal(readFileSync(join(main, 'plan.txt'), 'utf8'), 'new plan\n');
    assert.deepEqual(readdirSync(parent).sort(), ['linked', 'main']);
    const review = await kit.call('review_merge');
    assert.match(review, /plan.txt/);
    await kit.call('commit_merge');
    assert.equal(git(main, 'rev-list', '--parents', '-n', '1', 'HEAD').trim().split(' ').length, 3);
    assert.ok(existsSync(linked));
    await kit.call('remove_worktree');
    assert.equal(existsSync(linked), false);
    assert.deepEqual(readdirSync(parent), ['main']);
  } finally { rmSync(parent, {recursive:true, force:true}); }
});

test('fast-forward merge preserves unrelated untracked files and offers separate cleanup', async () => {
  const parent = mkdtempSync(join(tmpdir(), 'commit-ext-local-ff-'));
  const main = join(parent, 'main'), linked = join(parent, 'linked');
  try {
    mkdirSync(main);
    git(main, 'init', '-q'); git(main, 'config', 'user.name', 'Tester'); git(main, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(main, 'base.txt'), 'base\n'); git(main, 'add', 'base.txt'); git(main, 'commit', '-qm', 'init');
    git(main, 'worktree', 'add', '-qb', 'feature', linked);
    writeFileSync(join(linked, 'feature.txt'), 'feature\n'); git(linked, 'add', 'feature.txt'); git(linked, 'commit', '-qm', 'feature');
    writeFileSync(join(main, 'scratch.txt'), 'untouched\n');
    const kit = setupExtension(main, ['Merge locally', 'Remove worktree']);
    assert.match(await kit.call('local_merge', {proposal:{worktree:linked}}), /perform_local_merge/);
    assert.match(await kit.call('perform_local_merge'), /remove_worktree/);
    assert.equal(git(main, 'rev-parse', 'HEAD').trim(), git(linked, 'rev-parse', 'HEAD').trim());
    assert.equal(git(main, 'rev-list', '--parents', '-n', '1', 'HEAD').trim().split(' ').length, 2);
    assert.equal(readFileSync(join(main, 'scratch.txt'), 'utf8'), 'untouched\n');
    await kit.call('remove_worktree');
    assert.equal(existsSync(linked), false);
    assert.equal(readFileSync(join(main, 'scratch.txt'), 'utf8'), 'untouched\n');
  } finally { rmSync(parent, {recursive:true, force:true}); }
});

test('fast-forward merge rejects colliding untracked paths and tracked changes', async () => {
  const parent = mkdtempSync(join(tmpdir(), 'commit-ext-local-collision-'));
  const main = join(parent, 'main'), linked = join(parent, 'linked');
  try {
    mkdirSync(main);
    git(main, 'init', '-q'); git(main, 'config', 'user.name', 'Tester'); git(main, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(main, 'base.txt'), 'base\n'); git(main, 'add', 'base.txt'); git(main, 'commit', '-qm', 'init');
    git(main, 'worktree', 'add', '-qb', 'feature', linked);
    writeFileSync(join(linked, 'feature.txt'), 'feature\n'); git(linked, 'add', 'feature.txt'); git(linked, 'commit', '-qm', 'feature');
    writeFileSync(join(main, 'feature.txt'), 'keep me\n');
    const kit = setupExtension(main, ['Merge locally']);
    await assert.rejects(kit.call('local_merge', {proposal:{worktree:linked}}), /untracked|overlap|collision/i);
    rmSync(join(main, 'feature.txt'));
    writeFileSync(join(main, 'base.txt'), 'working edit\n');
    await assert.rejects(kit.call('local_merge', {proposal:{worktree:linked}}), /clean|tracked/i);
    assert.notEqual(git(main, 'rev-parse', 'HEAD').trim(), git(linked, 'rev-parse', 'HEAD').trim());
  } finally { rmSync(parent, {recursive:true, force:true}); }
});

test('model can start a guarded run from a commit request without a slash command', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-start-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root);
    assert.match(await kit.call('start'), /file.txt/);
    assert.equal((await kit.handlers.tool_call({toolName:'bash',input:{command:'git commit -am bad'}})).block, true);
    await assert.rejects(kit.call('start'), /active|current/i);
    await kit.call('stop');
    kit.ctx.mode = 'print';
    await assert.rejects(kit.call('start'), /interactive/i);
    assert.equal(git(root,'log','-1','--format=%s').trim(),'init');
  } finally { rmSync(root, {recursive:true,force:true}); }
});

test('approved symlink cannot grant edit access to a file outside the repository', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-link-'));
  const outside = mkdtempSync(join(tmpdir(), 'commit-ext-outside-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(outside, 'secret.txt'), 'not approved\n');
    symlinkSync(join(outside, 'secret.txt'), join(root, 'link'));
    writeFileSync(join(root, '.git', 'hooks', 'pre-commit'), '#!/bin/sh\nexit 1\n', {mode:0o755});
    const kit = setupExtension(root, ['Approve groups', 'Commit', 'Fix']);
    await kit.call('start');
    await kit.call('propose', {proposal:{groups:[{source:'working',reason:'Link',changes:[{path:'link'}]}],excluded:[]}});
    await kit.call('stage'); await kit.call('present_message', {message:'fix: Add link'}); await kit.call('decide_message');
    assert.match(await kit.call('commit'), /hook/i);
    assert.equal((await kit.handlers.tool_call({toolName:'edit',input:{path:join(root,'link')}})).block, true);
    assert.equal(readFileSync(join(outside,'secret.txt'),'utf8'), 'not approved\n');
  } finally { rmSync(root,{recursive:true,force:true}); rmSync(outside,{recursive:true,force:true}); }
});

test('repair and direct shell calls remain blocked while a run is active', async () => {
  const root = mkdtempSync(join(tmpdir(), 'commit-ext-guard-'));
  try {
    git(root, 'init', '-q'); git(root, 'config', 'user.name', 'Tester'); git(root, 'config', 'user.email', 'test@example.org');
    writeFileSync(join(root, 'file.txt'), 'old\n'); git(root, 'add', 'file.txt'); git(root, 'commit', '-qm', 'init');
    writeFileSync(join(root, 'file.txt'), 'new\n');
    const kit = setupExtension(root, ['Approve groups']);
    kit.ctx.mode = 'print'; await kit.commands['commit-changes'].handler('', kit.ctx);
    assert.equal(kit.events.filter(e => e[0] === 'prompt').length, 0);
    kit.ctx.mode = 'tui'; await kit.commands['commit-changes'].handler('', kit.ctx);
    await kit.commands['commit-changes'].handler('', kit.ctx);
    assert.equal(kit.events.filter(e => e[0] === 'prompt').length, 1);
    assert.equal((await kit.handlers.tool_call({toolName:'bash',input:{command:'git commit -am bad'}})).block, true);
    assert.equal((await kit.handlers.tool_call({toolName:'write',input:{path:'file.txt',content:'bad'}})).block, true);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'init');
  } finally { rmSync(root, { recursive:true, force:true }); }
});

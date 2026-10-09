import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { lstatSync, readFileSync, readlinkSync, realpathSync } from 'node:fs';
import { resolve } from 'node:path';

export type Change = { path: string; hunks?: string[] };
export type Group = { source: 'staged' | 'working'; reason: string; changes: Change[] };
export type Proposal = { groups: Group[]; excluded: string[]; blocked?: string };
export type Snapshot = { head: string; staged: string; stagedPaths: string[]; workingPaths: string[];
  untrackedPaths: string[]; fingerprints: Record<string, string>; hunks: Record<string, string[]>;
  status: string; history: string };
export type StageReceipt = { head: string; staged: string; addedPatch: string; group: Group };

const git = (root: string, ...args: string[]) => execFileSync('git', args,
  { cwd: root, encoding: 'utf8', maxBuffer: 8_000_000, stdio: 'pipe' });
const apply = (root: string, args: string[], input: string) => execFileSync('git', args,
  { cwd: root, encoding: 'utf8', maxBuffer: 8_000_000, stdio: 'pipe', input });
const names = (root: string, args: string[]) => git(root, ...args, '-z').split('\0').filter(Boolean);
const parts = (diff: string) => diff.split(/(?=^@@ )/m).slice(1);
const hunkId = (part: string) => createHash('sha256').update(part.slice(part.indexOf('\n') + 1)).digest('hex').slice(0, 16);
const fingerprint = (root: string, path: string) => {
  const file = resolve(root, path);
  const stat = lstatSync(file, { throwIfNoEntry: false });
  if (!stat) return 'missing';
  return createHash('sha256').update(stat.isSymbolicLink() ? readlinkSync(file) : readFileSync(file))
    .update(String(stat.mode & 0o170111)).digest('hex');
};

export function assertNoPendingMerge(root: string): void {
  try {
    if (git(root, 'rev-parse', '-q', '--verify', 'MERGE_HEAD').trim())
      throw new Error('A merge is already pending; resolve or abort it before starting a new commit run');
  } catch (error) {
    if ((error as {status?:number}).status !== 1) throw error;
  }
}

export function inventory(root: string): Snapshot {
  const stagedPaths = names(root, ['diff', '--cached', '--name-only']);
  const workingPaths = names(root, ['diff', '--name-only']);
  const untrackedPaths = names(root, ['ls-files', '--others', '--exclude-standard']);
  return {
    head: git(root, 'rev-parse', 'HEAD').trim(), staged: git(root, 'diff', '--cached', '--binary'),
    stagedPaths, workingPaths, untrackedPaths,
    fingerprints: Object.fromEntries([...new Set([...workingPaths, ...untrackedPaths])].map(path => [path, fingerprint(root, path)])),
    hunks: Object.fromEntries(workingPaths.map(path => [path, parts(git(root, 'diff', '--binary', '--', path)).map(hunkId)])),
    status: git(root, 'status', '--short', '--branch'), history: git(root, 'log', '-5', '--format=%h %s'),
  };
}

export function validateGroups(snapshot: Snapshot, proposal: Proposal): void {
  if (!proposal || !Array.isArray(proposal.groups) || !Array.isArray(proposal.excluded) ||
    (!proposal.groups.length && !proposal.blocked?.trim()) || (proposal.groups.length > 0 && proposal.blocked))
    throw new Error('Provide commit groups and exclusions or a blocker');
  if (proposal.blocked) return;
  const used = new Set<string>();
  for (const [i, group] of proposal.groups.entries()) {
    if (!group.reason?.trim() || !Array.isArray(group.changes) || !group.changes.length ||
      group.source !== (snapshot.staged && i === 0 ? 'staged' : 'working'))
      throw new Error('Staged content must be first; each group needs changes and a reason');
    for (const change of group.changes) {
      const available = group.source === 'staged' ? snapshot.stagedPaths : [...snapshot.workingPaths, ...snapshot.untrackedPaths];
      if (!available.includes(change.path) || (change.hunks !== undefined &&
        (!Array.isArray(change.hunks) || !change.hunks.length || change.hunks.some(id => !snapshot.hunks[change.path]?.includes(id)))))
        throw new Error('Group contains unknown path or hunk');
      for (const id of change.hunks ?? ['whole-file']) {
        const prefix = `${group.source}\0${change.path}\0`;
        if (used.has(prefix + id) || (id === 'whole-file' && [...used].some(k => k.startsWith(prefix))) ||
          (id !== 'whole-file' && used.has(prefix + 'whole-file'))) throw new Error('Duplicate path or hunk');
        used.add(prefix + id);
      }
    }
    if (group.source === 'staged' && (group.changes.some(c => c.hunks) ||
      group.changes.map(c => c.path).sort().join('\0') !== [...snapshot.stagedPaths].sort().join('\0')))
      throw new Error('Pre-existing staged changes must stay intact');
  }
  for (const path of [...snapshot.workingPaths, ...snapshot.untrackedPaths]) {
    const changes = proposal.groups.flatMap(g => g.source === 'working' ? g.changes.filter(c => c.path === path) : []);
    const covered = changes.some(c => !c.hunks) || (snapshot.hunks[path]?.length &&
      snapshot.hunks[path].every(id => changes.some(c => c.hunks?.includes(id))));
    if (!covered && !proposal.excluded.some(s => s.startsWith(`${path}: `) || s.startsWith(`${path} (`)))
      throw new Error(`Unaccounted working change: ${path}`);
  }
}

export function stageGroup(root: string, original: Snapshot, group: Group): StageReceipt {
  const now = inventory(root);
  if (now.head !== original.head || now.staged !== (group.source === 'staged' ? original.staged : ''))
    throw new Error('HEAD or index changed after approval');
  if (group.source === 'staged') return { head: now.head, staged: now.staged, addedPatch: '', group };
  const patch = group.changes.map(change => {
    if (original.fingerprints[change.path] === undefined || now.fingerprints[change.path] !== original.fingerprints[change.path])
      throw new Error(`Working file changed after approval: ${change.path}`);
    let diff: string;
    if (original.untrackedPaths.includes(change.path)) {
      try { apply(root, ['diff', '--no-index', '--binary', '--', '/dev/null', change.path], '');
        throw new Error('Untracked file disappeared'); }
      catch (error) {
        const failure = error as { status?: number; stdout?: string };
        if (failure.status !== 1 || !failure.stdout) throw error;
        diff = failure.stdout;
      }
    } else diff = git(root, 'diff', '--binary', '--', change.path);
    if (!diff) throw new Error(`Approved change unavailable: ${change.path}`);
    if (!change.hunks) return diff;
    const hunks = parts(diff);
    if (diff.slice(0, diff.indexOf(hunks[0])).split('\n').some(line => line && !/^(diff --git |index |--- |\+\+\+ )/.test(line)))
      throw new Error(`Partial hunk selection would include unapproved mode or file metadata: ${change.path}`);
    const chosen = hunks.filter(p => change.hunks!.includes(hunkId(p)));
    if (chosen.length !== change.hunks.length) throw new Error(`Approved hunks changed: ${change.path}`);
    return diff.slice(0, diff.indexOf(hunks[0])) + chosen.join('');
  }).join('');
  apply(root, ['apply', '--reverse', '--check', '-'], patch);
  apply(root, ['apply', '--cached', '--check', '-'], patch);
  apply(root, ['apply', '--cached', '-'], patch);
  const staged = git(root, 'diff', '--cached', '--binary');
  if (!staged) throw new Error('No changes were staged');
  return { head: now.head, staged, addedPatch: patch, group };
}

export function commitGroup(root: string, receipt: StageReceipt, message: string):
  { route: 'verify' | 'hookRepair' | 'blocked'; reason?: string; hash?: string } {
  if (!message.trim() || message.length > 4000) throw new Error('An exact commit message is required');
  if (git(root, 'rev-parse', 'HEAD').trim() !== receipt.head || git(root, 'diff', '--cached', '--binary') !== receipt.staged)
    throw new Error('HEAD or staged content changed after message approval');
  try {
    git(root, 'diff', '--cached', '--check');
    git(root, 'commit', '-m', message);
  } catch (error) {
    const unchanged = git(root, 'rev-parse', 'HEAD').trim() === receipt.head && git(root, 'diff', '--cached', '--binary') === receipt.staged;
    return { route: unchanged ? 'hookRepair' : 'blocked', reason: String(error) };
  }
  try { return { route: 'verify', hash: verifyCommit(root, receipt, message) }; }
  catch (error) { return { route: 'blocked', reason: String(error) }; }
}

export function verifyCommit(root: string, receipt: StageReceipt, message?: string): string {
  const head = git(root, 'rev-parse', 'HEAD').trim();
  if (head === receipt.head || git(root, 'rev-parse', 'HEAD^').trim() !== receipt.head ||
    git(root, 'diff', 'HEAD^', 'HEAD', '--binary') !== receipt.staged)
    throw new Error('Committed content or parent differs from the approved staged diff; do not push');
  if (message !== undefined && git(root, 'log', '-1', '--format=%B').replace(/\n+$/, '') !== message.replace(/\n+$/, ''))
    throw new Error('Committed message differs from approved message');
  if (git(root, 'diff', '--cached', '--binary')) throw new Error('Index still contains changes after commit');
  return head;
}

export function stageRepair(root: string, receipt: StageReceipt, patch: string): StageReceipt {
  if (!patch?.startsWith('diff --git ')) throw new Error('Provide a unified repair patch');
  if (git(root, 'rev-parse', 'HEAD').trim() !== receipt.head || git(root, 'diff', '--cached', '--binary') !== receipt.staged)
    throw new Error('HEAD or index changed; repair requires a fresh review');
  const paths = apply(root, ['apply', '--numstat', '-'], patch).trim().split('\n').map(line => line.split('\t').at(-1));
  const approved = receipt.group.changes.map(change => change.path);
  if (paths.some(path => !path || !approved.includes(path))) throw new Error('Repair patch touches a path outside the approved group');
  // ponytail: Partial-file hook repair stops rather than risk staging excluded same-file hunks; add a temporary-index dry run if that use case becomes necessary.
  if (receipt.group.changes.some(change => change.hunks && paths.includes(change.path)))
    throw new Error('Cannot safely isolate repair to approved hunks in a partially staged file');
  apply(root, ['apply', '--reverse', '--check', '-'], patch);
  apply(root, ['apply', '--cached', '--check', '-'], patch);
  apply(root, ['apply', '--cached', '-'], patch);
  git(root, 'diff', '--cached', '--check');
  return { ...receipt, staged: git(root, 'diff', '--cached', '--binary') };
}

export type PushPlan = { head: string; hashes: string[]; branch: string; upstream: string; remote: string;
  target: string; fetchUrl: string; pushUrls: string; status: string };
export type MergeReceipt = { head: string; mergeHead: string; staged: string; message: string };
export type LocalMergePlan = { target: string; source: string; branch: string; head: string; sourceHead: string };

export function planLocalMerge(target: string, source: string): LocalMergePlan {
  if (realpathSync(target) === realpathSync(source)) throw new Error('Cannot merge a branch into its own worktree');
  assertNoPendingMerge(target);
  const branch = git(source, 'branch', '--show-current').trim();
  if (!branch || branch === git(target, 'branch', '--show-current').trim()) throw new Error('Use a different named source branch');
  if (git(source, 'status', '--porcelain', '-uall') || git(target, 'status', '--porcelain', '-uall'))
    throw new Error('Source and target worktrees must be clean, including untracked files');
  return { target, source, branch, head: git(target, 'rev-parse', 'HEAD').trim(),
    sourceHead: git(source, 'rev-parse', 'HEAD').trim() };
}

export function performLocalMerge(plan: LocalMergePlan): {baseline: string; conflicts: string[]} {
  if (JSON.stringify(planLocalMerge(plan.target, plan.source)) !== JSON.stringify(plan))
    throw new Error('Merge plan changed after approval');
  try { git(plan.target, 'merge', '--no-ff', '--no-commit', plan.sourceHead); }
  catch (error) {
    const conflicts = names(plan.target, ['diff', '--name-only', '--diff-filter=U']);
    if (!conflicts.length) throw new Error(`Merge did not prepare: ${String(error)}`);
  }
  if (git(plan.target, 'rev-parse', 'MERGE_HEAD').trim() !== plan.sourceHead)
    throw new Error('Merge head changed');
  return { baseline: git(plan.target, 'diff', '--cached', '--binary'),
    conflicts: names(plan.target, ['diff', '--name-only', '--diff-filter=U']) };
}

export function removeMergedWorktree(plan: LocalMergePlan, mergedHead: string): void {
  if (git(plan.target, 'rev-parse', 'HEAD').trim() !== mergedHead ||
    git(plan.target, 'branch', '--show-current').trim() === plan.branch ||
    git(plan.source, 'rev-parse', 'HEAD').trim() !== plan.sourceHead ||
    git(plan.source, 'status', '--porcelain', '-uall')) throw new Error('Merge or worktree changed; cannot remove it');
  git(plan.target, 'merge-base', '--is-ancestor', plan.sourceHead, 'HEAD');
  git(plan.target, 'worktree', 'remove', '--', plan.source);
}

export function pushPlan(root: string, hashes: string[]): PushPlan {
  const branch = git(root, 'branch', '--show-current').trim();
  let upstream = '', remote = '', target = '';
  try {
    upstream = git(root, 'rev-parse', '--abbrev-ref', '@{u}').trim();
    remote = git(root, 'config', '--get', `branch.${branch}.remote`).trim();
    target = git(root, 'config', '--get', `branch.${branch}.merge`).trim();
  } catch { /* no configured upstream */ }
  let fetchUrl = '', pushUrls = '';
  if (remote && remote !== '.') {
    try {
      fetchUrl = git(root, 'remote', 'get-url', remote).replace(/\n$/, '');
      pushUrls = git(root, 'remote', 'get-url', '--push', '--all', remote).replace(/\n$/, '');
    } catch { /* unconfigured remote */ }
  }
  return { head: git(root, 'rev-parse', 'HEAD').trim(), branch, upstream, remote, target,
    fetchUrl, pushUrls, hashes, status: git(root, 'status', '--short') };
}

function pushable(root: string, plan: PushPlan): boolean {
  const now = pushPlan(root, plan.hashes);
  return !!(plan.branch && plan.remote && plan.remote !== '.' && plan.upstream && plan.fetchUrl && plan.pushUrls &&
    plan.target.startsWith('refs/heads/') && plan.head === now.head && plan.branch === now.branch &&
    plan.remote === now.remote && plan.target === now.target && plan.upstream === now.upstream &&
    plan.fetchUrl === now.fetchUrl && plan.pushUrls === now.pushUrls);
}

export function pushApproved(root: string, plan: PushPlan): {route:'confirmed'|'reconcile'|'blocked';reason?:string} {
  if (!pushable(root, plan)) return { route: 'blocked', reason: 'HEAD or upstream changed after push approval' };
  try {
    git(root, 'push', '--', plan.remote, `HEAD:${plan.target}`);
    return { route: 'confirmed' };
  } catch (error) {
    const reason = String(error);
    return { route: /non-fast-forward|fetch first|rejected.*behind/i.test(reason) ? 'reconcile' : 'blocked', reason };
  }
}

export function reconcile(root: string, plan: PushPlan): { route:'fastForward'|'conflicts'|'mergeApproval'|'blocked'; reason?:string; hash?:string; paths?:string[] } {
  if (!pushable(root, plan)) return { route:'blocked', reason:'HEAD or upstream changed since push approval' };
  if (git(root, 'status', '--porcelain')) return { route:'blocked', reason:'Working tree is not clean; cannot fetch and merge safely' };
  try {
    git(root, 'fetch', plan.remote);
    let mergeError: unknown;
    try { git(root, 'merge', '--ff', '--no-commit', '@{u}'); }
    catch (error) { mergeError = error; }
    const conflicts = names(root, ['diff', '--name-only', '--diff-filter=U']);
    if (conflicts.length) return { route:'conflicts', paths: conflicts };
    try {
      if (git(root, 'rev-parse', '-q', '--verify', 'MERGE_HEAD').trim()) return { route:'mergeApproval' };
    } catch { /* fast-forward */ }
    if (mergeError) return { route:'blocked', reason:String(mergeError) };
    return { route:'fastForward', hash:git(root, 'rev-parse', 'HEAD').trim() };
  } catch (error) { return { route:'blocked', reason:String(error) }; }
}

export function prepareMerge(root: string, expectedStaged: string): MergeReceipt {
  if (expectedStaged === undefined || git(root, 'diff', '--cached', '--binary') !== expectedStaged)
    throw new Error('Merge index changed after reconciliation; unrelated staged content is not approved');
  const unmerged = names(root, ['diff', '--name-only', '--diff-filter=U']);
  const changed = names(root, ['diff', '--name-only']);
  if (changed.some(path => !unmerged.includes(path))) throw new Error('Unrelated working changes during merge');
  if (unmerged.length) git(root, 'add', '--', ...unmerged);
  if (git(root, 'ls-files', '-u')) throw new Error('Unresolved merge entries remain');
  git(root, 'diff', '--cached', '--check');
  return { head: git(root, 'rev-parse', 'HEAD').trim(), mergeHead: git(root, 'rev-parse', 'MERGE_HEAD').trim(),
    staged: git(root, 'diff', '--cached', '--binary'), message: 'Merge upstream into current branch' };
}

export function commitMerge(root: string, receipt: MergeReceipt): string {
  if (git(root, 'rev-parse', 'HEAD').trim() !== receipt.head ||
    git(root, 'rev-parse', 'MERGE_HEAD').trim() !== receipt.mergeHead ||
    git(root, 'diff', '--cached', '--binary') !== receipt.staged || git(root, 'ls-files', '-u') ||
    git(root, 'diff', '--binary'))
    throw new Error('Merge contents changed after approval; do not commit or push');
  git(root, 'diff', '--cached', '--check');
  git(root, 'commit', '-m', receipt.message);
  const [hash, first, second] = git(root, 'rev-list', '--parents', '-n', '1', 'HEAD').trim().split(' ');
  if (first !== receipt.head || second !== receipt.mergeHead || git(root, 'diff', 'HEAD^', 'HEAD', '--binary') !== receipt.staged)
    throw new Error('Merge commit differs from approved contents or parents; do not push');
  if (git(root, 'log', '-1', '--format=%B').replace(/\n+$/, '') !== receipt.message.replace(/\n+$/, ''))
    throw new Error('Merge commit message differs from approved message; do not push');
  return hash;
}

export function skipGroup(root: string, receipt: StageReceipt): void {
  if (git(root, 'rev-parse', 'HEAD').trim() !== receipt.head || git(root, 'diff', '--cached', '--binary') !== receipt.staged)
    throw new Error('HEAD or index changed; cannot safely skip');
  if (!receipt.addedPatch) return;
  apply(root, ['apply', '--cached', '--reverse', '--check', '-'], receipt.staged);
  apply(root, ['apply', '--cached', '--reverse', '-'], receipt.staged);
  if (git(root, 'diff', '--cached', '--binary')) throw new Error('Index changed while skipping');
}

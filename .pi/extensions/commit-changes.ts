import type { ExtensionAPI } from '@earendil-works/pi-coding-agent';
import { Type } from 'typebox';
import { Text } from '@earendil-works/pi-tui';
import { inventory, assertNoPendingMerge, validateGroups, stageGroup, skipGroup, commitGroup, stageRepair, pushPlan, pushApproved, reconcile, prepareMerge, commitMerge, planLocalMerge, performLocalMerge, removeMergedWorktree,
  type Snapshot, type Proposal, type StageReceipt, type PushPlan, type MergeReceipt, type LocalMergePlan } from './commit-changes/git.ts';
import { resolve } from 'node:path';
import { lstatSync, realpathSync } from 'node:fs';
import { execFileSync } from 'node:child_process';

type Run = { root: string; session: string; snapshot: Snapshot; phase: 'propose' | 'stage' | 'message' | 'decision' | 'commit' | 'repair' | 'push' | 'pushApproved' | 'reconcile' | 'conflicts' | 'mergeReview' | 'mergeDecision' | 'localApproved' | 'localCleanup';
  proposal?: Proposal; index: number; receipt?: StageReceipt; message?: string; hashes: string[]; plan?: PushPlan;
  merge?: MergeReceipt; mergeBaseline?: string; conflicts?: string[]; local?: LocalMergePlan; mergedHead?: string };
const result = (text: string) => ({ content: [{ type: 'text' as const, text }],
  details: { display: text.startsWith('{') ? 'Inventory sent to agent' : text.startsWith('Proposed commit message:') ? text : text.split('\n')[0] } });

function worktreeRoot(session: string, path?: string): string {
  const git = (cwd: string, ...args: string[]) => execFileSync('git', args, {cwd, encoding:'utf8'}).trim();
  const base = realpathSync(git(session, 'rev-parse', '--show-toplevel'));
  const candidate = realpathSync(resolve(session, path || '.'));
  const root = realpathSync(git(candidate, 'rev-parse', '--show-toplevel'));
  const common = (cwd: string) => realpathSync(resolve(cwd, git(cwd, 'rev-parse', '--git-common-dir')));
  const registered = git(base, 'worktree', 'list', '--porcelain').split('\n')
    .some(line => line.startsWith('worktree ') && realpathSync(line.slice(9)) === root);
  if (common(base) !== common(root) || !registered) throw new Error('Target must be a registered worktree of the current repository');
  return root;
}

function mainWorktree(root: string): string {
  const listing = execFileSync('git', ['worktree', 'list', '--porcelain'], {cwd: root, encoding: 'utf8'});
  const path = listing.split('\n')[0];
  if (!path.startsWith('worktree ')) throw new Error('Main checkout not found');
  return realpathSync(path.slice(9));
}

function editable(root: string, path: string): boolean {
  try {
    const target = resolve(root, path);
    return lstatSync(target).isFile() && realpathSync(target) === resolve(realpathSync(root), path);
  } catch { return false; }
}

export default function (pi: ExtensionAPI) {
  let active: Run | undefined;
  pi.on('session_start', () => { active = undefined; });
  pi.on('session_tree', () => { active = undefined; });
  pi.on('tool_call', event => {
    if (!active || event.toolName === 'commit_changes' || event.toolName === 'read') return;
    if (event.toolName === 'edit' &&
      ((active.phase === 'repair' && active.receipt?.group.changes.some(change => resolve(active.root, change.path) === resolve(active.root, String(event.input.path)) && editable(active.root, change.path))) ||
        (active.phase === 'conflicts' && active.conflicts?.some(path => resolve(active.root, path) === resolve(active.root, String(event.input.path)) && editable(active.root, path))))) return;
    return { block: true, reason: 'The commit run permits only read, commit_changes and scoped repair edits.' };
  });
  pi.registerCommand('commit-changes', {
    description: 'Review and commit changes in this checkout or a specified worktree path',
    handler: async (_args, ctx) => {
      if (ctx.mode !== 'tui' || !ctx.isIdle() || active) {
        ctx.ui.notify('An interactive idle Pi session with no active commit run is required', 'warning'); return;
      }
      let root: string;
      try { root = worktreeRoot(ctx.cwd, _args.trim()); assertNoPendingMerge(root); }
      catch (error) { ctx.ui.notify(String(error), 'error'); return; }
      active = { root, session: ctx.cwd, snapshot: inventory(root), phase: 'propose', index: 0, hashes: [] };
      if (!active.snapshot.staged && !active.snapshot.workingPaths.length && !active.snapshot.untrackedPaths.length) {
        active = undefined; ctx.ui.notify('Nothing to commit', 'info'); return;
      }
      pi.sendUserMessage(`Commit target: ${root}. Read .pi/skills/commit-changes/SKILL.md. Inspect all changes, then call commit_changes with a proposal. Do not run Git mutations or print the staged diff. Call commit_changes with action inventory for read-only Git data.`);
    },
  });
  pi.registerTool({
    name: 'commit_changes', label: 'Commit changes', executionMode: 'sequential',
    description: 'For commit requests, call start with worktree path or proposal: {worktree: path} when the API schema omits worktree. Use local_merge with proposal: {worktree: path} for a guarded local merge. Never run Git mutations yourself.',
    parameters: Type.Object({
      action: Type.Union(['start', 'inventory', 'propose', 'stage', 'present_message', 'decide_message', 'commit', 'repair', 'review_push', 'push', 'fetch_resolve', 'review_merge', 'commit_merge', 'local_merge', 'perform_local_merge', 'remove_worktree', 'stop'].map(Type.Literal)),
      proposal: Type.Optional(Type.Any()), message: Type.Optional(Type.String()), patch: Type.Optional(Type.String()),
      worktree: Type.Optional(Type.String({description:'Registered worktree path, for start only; defaults to the current checkout'})),
    }),
    renderResult: response => new Text(response.details?.display ?? 'Commit-changes step', 0, 0),
    async execute(_id, params, _signal, _onUpdate, ctx) {
      const { action } = params;
      if (action === 'local_merge') {
        if (active) throw new Error('A commit run is already active');
        if (ctx.mode !== 'tui') throw new Error('Merge approval requires an interactive Pi session');
        if (!params.proposal?.worktree) throw new Error('Specify the registered source worktree in proposal.worktree');
        const source = worktreeRoot(ctx.cwd, params.proposal.worktree);
        const target = worktreeRoot(ctx.cwd, mainWorktree(ctx.cwd));
        const local = planLocalMerge(target, source);
        const choice = await ctx.ui.select(`Merge ${local.branch} (${local.sourceHead}) into ${target} (${local.head})?\n${local.fastForward ? 'Fast-forward only; unrelated untracked target files remain untouched.' : 'Non-fast-forward merge; both worktrees are clean.'} No push or branch deletion.`, ['Merge locally', 'Stop']);
        if (choice !== 'Merge locally') return result('Stopped; no merge attempted.');
        if (JSON.stringify(planLocalMerge(target, source)) !== JSON.stringify(local)) throw new Error('Merge plan changed before approval');
        active = {root:target, session:ctx.cwd, snapshot:inventory(target), phase:'localApproved', index:0, hashes:[], local};
        return result('Local merge approved. Call commit_changes perform_local_merge.');
      }
      if (action === 'start') {
        if (active) throw new Error('A commit run is already active');
        if (ctx.mode !== 'tui') throw new Error('Commit approval requires an interactive Pi session');
        const root = worktreeRoot(ctx.cwd, params.worktree ?? params.proposal?.worktree);
        assertNoPendingMerge(root);
        const snapshot = inventory(root);
        if (!snapshot.staged && !snapshot.workingPaths.length && !snapshot.untrackedPaths.length)
          return result('Nothing to commit; no push attempted.');
        active = {root, session:ctx.cwd, snapshot, phase:'propose', index:0, hashes:[]};
        return result(JSON.stringify({root, ...snapshot, unstaged:gitDiff(root)}));
      }
      const run = active;
      if (params.worktree !== undefined || params.proposal?.worktree !== undefined) throw new Error('worktree may only be specified on start or local_merge');
      if (!run || ctx.cwd !== run.session || ctx.mode !== 'tui') throw new Error('No current interactive commit run');
      if (worktreeRoot(ctx.cwd, run.root) !== run.root) throw new Error('Commit worktree changed; start a new run');
      if (action === 'stop') { active = undefined; return result('Stopped. Git state was not discarded.'); }
      if (action === 'perform_local_merge') {
        if (run.phase !== 'localApproved' || !run.local) throw new Error('No approved local merge');
        const prepared = performLocalMerge(run.local);
        if (prepared.fastForward) {
          run.mergedHead = run.local.sourceHead;
          run.phase = 'localCleanup';
          return result('Fast-forward complete; call commit_changes remove_worktree for separate cleanup approval.');
        }
        run.mergeBaseline = prepared.baseline;
        run.conflicts = prepared.conflicts;
        run.phase = prepared.conflicts.length ? 'conflicts' : 'mergeReview';
        return result(prepared.conflicts.length ? `Resolve only these conflicts with edit: ${prepared.conflicts.join(', ')}; then call review_merge.` : 'Merge prepared; call commit_changes review_merge.');
      }
      if (action === 'remove_worktree') {
        if (run.phase !== 'localCleanup' || !run.local || !run.mergedHead) throw new Error('No completed local merge awaiting cleanup');
        const choice = await ctx.ui.select(`Remove merged worktree ${run.local.source}? Source branch is retained.`, ['Remove worktree', 'Stop']);
        if (choice !== 'Remove worktree') { active = undefined; return result('Worktree retained.'); }
        removeMergedWorktree(run.local, run.mergedHead);
        active = undefined;
        return result(`Removed ${run.local.source}; retained branch ${run.local.branch}.`);
      }
      if (action === 'inventory') {
        if (run.phase !== 'propose') throw new Error('Inventory is available during group proposal only');
        return result(JSON.stringify({ root: run.root, ...run.snapshot, unstaged: gitDiff(run.root) }));
      }
      if (action === 'propose') {
        if (run.phase !== 'propose') throw new Error('Group proposal is not expected now');
        validateGroups(run.snapshot, params.proposal as Proposal);
        const proposal = params.proposal as Proposal;
        if (proposal.blocked) { active = undefined; return result(`Blocked: ${proposal.blocked}`); }
        const summary = proposal.groups.map((g, i) => `Commit ${i + 1}: ${g.reason}\n${g.changes.map(c => `  ${c.path}${c.hunks ? ` (hunks ${c.hunks.join(', ')})` : ''}`).join('\n')}`).join('\n');
        const decision = await ctx.ui.select(`Review groups in ${run.root}\n${summary}\nExcluded: ${proposal.excluded.join('; ') || 'none'}`,
          ['Approve groups', 'Revise groups', 'Stop']);
        if (decision === 'Revise groups') {
          const instructions = (await ctx.ui.input('What should change?'))?.trim();
          if (instructions) return result(`Revise groups: ${instructions}. Re-inventory before proposing again.`);
        }
        if (decision !== 'Approve groups') { active = undefined; return result('Stopped; nothing staged.'); }
        const now = inventory(run.root);
        if (now.head !== run.snapshot.head || now.staged !== run.snapshot.staged ||
          Object.entries(run.snapshot.fingerprints).some(([path, hash]) => now.fingerprints[path] !== hash))
          throw new Error('Git state changed after proposal; start a new run');
        run.proposal = proposal; run.phase = 'stage';
        return result('Groups approved. Call commit_changes stage for group 1.');
      }
      if (action === 'stage') {
        if (run.phase !== 'stage' || !run.proposal) throw new Error('No approved group to stage');
        const group = run.proposal.groups[run.index];
        if (!group) throw new Error('No remaining group');
        const now = inventory(run.root);
        if (group.changes.some(change => group.source === 'working' && now.fingerprints[change.path] !== run.snapshot.fingerprints[change.path]))
          throw new Error('Working file changed after group approval; start a new run');
        run.receipt = stageGroup(run.root, run.snapshot, group);
        run.phase = 'message';
        return result(`Group staged. Draft a specific message from the complete staged diff (do not print the diff):\n${run.receipt.staged}`);
      }
      if (action === 'present_message') {
        if (run.phase !== 'message' || !run.receipt || !params.message?.trim() || params.message.length > 4000)
          throw new Error('An exact message for the staged group is required');
        if (inventory(run.root).staged !== run.receipt.staged) throw new Error('Staged diff changed; start a new run');
        run.message = params.message;
        run.phase = 'decision';
        pi.sendMessage({ customType: 'commit-message-preview', content: `Proposed commit message:\n\n${params.message}`, display: true }, { triggerTurn: false });
        return result(`Proposed commit message:\n\n\`\`\`text\n${params.message}\n\`\`\`\n\nReview it, then call commit_changes decide_message in a separate turn.`);
      }
      if (action === 'decide_message') {
        if (run.phase !== 'decision' || !run.message || !run.receipt) throw new Error('Present the exact message before opening its decision menu');
        const decision = await ctx.ui.select(`Commit ${run.index + 1}: ${run.receipt.group.reason}`, ['Commit', 'Revise message', 'Skip group', 'Stop']);
        if (decision === 'Revise message') {
          const instructions = (await ctx.ui.input('What should change?'))?.trim();
          if (instructions) { run.phase = 'message'; return result(`Revise the exact message: ${instructions}. Present it again before approval.`); }
        }
        if (decision === 'Skip group') {
          skipGroup(run.root, run.receipt);
          if (!run.receipt.addedPatch && run.index + 1 < run.proposal!.groups.length) {
            active = undefined; return result('Pre-existing staged group left untouched; cannot stage later groups over it.');
          }
          run.index++;
          if (run.index >= run.proposal!.groups.length) {
            if (run.hashes.length) {
              if (run.root !== mainWorktree(run.root)) {
                active = undefined;
                return result(`Committed ${run.hashes.length} group(s) in a linked worktree. Offer a local_merge into the main checkout next, not a push.`);
              }
              run.phase = 'push'; return result(`Committed ${run.hashes.length} group(s); review push next.`);
            }
            active = undefined; return result('No groups committed. No push attempted.');
          }
          run.receipt = undefined; run.message = undefined; run.phase = 'stage';
          return result(`Skipped group. Stage group ${run.index + 1}.`);
        }
        if (decision !== 'Commit') { active = undefined; return result('Stopped with staged changes left intact.'); }
        run.phase = 'commit';
        return result('Commit approved. Call commit_changes commit.');
      }
      if (action === 'commit') {
        if (run.phase !== 'commit' || !run.receipt || !run.message) throw new Error('No approved commit message and diff');
        const committed = commitGroup(run.root, run.receipt, run.message);
        if (committed.route === 'blocked') { active = undefined; return result(`Commit blocked: ${committed.reason}`); }
        if (committed.route === 'hookRepair') {
          const decision = await ctx.ui.select(`Commit hook failed: ${committed.reason}`, ['Fix', 'Stop']);
          if (decision !== 'Fix') { active = undefined; return result('Stopped. Approved staged content remains in the index.'); }
          run.phase = 'repair';
          return result('Hook failed. Edit only approved group paths, then call commit_changes repair with a unified patch against the staged index. Do not run Git.');
        }
        run.hashes.push(committed.hash!);
        run.index++;
        if (run.index >= run.proposal!.groups.length) {
          if (run.root !== mainWorktree(run.root)) {
            active = undefined;
            return result(`Committed ${committed.hash} in a linked worktree. Offer a local_merge into the main checkout next, not a push.`);
          }
          run.phase = 'push'; run.receipt = undefined; run.message = undefined;
          return result(`Committed ${committed.hash}. Review push next.`);
        }
        const now = inventory(run.root);
        run.snapshot = { ...run.snapshot, head: now.head, staged: now.staged };
        run.receipt = undefined; run.message = undefined; run.phase = 'stage';
        return result(`Committed ${committed.hash}. Stage group ${run.index + 1} next.`);
      }
      if (action === 'repair') {
        if (run.phase !== 'repair' || !run.receipt) throw new Error('No failed hook requiring repair');
        run.receipt = stageRepair(run.root, run.receipt, params.patch ?? '');
        run.message = undefined; run.phase = 'message';
        return result(`Repair staged. Review the complete staged diff and draft a fresh commit message:\n${run.receipt.staged}`);
      }
      if (action === 'review_push') {
        if (run.phase !== 'push' || !run.hashes.length) throw new Error('No committed groups to push');
        const plan = pushPlan(run.root, run.hashes);
        const choice = await ctx.ui.select(`Push ${plan.hashes.join(', ')} from ${run.root} to ${plan.remote} ${plan.target || '(no upstream)'}?\nRemaining: ${plan.status || 'clean'}`, ['Push', 'Stop']);
        if (choice !== 'Push') { active = undefined; return result(`Stopped without pushing. Commits: ${run.hashes.join(', ')}`); }
        run.plan = plan; run.phase = 'pushApproved';
        return result(`Push approved to ${plan.remote} ${plan.target}. Call commit_changes push.`);
      }
      if (action === 'push') {
        if (run.phase !== 'pushApproved' || !run.plan) throw new Error('No approved push target');
        const pushed = pushApproved(run.root, run.plan);
        if (pushed.route === 'confirmed' || pushed.route === 'blocked') {
          active = undefined; return result(`Push ${pushed.route}: ${pushed.reason ?? run.hashes.join(', ')}`);
        }
        const choice = await ctx.ui.select(`Push rejected: ${pushed.reason}`, ['Fetch and resolve', 'Stop']);
        if (choice !== 'Fetch and resolve') { active = undefined; return result('Push rejected; no fetch or merge attempted.'); }
        run.phase = 'reconcile';
        return result('Fetch and resolve approved. Call commit_changes fetch_resolve.');
      }
      if (action === 'fetch_resolve') {
        if (run.phase !== 'reconcile' || !run.plan) throw new Error('No approved fetch and resolve step');
        const merged = reconcile(run.root, run.plan);
        if (merged.route === 'blocked') { active = undefined; return result(`Reconciliation blocked: ${merged.reason}`); }
        if (merged.route === 'fastForward') {
          const pushed = pushApproved(run.root, { ...run.plan, head: merged.hash! });
          active = undefined; return result(`Push ${pushed.route}: ${pushed.reason ?? run.hashes.join(', ')}`);
        }
        run.mergeBaseline = gitIndex(run.root);
        if (merged.route === 'conflicts') {
          run.conflicts = merged.paths; run.phase = 'conflicts';
          return result(`Resolve only these conflicted files with edit; then call review_merge: ${merged.paths!.join(', ')}`);
        }
        run.phase = 'mergeReview'; return result('Merge prepared; call commit_changes review_merge.');
      }
      if (action === 'review_merge') {
        if (run.phase !== 'mergeReview' && run.phase !== 'conflicts') throw new Error('No pending merge to review');
        run.merge = prepareMerge(run.root, run.mergeBaseline!);
        if (run.local) run.merge.message = `Merge ${run.local.branch} into ${gitBranch(run.root)}`;
        run.phase = 'mergeDecision';
        const review = `Merge message:\n\n${run.merge.message}\n\nHEAD: ${run.merge.head}\nMerge head: ${run.merge.mergeHead}\n\nStaged merge diff:\n${run.merge.staged}`;
        pi.sendMessage({customType:'commit-message-preview', content:review, display:true}, {triggerTurn:false});
        return result(`${review}\n\nReview the merge, then call commit_changes commit_merge in a separate turn.`);
      }
      if (action === 'commit_merge') {
        if (run.phase !== 'mergeDecision' || !run.merge || (!run.plan && !run.local)) throw new Error('No reviewed merge commit');
        const choice = await ctx.ui.select(run.local ? 'Commit this exact local merge?' : 'Commit this exact merge and push?', run.local ? ['Commit merge', 'Stop'] : ['Commit merge and push', 'Stop']);
        if (choice !== (run.local ? 'Commit merge' : 'Commit merge and push')) { active = undefined; return result('Stopped before merge commit; merge remains pending.'); }
        try {
          const hash = commitMerge(run.root, run.merge);
          if (run.local) {
            run.mergedHead = hash; run.phase = 'localCleanup';
            return result(`Local merge commit ${hash}. Call commit_changes remove_worktree for separate cleanup approval.`);
          }
          const pushed = pushApproved(run.root, {...run.plan!, head:hash});
          active = undefined; return result(`Merge commit ${hash}. Push ${pushed.route}: ${pushed.reason ?? 'confirmed'}`);
        } catch (error) { active = undefined; return result(`Merge commit blocked: ${String(error)}`); }
      }
      throw new Error(`Action ${action} is unavailable at this step`);
    },
  });
}

const gitBranch = (root: string) => execFileSync('git', ['branch', '--show-current'], {cwd:root, encoding:'utf8'}).trim();
const gitDiff = (root: string) => execFileSync('git', ['diff', '--binary'], { cwd: root, encoding: 'utf8', maxBuffer: 8_000_000 });
const gitIndex = (root: string) => execFileSync('git', ['diff', '--cached', '--binary'], { cwd: root, encoding: 'utf8', maxBuffer: 8_000_000 });

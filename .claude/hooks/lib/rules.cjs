'use strict';

const RULES = {
  process: ['process tools', 'CLAUDE.md "Standing rules" (Riot Games) and plan/design/workflow.md "Hooks"'],
  data: ['the real data folder', 'CLAUDE.md "Standing rules": never touch %APPDATA%\\OneLaunch'],
  launcher: ['starting a launcher', 'CLAUDE.md "Standing rules": never start or stop a launcher'],
  installer: ['starting an installer', 'CLAUDE.md "Standing rules": never run an installer on this machine'],
  registry: ['registry writes', 'CLAUDE.md "Standing rules": never modify launcher-owned files or registry keys'],
  tags: ['tags and releases', 'CLAUDE.md "Standing rules": releases are the owner\'s'],
  merge: ['merge without --auto', 'plan/design/workflow.md "Identity and merging": GitHub merges, not an agent'],
  bypass: ['hook bypass and force-push', 'plan/design/workflow.md "Hooks"'],
  deps: ['dependencies', 'CLAUDE.md "Standing rules": only the approved list in plan/PLAN.md'],
  app: ['the app without --data-dir', 'CLAUDE.md "Standing rules": run the app only with --data-dir on a scratch folder'],
  github: ['GitHub other than as the app', 'plan/design/workflow.md "Acting as the app"'],
  key: ["the app's key and tokens", 'plan/design/workflow.md "Acting as the app"; CLAUDE.md "Standing rules": never print secrets'],
  secrets: ['the secret scan before a commit', 'plan/design/workflow.md "Hooks"; CLAUDE.md "Standing rules": never commit secrets, emails or account ids'],
  scope: ['write_scope', 'plan/design/workflow.md "Ticket" and "Hooks"'],
  owner: ['owner-approved paths', 'plan/design/workflow.md "Owner-approved paths"'],
  unreadable: ['a command the hook cannot read', 'plan/design/workflow.md "Hooks": the hook could not check this call against them'],
  failed: ['fail closed', 'plan/design/workflow.md "Hooks": the hook could not check this call against them'],
};

class Refusal extends Error {
  constructor(rule, detail) {
    super(detail);
    this.rule = rule;
  }
}

function explain(error) {
  const known = error instanceof Refusal && RULES[error.rule];
  const [title, where] = known || RULES.failed;
  const detail = known ? error.message : `the hook failed while reading this call (${error && error.message})`;
  return `Refused by the safety hook: ${detail}.\nRule: ${title} (${where}).\n`;
}

function unloaded(error) {
  const why = (error && (error.code || error.message)) || 'it gave no verdict';
  require('node:fs').writeSync(2, explain(new Refusal('failed', `the hook could not be loaded (${why})`)));
}

module.exports = { RULES, Refusal, explain, unloaded };

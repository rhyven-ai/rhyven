import {escapeHTML as esc, icon} from './docs.js';

export const skills = [
  {id:'use-rhyven', title:'Use Rhyven', icon:'layers', label:'AGENT USAGE',
    description:'Connect an agent, discover and use apps, and manage marketplace installs with human approval.',
    requires:'Rhyven · MCP connection or CLI', rule:true},
  {id:'publish-rhyven-app', title:'Publish an app', icon:'branch', label:'DISTRIBUTION',
    description:'Validate a release, upload its package and image, and submit a marketplace listing.',
    requires:'Rhyven · Git · GitHub CLI'},
  {id:'build-rhyven-declarative-app', title:'Build a declarative app', icon:'layers', label:'ENGINE',
    description:'Define records, rules, calculations and search in JSON, with a complete tested example.',
    requires:'Rhyven'},
  {id:'build-rhyven-container-app', title:'Build an on-demand container', icon:'box', label:'CONTAINER',
    description:'Package custom code for bounded actions. Includes the execution contract and a Python Dockerfile.',
    requires:'Rhyven · Docker'},
  {id:'build-rhyven-service-app', title:'Build a persistent service', icon:'pulse', label:'SERVICE',
    description:'Run background work with durable state and supervised recovery. Includes a Python Dockerfile.',
    requires:'Rhyven · Docker'}
];

let revision = 0;
const cache = new Map();
const pathFor = (skill, rule) => `skills/${skill.id}/${rule?'RULE.md':'SKILL.md'}`;

export async function renderSkill(id, format) {
  const skill = skills.find(s=>s.id===id) || skills[0];
  const rule = skill.rule && format==='rule';
  const filename = rule?'RULE.md':'SKILL.md';
  const path = pathFor(skill, rule);
  const current = ++revision;
  const reader = document.querySelector('#skill-reader');
  document.querySelector('#skills-nav').innerHTML = skills.map(s=>
    `<a href="#skills/${s.id}" ${s.id===skill.id?'aria-current="page"':''}>
      <span class="skill-nav-icon">${icon(s.icon)}</span><span><span class="tiny-label">${s.label}</span><strong>${s.title}</strong></span>${icon('chevron')}
    </a>`).join('');
  reader.setAttribute('aria-busy','true');
  reader.innerHTML = '<p class="skill-loading" role="status">Loading instructions…</p>';
  try {
    let source = cache.get(path);
    if (!source) {
      const response = await fetch(path);
      if (!response.ok) throw new Error('Skill request failed');
      source = await response.text();
      cache.set(path,source);
    }
    if (current!==revision) return;
    const lines = source.trimEnd().split('\n').length;
    reader.innerHTML = `<div class="skill-heading"><span class="eyebrow">${skill.label} ${rule?'RULE':'SKILL'}</span><h2 id="skill-title">${skill.title}</h2><p>${skill.description}</p></div>
      ${skill.rule?`<nav class="skill-formats" aria-label="Instruction format"><a href="#skills/${skill.id}" ${!rule?'aria-current="page"':''}>Skill</a><a href="#skills/${skill.id}/rule" ${rule?'aria-current="page"':''}>Rule</a></nav><p class="skill-format-help">Choose the skill for task-specific guidance, or the shorter rule for your agent’s project instructions. Either works with the same Rhyven interface.</p>`:''}
      <dl class="skill-facts"><div><dt>Requires</dt><dd>${skill.requires}</dd></div><div><dt>Length</dt><dd>${lines} / 500 lines</dd></div><div><dt>Format</dt><dd>${filename}</dd></div></dl>
      <div class="skill-actions"><button type="button" class="button button-primary" data-copy-skill>${icon('copy')}<span>Copy</span><span class="sr-only"> complete ${rule?'rule':'skill'}</span></button><a class="button" href="${path}" download="${filename}">Download ${filename} ${icon('arrow')}</a><a class="text-link" href="${path}">Open raw file ${icon('arrow-up')}</a></div>
      <div class="skill-source-heading"><span class="tiny-label">COMPLETE AGENT INSTRUCTIONS</span><span>Rhyven 0.4.0-rc.8</span></div>
      <pre class="skill-source" tabindex="0" aria-label="${skill.title}: complete ${filename}"><code>${esc(source)}</code></pre>
      <p class="skill-use">${rule?'Append this rule to your agent’s supported project instructions file (for example, <code>AGENTS.md</code> where supported), or add it through the client’s rules settings. Preserve existing instructions; downloading <code>RULE.md</code> alone does not activate it.':`Save this file as <code>${skill.id}/SKILL.md</code> in your agent’s supported skills folder, or attach it to your agent with the task you want completed.`}</p>`;
  } catch {
    if (current!==revision) return;
    reader.innerHTML = `<div class="empty-state" role="alert"><h2>These instructions could not be loaded.</h2><p>Try again or <a href="${path}">open the Markdown file directly</a>.</p><button type="button" class="button" data-retry-skill>Try again</button></div>`;
  } finally {
    if (current===revision) reader.removeAttribute('aria-busy');
  }
}

import {docs, escapeHTML as esc, icon, codeBlock, installCommand} from './docs.js';
import {renderSkill} from './skills.js';

const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];
const appDialog = $('#app-dialog');
const groups = {
  declarative:{title:'Declarative apps', subtitle:'State, rules and workflows. No Docker required.', description:'Define structured records, relationships, rules and actions in an app manifest.', icon:'layers'},
  container:{title:'On-demand containers', subtitle:'Custom code through MCP and REST.', description:'Run custom logic in a packaged environment, with persistent data between calls.', icon:'box'},
  service:{title:'Persistent services', subtitle:'Supervised background processes.', description:'Keep a supervised process available for background work and durable delivery.', icon:'pulse'}
};
const appPresentation = {
  'work-management':{title:'Work Management',icon:'work',color:'cyan',short:'Create tasks and issues with owners, priorities, labels and due dates.'},
  'project-knowledge':{title:'Project Knowledge',icon:'book',color:'blue',short:'Store and search project notes, decisions and corrections across sessions.'},
  'error-management':{title:'Error Management',icon:'bug',color:'purple',short:'Track failures, link remediation and record what fixed them.'},
  'ci-management':{title:'CI Management',icon:'branch',color:'green',short:'Track pipeline runs, stages and results in structured state.'},
  inventory:{title:'Inventory',icon:'box',color:'amber',short:'Track assets and locations with a simple, agent-readable lifecycle.'},
  'repo-documentation-tool':{title:'Rhyven Repo Documentation Tool',icon:'code',color:'cyan',short:'Map a repository’s symbols, relationships and agent-written summaries.'},
  messaging:{title:'Messaging',icon:'message',color:'purple',short:'Send messages through persistent local inboxes and channels.'}
};
const wholeMatch = (pattern, value) => typeof value==='string' && pattern.exec(value)?.[0]===value;
function validInstallMetadata(app) {
  return app && wholeMatch(/^[a-z][a-z0-9_-]{0,63}\/[a-z][a-z0-9_-]{0,63}/, app.id)
    && app.name===app.id.split('/')[1] && Object.hasOwn(groups, app.kind)
    && (app.registry===null || wholeMatch(/^[A-Za-z0-9][A-Za-z0-9-]*\/[A-Za-z0-9_][A-Za-z0-9_.-]*/, app.registry));
}
const presentationFor = app => Object.hasOwn(appPresentation, app.name)
  ? appPresentation[app.name] : {title:app.name,icon:'box',color:'cyan',short:app.description};
const workflow = [
  {category:'rhyven/work-management',function:'object_task_create',args:{data:{title:'Verify release recovery',labels:['release']}},title:'Create a task record',body:'A task gives the next agent a stable record to read, update and complete.'},
  {category:'rhyven/project-knowledge',function:'action_remember',args:{title:'Recovery decision',body:'Restore database and container files.',topic:'release'},title:'Save a project decision',body:'An immutable note preserves the decision and its context beyond this session.'},
  {category:'rhyven/messaging',function:'action_send_direct',args:{recipient:'reviewer',body:'Recovery is ready for review.',message_key:'release-handoff-1'},title:'Send a message to another agent',body:'The next agent polls its inbox, claims the message and continues the work.'}
];
let apps = [];
let activeFilter = 'all';
let catalogReady = false;
let previousFocus;
let toastTimer;
const expanded = new Set(['declarative']);

function toast(message) {
  clearTimeout(toastTimer);
  $('#toast').textContent = message;
  $('#toast').classList.add('visible');
  toastTimer = setTimeout(()=>$('#toast').classList.remove('visible'), 2600);
}
async function copyText(text, button) {
  try {
    if (navigator.clipboard?.writeText) await navigator.clipboard.writeText(text);
    else {
      const field = document.createElement('textarea');
      field.value = text;
      field.className = 'clipboard-fallback';
      (appDialog.open ? appDialog : document.body).append(field);
      field.select();
      const copied = document.execCommand('copy');
      field.remove();
      if (!copied) throw new Error('Clipboard unavailable');
    }
    toast('Copied to clipboard');
    if (button) {
      const label = button.querySelector('span');
      if (label) { label.textContent = 'Copied'; setTimeout(()=>label.textContent='Copy', 1800); }
    }
  } catch {
    toast('Clipboard unavailable. Select and copy the command directly.');
  }
}
function appCard(app) {
  const p = presentationFor(app);
  return `<button type="button" class="app-card" data-app="${esc(app.id)}" aria-label="View ${esc(p.title)} details"><span class="app-card-top"><span class="app-icon ${p.color}">${icon(p.icon)}</span><span class="app-version">v${esc(app.version)}</span></span><h3>${esc(p.title)}</h3><p>${esc(p.short)}</p><span class="app-card-bottom"><span>${icon(app.publisher==='rhyven'?'layers':'box')}${app.publisher==='rhyven'?'Rhyven app':esc(app.publisher)+' app'}</span><span class="open-card-icon">${icon('arrow-up')}</span></span></button>`;
}
function renderCatalog() {
  if (!catalogReady) return;
  const search = $('#app-search').value.trim().toLowerCase();
  const words = search.split(/\s+/).filter(Boolean);
  let found = 0;
  const markup = Object.entries(groups).map(([kind, group])=> {
    if (activeFilter !== 'all' && activeFilter !== kind) return '';
    const matches = apps.filter(app => app.kind === kind && words.every(word => `${app.id} ${app.description} ${appPresentation[app.name]?.title || ''} ${app.objects.join(' ')} ${app.actions.map(a=>a.name).join(' ')}`.toLowerCase().includes(word)));
    found += matches.length;
    if (!matches.length) return '';
    const open = search || activeFilter !== 'all' || expanded.has(kind);
    return `<details class="app-group" data-kind="${kind}" ${open?'open':''}><summary><span class="group-icon ${kind}">${icon(group.icon)}</span><span class="group-title">${group.title}<span>${String(matches.length).padStart(2,'0')}</span></span><span class="group-subtitle">${group.subtitle}</span><svg class="icon group-chevron" aria-hidden="true"><use href="#i-chevron"/></svg></summary><div class="group-content"><div class="group-description"><span>${group.description}</span><a href="#docs/app-types">How it works <span aria-hidden="true">↗</span></a></div><div class="app-grid">${matches.map(appCard).join('')}</div></div></details>`;
  }).join('');
  $('#app-groups').innerHTML = markup || `<div class="empty-state"><h3>No apps match that search.</h3><p>Search by app name or function, such as knowledge, code or messaging.</p><button type="button" class="button button-small" data-reset-search>Clear filters</button></div>`;
  $('#catalog-status').textContent = `${found} ${found===1?'app':'apps'} found${search?' for '+search:''}.`;
  $$('.app-group').forEach(details=>details.addEventListener('toggle', ()=> {
    if (search || activeFilter !== 'all') return;
    details.open ? expanded.add(details.dataset.kind) : expanded.delete(details.dataset.kind);
  }));
}
async function loadCatalog() {
  try {
    const response = await fetch('./data/catalog.json');
    if (!response.ok) throw new Error('Catalog request failed');
    const catalog = await response.json();
    // Catalog values become shell arguments when a visitor copies installation instructions.
    if (!Array.isArray(catalog.apps) || !catalog.apps.every(validInstallMetadata)) {
      throw new Error('Invalid catalog installation metadata');
    }
    apps = catalog.apps;
    const order = ['work-management','project-knowledge','error-management','ci-management','inventory','repo-documentation-tool','messaging'];
    apps.sort((a,b)=>order.indexOf(a.name)-order.indexOf(b.name));
    catalogReady = true;
    $('#app-count').textContent = apps.length;
    $$('[data-runtime-version]').forEach(el=>el.textContent=catalog.runtime_version);
    renderCatalog();
  } catch {
    $('#app-groups').innerHTML = `<div class="empty-state"><h3>The app catalog could not be loaded.</h3><p>Please try again. If the problem continues, contact support@rhyvenai.com.</p><button type="button" class="button button-small" data-retry-catalog>Try again</button></div>`;
    $('#catalog-status').textContent = 'The app catalog could not be loaded.';
  }
}
function showApp(id, trigger) {
  const app = apps.find(a=>a.id===id);
  if (!app) return;
  const p = presentationFor(app);
  const command = app.registry
    ? `rhyven --collection my-project registry-sync ${app.registry} --anonymous\nrhyven --collection my-project inspect ${app.id}\nrhyven --collection my-project install ${app.id} --accept-permissions${app.kind==='service'?'\nrhyven daemon start':''}`
    : null;
  const installation = command
    ? `<h3>Install from the marketplace</h3><p>Requires Rhyven 0.4.0-rc.6 or later. Sync downloads app manifests for local browsing; it does not install apps or pull container images. Review the permissions above before running the install command.</p>${codeBlock(command)}<p class="detail-notice">Public packages from <a href="https://github.com/${esc(app.registry)}">${esc(app.registry)}</a>. The runtime retrieves current repository stars when you refresh the marketplace. ${app.kind==='declarative'?'No Docker required.':'A compatible Docker engine is required.'} Nothing is installed by this website.</p>`
    : '<h3>Publication pending</h3><p>This app version is being prepared for publication and is not yet available from the public registry. The setup guide describes its requirements.</p>';
  $('#app-dialog-body').innerHTML = `<div class="dialog-app-heading"><span class="app-icon ${p.color}">${icon(p.icon)}</span><div><h2 id="app-dialog-title">${esc(p.title)}</h2><p>${esc(app.id)} · v${esc(app.version)}</p></div></div><p class="dialog-description">${esc(app.description)}</p><div class="detail-facts"><div><span>EXECUTION</span><strong>${esc(groups[app.kind].title)}</strong></div><div><span>DATA</span><strong>Your environment</strong></div><div><span>PUBLISHER</span><strong>${app.publisher==='rhyven'?'Rhyven':esc(app.publisher)}</strong></div></div><section class="detail-section"><h3>Requested permissions</h3><div class="detail-tags">${app.permissions.map(x=>`<code>${esc(x)}</code>`).join('')}</div></section>${app.objects.length?`<section class="detail-section"><h3>Structured objects</h3><div class="detail-tags">${app.objects.map(x=>`<code>${esc(x)}</code>`).join('')}</div></section>`:''}<section class="detail-section"><h3>App actions</h3><ul>${app.actions.map(action=>`<li><strong>${esc(action.name)}</strong> — ${esc(action.description)}</li>`).join('')}</ul><details><summary>Read the agent guidance</summary><p>${esc(app.guide)}</p></details></section><section class="detail-section">${installation}</section><div class="dialog-actions"><a class="button button-primary" href="#docs/${app.kind==='service'?'services':'build-an-app'}">Read the setup guide ${icon('arrow')}</a><button class="button" type="button" data-close-dialog>Back to apps</button></div>`;
  previousFocus = trigger;
  appDialog.showModal();
  appDialog.scrollTop = 0;
}
function renderWorkflow(index) {
  const step = workflow[index];
  $$('.workflow-step').forEach(button=> {
    const selected = Number(button.dataset.step)===index;
    button.classList.toggle('active',selected);
    button.setAttribute('aria-pressed',String(selected));
  });
  const args = JSON.stringify(step.args,null,2).split('\n').map(x=>'    '+x).join('\n');
  $('#workflow-output').innerHTML = `<span class="tiny-label">${String(index+1).padStart(2,'0')} / EXAMPLE AGENT CALL</span><pre><code><span class="code-fn">rhyven_call</span>({
  <span class="code-key">category</span>: <span class="code-value">"${esc(step.category)}"</span>,
  <span class="code-key">function</span>: <span class="code-value">"${esc(step.function)}"</span>,
  <span class="code-key">args</span>:
<span class="code-value">${esc(args)}</span>
})</code></pre><div class="workflow-result">${icon('check')}<div><strong>${step.title}</strong>${step.body}</div></div>`;
}
function renderDoc(id) {
  const doc = docs.find(d=>d.id===id) || docs[0];
  $('#docs-search').value = '';
  $('#docs-nav').innerHTML = docs.map(d=>`<a href="#docs/${d.id}" ${d.id===doc.id?'aria-current="page"':''}>${icon(d.icon)}${d.nav}</a>`).join('');
  const index = docs.indexOf(doc);
  const next = docs[(index+1)%docs.length];
  $('#docs-article').innerHTML = `<div class="doc-breadcrumb"><a href="#docs">Docs</a>${icon('chevron')}<span>${doc.nav}</span></div><h2 class="docs-article-title">${doc.title}</h2><p class="doc-lead">${doc.lead}</p><div class="doc-body">${doc.body}</div><a class="doc-next" href="#docs/${next.id}"><div><span>CONTINUE READING</span>${next.nav}</div>${icon('arrow')}</a>`;
  $('#docs-search-status').textContent = '';
}
function searchDocs() {
  const input = $('#docs-search').value.trim();
  if (!input) { renderDoc(location.hash.split('/')[1] || docs[0].id); return; }
  const words = input.toLowerCase().split(/\s+/);
  const matched = docs.filter(doc=>words.every(word=>`${doc.title} ${doc.nav} ${doc.lead} ${doc.body.replace(/<[^>]*>/g,' ')}`.toLowerCase().includes(word)));
  $('#docs-article').innerHTML = `<div class="doc-breadcrumb">DOCS / SEARCH</div><h2 class="docs-article-title">Documentation search results</h2><p class="doc-lead">${matched.length} ${matched.length===1?'topic':'topics'} for “${esc(input)}”</p>${matched.length?matched.map(doc=>`<a class="doc-search-result" href="#docs/${doc.id}">${icon('arrow')}<h3>${doc.nav}</h3><p>${doc.lead}</p></a>`).join(''):'<div class="empty-state"><h3>No matching topics.</h3><p>Try collections, permissions, publishing or Docker.</p></div>'}`;
  $('#docs-search-status').textContent = `${matched.length} matching documentation topics.`;
}
function route({initial=false}={}) {
  clearTimeout(toastTimer);
  $('#toast').classList.remove('visible');
  const [requested, sub, format] = (location.hash.slice(1) || 'marketplace').split('/');
  const page = ['marketplace','corvid','razorback','docs','skills'].includes(requested)?requested:'marketplace';
  if (appDialog.open) appDialog.close();
  $$('.page').forEach(el=>el.hidden=el.id!==`page-${page}`);
  $$('[data-page-link]').forEach(link=> {
    if (link.dataset.pageLink===page) link.setAttribute('aria-current','page');
    else link.removeAttribute('aria-current');
  });
  document.title = page==='marketplace'?'Rhyven — Agent first apps for a user first experience':`${page==='docs'?'Docs':page[0].toUpperCase()+page.slice(1)} — Rhyven`;
  if (page==='docs') renderDoc(sub || 'getting-started');
  if (page==='skills') renderSkill(sub,format);
  const anchor = page==='marketplace'&&sub==='apps'?$('#app-library'):page==='corvid'&&sub==='vision'?$('#corvid-vision'):null;
  if (!initial) $('#main').focus({preventScroll:true});
  requestAnimationFrame(()=> {
    if (anchor) anchor.scrollIntoView({behavior:initial?'instant':'smooth',block:'start'});
    else window.scrollTo({top:0,behavior:'instant'});
  });
}

document.addEventListener('click',event=> {
  const card = event.target.closest('[data-app]');
  if (card) showApp(card.dataset.app,card);
  const close = event.target.closest('[data-close-dialog]');
  if (close) appDialog.close();
  const filter = event.target.closest('[data-filter]');
  if (filter) {
    activeFilter = filter.dataset.filter;
    $$('.filter').forEach(el=> { const selected=el===filter; el.classList.toggle('active',selected); el.setAttribute('aria-pressed',String(selected)); });
    renderCatalog();
  }
  if (event.target.closest('[data-reset-search]')) {
    $('#app-search').value='';
    $('[data-filter="all"]').click();
    $('#app-search').focus();
  }
  if (event.target.closest('[data-retry-catalog]')) loadCatalog();
  const step = event.target.closest('[data-step]');
  if (step) renderWorkflow(Number(step.dataset.step));
  const copy = event.target.closest('[data-copy-code]');
  if (copy) copyText(copy.closest('.code-block').querySelector('code').textContent,copy);
  const copySkill = event.target.closest('[data-copy-skill]');
  if (copySkill) copyText($('.skill-source code').textContent,copySkill);
  if (event.target.closest('[data-retry-skill]')) {
    const [,id,format] = location.hash.split('/');
    renderSkill(id,format);
  }
  const link = event.target.closest('a[href^="#"]');
  if (link?.getAttribute('href') === '#main') {
    event.preventDefault();
    $('#main').focus();
    return;
  }
  if (link && appDialog.contains(link)) {
    previousFocus = null;
    appDialog.close();
  }
  if (link && link.getAttribute('href')===location.hash) {
    if (link.getAttribute('href').startsWith('#docs')) $('#docs-search').value='';
    route();
  }
});
appDialog.addEventListener('click',event=> {
  if (event.target!==appDialog) return;
  const rect=appDialog.getBoundingClientRect();
  if (event.clientX<rect.left||event.clientX>rect.right||event.clientY<rect.top||event.clientY>rect.bottom) appDialog.close();
});
appDialog.addEventListener('close',()=>previousFocus?.isConnected&&previousFocus.focus({preventScroll:true}));
$('#app-search').addEventListener('input',renderCatalog);
$('#docs-search').addEventListener('input',searchDocs);
window.addEventListener('hashchange',()=>route());
document.addEventListener('keydown',event=> {
  if (event.key==='/' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.target.matches('input,textarea,[contenteditable="true"]') && !appDialog.open) {
    const page=location.hash.slice(1).split('/')[0];
    const field=page==='docs'?$('#docs-search'):page===''||page==='marketplace'?$('#app-search'):null;
    if (field) { event.preventDefault(); field.focus(); field.scrollIntoView({block:'center'}); }
  }
});
$('#home-install').innerHTML = codeBlock(`${installCommand}\n~/.local/bin/rhyven`, 'INSTALL AND OPEN');
$('#home-install [data-copy-code]').setAttribute('aria-label', 'Copy install and launch commands');
renderWorkflow(0);
route({initial:true});
loadCatalog();

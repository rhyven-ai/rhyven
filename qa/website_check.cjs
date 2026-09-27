/* Local website behavior and accessibility acceptance. No live runtime calls. */
const {chromium} = require('playwright');
const AxeBuilder = require('@axe-core/playwright').default;
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const base = process.env.RHYVEN_WEBSITE_URL || 'http://127.0.0.1:5173';
const output = '/tmp/rhyven-website-screenshots';

(async()=> {
  await fs.mkdir(output,{recursive:true});
  const browser = await chromium.launch({headless:true});
  const context = await browser.newContext({viewport:{width:1440,height:1000},permissions:['clipboard-read','clipboard-write'],reducedMotion:'reduce'});
  const page = await context.newPage();
  const errors=[], external=[], accessibility=[];
  page.on('pageerror',error=>errors.push(error.message));
  page.on('console',message=> { if (message.type()==='error' && /Content Security Policy|violates.*directive/i.test(message.text())) errors.push(message.text()); });
  page.on('request',req=> { if (!req.url().startsWith(base) && !req.url().startsWith('data:')) external.push(req.url()); });
  page.on('response',res=> { if (res.status()>=400) errors.push(`${res.status()} ${res.url()}`); });
  async function screenshot(name,fullPage=true) { await page.screenshot({path:path.join(output,name+'.png'),fullPage}); }
  async function audit(name) {
    const results=await new AxeBuilder({page}).withTags(['wcag2a','wcag2aa','wcag21aa']).analyze();
    accessibility.push({name,violations:results.violations.map(v=>({id:v.id,impact:v.impact,description:v.description,nodes:v.nodes.map(n=>({target:n.target,summary:n.failureSummary}))}))});
  }
  try {
    const response = await page.goto(base, {waitUntil:'networkidle'});
    assert.match(response.headers()['content-security-policy'] || '', /default-src 'none'/);
    assert.equal(response.headers()['x-content-type-options'], 'nosniff');
    await page.locator('.app-card').first().waitFor();
    await page.evaluate(()=>document.fonts.ready);
    assert.equal(await page.locator('.app-card').count(),7);
    assert.equal(await page.locator('.app-card-bottom').filter({hasText:'Rhyven app'}).count(),7);
    assert.doesNotMatch(await page.locator('#app-groups').textContent(),/Community app|official\//);
    assert.equal(await page.locator('.app-card:visible').count(),5);
    assert.equal(await page.locator('.top-nav a').allTextContents().then(x=>x.map(s=>s.trim()).join('|')), 'Marketplace|Corvid|Razorback|Docs|Skills');
    assert.match(await page.locator('#marketplace-title').textContent(),/^Agent first apps for a user first experience$/);
    await screenshot('marketplace-desktop');
    await audit('marketplace');

    const firstGroup=page.locator('.app-group[data-kind="declarative"]');
    await firstGroup.locator('summary').click();
    assert.equal(await firstGroup.getAttribute('open'),null);
    await firstGroup.locator('summary').focus();
    await page.keyboard.press('Enter');
    assert.notEqual(await firstGroup.getAttribute('open'),null);
    await page.getByRole('button',{name:'View Work Management details',exact:true}).click();
    assert.equal(await page.locator('#app-dialog').isVisible(),true);
    assert.match(await page.locator('#app-dialog-body').textContent(),/state.write/);
    assert.match(await page.locator('#app-dialog-body').textContent(),/PUBLISHERRhyven/);
    await page.locator('#app-dialog [data-copy-code]').click();
    assert.match(await page.evaluate(()=>navigator.clipboard.readText()),/registry-sync rhyven-ai\/registry --anonymous[\s\S]*install rhyven\/work-management/);
    await audit('app-details');
    await page.locator('#app-dialog').evaluate(el=>el.scrollTop=0);
    await screenshot('app-details',false);
    await page.keyboard.press('Escape');
    assert.equal(await page.locator('#app-dialog').isVisible(),false);
    assert.match(await page.locator(':focus').getAttribute('aria-label'),/Work Management/);

    await page.locator('#app-search').fill('repo documentation');
    assert.equal(await page.locator('.app-card:visible').count(),1);
    assert.match(await page.locator('.app-card:visible').textContent(),/Rhyven Repo Documentation Tool/);
    await page.locator('#app-search').fill('no-such-capability-xyz');
    assert.match(await page.locator('.empty-state').textContent(),/No apps match/);
    await page.locator('[data-reset-search]').click();
    await page.locator('[data-filter="service"]').click();
    assert.equal(await page.locator('.app-card:visible').count(),1);
    await page.getByRole('button',{name:'View Messaging details',exact:true}).click();
    assert.match(await page.locator('#app-dialog-body').textContent(),/Publication pending/);
    assert.equal(await page.locator('#app-dialog [data-copy-code]').count(),0);
    await page.locator('#app-dialog').getByRole('link',{name:'Read the setup guide'}).click();
    await page.waitForURL('**/#docs/services');
    await page.locator('#app-dialog').waitFor({state:'hidden'});
    await page.locator('#page-docs').waitFor({state:'visible'});
    assert.equal(await page.locator('#app-dialog').isVisible(),false);
    assert.match(await page.locator('.docs-article-title').textContent(),/Run persistent services/);

    await page.locator('.top-nav a[href="#corvid"]').click();
    await page.waitForURL('**/#corvid');
    await page.locator('.corvid-background').evaluate(img=>img.decode());
    assert.match(await page.locator('.corvid-background').getAttribute('src'),/corvid-crows-pixel\.png$/);
    assert.match(await page.locator('#page-corvid').textContent(),/IN PROGRESS/);
    assert.doesNotMatch(await page.locator('#page-corvid').textContent(),/planned|after the marketplace launch/i);
    await screenshot('corvid-desktop');
    await audit('corvid');
    await page.locator('.top-nav a[href="#razorback"]').click();
    await page.locator('#page-razorback').waitFor({state:'visible'});
    assert.equal(await page.locator('.caution-tape:visible').count(),2);
    assert.match(await page.locator('.coming-seal').textContent(),/COMING SOON/);
    await screenshot('razorback-desktop');
    await audit('razorback');

    await page.locator('.top-nav a[href="#skills"]').click();
    await page.locator('.skill-source').waitFor();
    const skillLinks=await page.locator('#skills-nav a').evaluateAll(links=>links.map(a=>a.getAttribute('href')));
    assert.equal(skillLinks.length,5);
    for (const link of skillLinks) {
      await page.locator(`#skills-nav a[href="${link}"]`).click();
      const id=link.split('/')[1];
      await page.waitForFunction(id=>document.querySelector('.skill-source code')?.textContent.startsWith('---\nname: '+id+'\n'),id);
      const source=await page.locator('.skill-source code').textContent();
      const local=await fs.readFile(path.join(__dirname,'../website/skills',id,'SKILL.md'),'utf8');
      assert.equal(source,local);
      assert.match(source,new RegExp('^---\\nname: '+id+'\\ndescription: '));
      const lines=source.trimEnd().split('\n').length;
      assert(lines>20 && lines<=500,`${id}: ${lines} lines`);
      assert.match(await page.locator('.skill-facts').textContent(),new RegExp(`${lines} / 500 lines`));
      assert.equal(await page.locator('#skills-nav [aria-current="page"]').getAttribute('href'),link);
      await page.locator('[data-copy-skill]').click();
      assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),source);
      const downloading=page.waitForEvent('download');
      await page.locator('#skill-reader a[download]').click();
      const downloaded=await downloading;
      assert.equal(downloaded.suggestedFilename(),'SKILL.md');
      assert.equal(await fs.readFile(await downloaded.path(),'utf8'),source);
      const raw=await page.request.get(base+'/skills/'+id+'/SKILL.md');
      assert.equal(raw.status(),200);
      assert.equal(await raw.text(),source);
      await audit('skills-'+id);
    }
    await page.goto(base+'/#skills/build-rhyven-container-app',{waitUntil:'networkidle'});
    assert.match(await page.locator('.skill-source').textContent(),/```dockerfile/);
    await screenshot('skills-desktop');
    await page.goBack({waitUntil:'networkidle'});
    assert.equal(await page.locator('#skills-nav [aria-current="page"]').getAttribute('href'),'#skills/build-rhyven-service-app');
    await page.goto(base+'/#skills/unknown-skill',{waitUntil:'networkidle'});
    assert.equal(await page.locator('#skills-nav [aria-current="page"]').getAttribute('href'),'#skills/use-rhyven');

    // Skill and rule keep separate source/cache entries and export exactly the preview.
    await page.getByRole('navigation',{name:'Instruction format'}).getByRole('link',{name:'Rule',exact:true}).click();
    await page.waitForURL('**/#skills/use-rhyven/rule');
    await page.waitForFunction(()=>document.querySelector('.skill-source code')?.textContent.startsWith('# Rhyven usage rule'));
    const rule=await fs.readFile(path.join(__dirname,'../website/skills/use-rhyven/RULE.md'),'utf8');
    assert.equal(await page.locator('.skill-source code').textContent(),rule);
    assert(rule.trimEnd().split('\n').length<=500);
    assert.equal(await page.locator('.skill-formats [aria-current="page"]').textContent(),'Rule');
    await page.locator('[data-copy-skill]').click();
    assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),rule);
    const ruleDownloading=page.waitForEvent('download');
    await page.locator('#skill-reader a[download]').click();
    const ruleDownload=await ruleDownloading;
    assert.equal(ruleDownload.suggestedFilename(),'RULE.md');
    assert.equal(await fs.readFile(await ruleDownload.path(),'utf8'),rule);
    const rawRule=await page.request.get(base+'/skills/use-rhyven/RULE.md');
    assert.equal(rawRule.status(),200);
    assert.equal(await rawRule.text(),rule);
    await page.reload({waitUntil:'networkidle'});
    assert.equal(await page.locator('.skill-source code').textContent(),rule);
    await screenshot('usage-rule-desktop');
    await audit('usage-rule');
    await page.getByRole('navigation',{name:'Instruction format'}).getByRole('link',{name:'Skill',exact:true}).click();
    await page.waitForFunction(()=>document.querySelector('.skill-source code')?.textContent.startsWith('---\nname: use-rhyven\n'));
    await page.goBack({waitUntil:'networkidle'});
    assert.equal(await page.locator('.skill-source code').textContent(),rule);

    // A failed fetch stays recoverable without copying partial content.
    const retryPage=await context.newPage();
    await retryPage.route('**/skills/use-rhyven/SKILL.md',route=>route.fulfill({status:503,body:'Unavailable'}));
    await retryPage.goto(base+'/#skills');
    await retryPage.getByRole('alert').waitFor();
    assert.equal(await retryPage.locator('[data-copy-skill]').count(),0);
    await retryPage.unroute('**/skills/use-rhyven/SKILL.md');
    await retryPage.locator('[data-retry-skill]').click();
    await retryPage.locator('.skill-source').waitFor();
    await retryPage.route('**/skills/use-rhyven/RULE.md',route=>route.fulfill({status:503,body:'Unavailable'}));
    await retryPage.getByRole('navigation',{name:'Instruction format'}).getByRole('link',{name:'Rule',exact:true}).click();
    await retryPage.getByRole('alert').waitFor();
    assert.equal(await retryPage.locator('[data-copy-skill]').count(),0);
    await retryPage.unroute('**/skills/use-rhyven/RULE.md');
    await retryPage.locator('[data-retry-skill]').click();
    await retryPage.locator('.skill-source').waitFor();
    assert.equal(await retryPage.locator('.skill-source code').textContent(),rule);
    await retryPage.close();

    await page.locator('.top-nav a[href="#docs"]').click();
    await page.locator('#docs-search').fill('SQLite');
    assert((await page.locator('.doc-search-result').count())>=2);
    await page.locator('#docs-search').fill('unknown-topic-zzz');
    assert.match(await page.locator('#docs-article').textContent(),/No matching topics/);
    await page.locator('#docs-search').fill('publishing');
    await page.locator('.doc-search-result').first().click();
    await page.locator('.doc-body').waitFor({state:'visible'});
    assert.equal(await page.locator('#docs-search').inputValue(),'');
    const topics=await page.locator('#docs-nav a').evaluateAll(links=>links.map(a=>a.getAttribute('href')));
    assert.equal(topics.length,13);
    await page.goto(base+'/#docs/getting-started',{waitUntil:'networkidle'});
    const installDocs=await fs.readFile(path.join(__dirname,'../docs/installation.md'),'utf8');
    const installCommand=installDocs.match(/```bash\n(curl -fsSL[^\n]+)\n```/)[1];
    const installer=page.locator('.doc-body .code-block').first();
    assert.equal(await installer.locator('code').textContent(),installCommand);
    await installer.locator('[data-copy-code]').click();
    assert.equal(await page.evaluate(()=>navigator.clipboard.readText()),installCommand);
    assert.match(await page.locator('.doc-body').textContent(),/Domain installer not published yet/);
    assert.doesNotMatch(await page.locator('.doc-body').textContent(),/gh auth login|Build from source|cargo build/);
    assert.equal(await page.locator('.doc-body .code-block').nth(1).locator('code').textContent(),'rhyven');
    await page.evaluate(()=>window.scrollTo(0,0));
    await screenshot('getting-started-desktop');
    for (const topic of topics) {
      await page.goto(base+'/'+topic,{waitUntil:'networkidle'});
      assert((await page.locator('.doc-body').textContent()).length>500,topic);
      assert.equal(await page.locator('#docs-nav [aria-current="page"]').getAttribute('href'),topic);
      const links=await page.locator('.doc-body a').evaluateAll(a=>a.map(x=>x.getAttribute('href')));
      const publicLinks = new Set(['https://github.com/rhyven-ai/registry', 'https://github.com/rhyven-ai/apps', 'https://github.com/rhyven-ai/rhyven', 'https://github.com/rhyven-ai/rhyven/blob/main/LICENSE', 'https://github.com/rhyven-ai/rhyven/blob/main/CONTRIBUTING.md', 'https://github.com/rhyven-ai/rhyven/security/advisories/new', 'https://github.com/rhyven-ai/registry/releases/tag/v0.4.0-rc.6']);
      assert(links.every(link=>link.startsWith('#') || publicLinks.has(link)),'unexpected external docs link');
    }
    await page.goto(base+'/#docs/agent-interface',{waitUntil:'networkidle'});
    await page.locator('.skip-link').focus();
    await page.keyboard.press('Enter');
    assert.equal(await page.locator(':focus').getAttribute('id'),'main');
    assert.equal(new URL(page.url()).hash,'#docs/agent-interface');
    await page.keyboard.press('/');
    assert.equal(await page.locator(':focus').getAttribute('id'),'docs-search');
    await page.locator('#main').focus();
    await screenshot('docs-desktop');
    await audit('docs');
    await page.goto(base+'/#marketplace',{waitUntil:'networkidle'});
    await page.locator('[data-step="1"]').click();
    assert.match(await page.locator('#workflow-output').textContent(),/action_remember/);
    await page.locator('[data-step="2"]').click();
    assert.match(await page.locator('#workflow-output').textContent(),/action_send_direct/);
    await page.goto(base+'/#corvid',{waitUntil:'networkidle'});
    await page.goBack({waitUntil:'networkidle'});
    assert.equal(await page.locator('#page-marketplace').isVisible(),true);

    for (const width of [390,768]) {
      await page.setViewportSize({width,height:844});
      for (const section of ['marketplace','corvid','razorback','docs/build-an-app','skills/build-rhyven-declarative-app','skills/use-rhyven','skills/use-rhyven/rule']) {
        await page.goto(base+'/#'+section,{waitUntil:'networkidle'});
        if (section==='marketplace') await page.locator('[data-filter="all"]').click();
        const size=await page.evaluate(()=>({scroll:document.documentElement.scrollWidth,client:document.documentElement.clientWidth}));
        assert(size.scroll<=size.client+1,`Overflow at ${width}px ${section}: ${JSON.stringify(size)}`);
        await screenshot(section.replaceAll('/','-')+'-'+width);
        if (width===390) await audit(section+'-mobile');
      }
    }
    await page.setViewportSize({width:390,height:844});
    for (const topic of topics) {
      await page.goto(base+'/'+topic,{waitUntil:'networkidle'});
      assert(await page.evaluate(()=>document.documentElement.scrollWidth<=document.documentElement.clientWidth+1),'Mobile docs overflow: '+topic);
      await audit(topic+'-mobile');
    }
    assert.deepEqual(errors,[],'Browser errors');
    assert.deepEqual(external,[],'External requests');
    await fs.writeFile(path.join(output,'accessibility.json'),JSON.stringify(accessibility,null,2));
    const failures=accessibility.filter(a=>a.violations.length);
    assert.deepEqual(failures,[],'Accessibility violations; see accessibility.json');
    console.log('PASS: all five pages; five skills and a usage rule with full copy/download, format switching, line limits, deep links and retry; seven real apps; filters/search/accordions; accessible details and clipboard; thirteen docs topics and search; workflow controls; back navigation; 390/768/1440 layouts; no external requests; WCAG A/AA automated checks.');
    console.log('Screenshots: '+output);
  } finally { await browser.close(); }
})().catch(error=> { console.error(error); process.exitCode=1; });

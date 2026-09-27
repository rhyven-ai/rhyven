/* Adversarial browser checks using synthetic responses; never executes copied commands. */
const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const {chromium} = require('playwright');
const base = process.env.RHYVEN_WEBSITE_URL || 'http://127.0.0.1:5173';
const address = process.env.RHYVEN_WEBSITE_IP;
const payload = '<img src="https://example.invalid/audit" onerror="window.auditInjected=true"><script>window.auditInjected=true</script>';

(async()=> {
  const catalog = JSON.parse(await fs.readFile(path.join(__dirname,'../website/data/catalog.json'),'utf8'));
  const browser = await chromium.launch({headless:true,
    args:address?[`--host-resolver-rules=MAP ${new URL(base).hostname} ${address}`]:[]});
  const context = await browser.newContext({permissions:['clipboard-read','clipboard-write']});
  const external = [], errors = [];
  context.on('request', request=> {
    if (new URL(request.url()).origin!==new URL(base).origin) external.push(request.url());
  });
  async function pageWithCatalog(data) {
    const page = await context.newPage();
    page.on('pageerror', error=>errors.push(error.message));
    await page.route('**/data/catalog.json', route=>route.fulfill({json:data}));
    return page;
  }
  try {
    const page = await pageWithCatalog(catalog);
    const response = await page.goto(base,{waitUntil:'networkidle'});
    const headers = response.headers();
    assert.match(headers['content-security-policy'],/default-src 'none'/);
    assert.match(headers['content-security-policy'],/frame-ancestors 'none'/);
    assert.doesNotMatch(headers['content-security-policy'],/unsafe-inline|unsafe-eval/);
    assert.equal(headers['x-content-type-options'],'nosniff');
    assert.equal(headers['x-frame-options'],'DENY');
    assert.equal(headers['referrer-policy'],'no-referrer');
    assert.match(headers['cache-control'],/no-transform/);
    assert.equal(headers['set-cookie'],undefined);
    await page.goto(base+'/#docs');
    await page.locator('#docs-search').fill(payload);
    assert((await page.locator('#docs-article').textContent()).includes(payload));
    assert.equal(await page.locator('#docs-article img, #docs-article script').count(),0);
    await page.goto(base+'/#marketplace');
    await page.locator('#app-search').fill(payload);
    assert.equal(await page.locator('#app-groups img, #app-groups script').count(),0);
    await page.goto(base+'/#skills/../../.env');
    await page.locator('.skill-source').waitFor();
    assert.equal(await page.locator('#skills-nav [aria-current]').getAttribute('href'),'#skills/use-rhyven');
    await page.close();

    const hostile = structuredClone(catalog);
    const app = hostile.apps[0];
    app.id='example/constructor';app.name='constructor';app.publisher=payload;
    app.description=payload;app.guide=payload;app.permissions=[payload];app.objects=[payload];
    app.actions=[{name:payload,description:payload,input:{}}];
    const escaped = await pageWithCatalog(hostile);
    await escaped.goto(base);
    await escaped.locator('[data-app="example/constructor"]').click();
    assert((await escaped.locator('.dialog-description').textContent()).includes(payload));
    assert.equal(await escaped.locator('#app-groups img, #app-groups script, #app-dialog img, #app-dialog script').count(),0);
    await escaped.locator('#app-dialog [data-copy-code]').click();
    assert.match(await escaped.evaluate(()=>navigator.clipboard.readText()),/install example\/constructor --accept-permissions/);
    assert.equal(await escaped.evaluate(()=>window.auditInjected),undefined);
    await escaped.close();

    const attacks = ['; printf AUDIT_MARKER','\nprintf AUDIT_MARKER','$(printf AUDIT_MARKER)',
      '`printf AUDIT_MARKER`',' --help','\n','\r','\u2028','\u0000',"'quoted'"];
    for (const field of ['id','registry']) {
      for (const suffix of attacks) {
        const bad = structuredClone(catalog);
        bad.apps[0][field]+=suffix;
        if (field==='id') bad.apps[0].name=bad.apps[0].id.split('/')[1];
        const rejected = await pageWithCatalog(bad);
        await rejected.goto(base);
        await rejected.locator('[data-retry-catalog]').waitFor();
        assert.equal(await rejected.locator('.app-card').count(),0,`${field}: ${JSON.stringify(suffix)}`);
        assert.equal(await rejected.locator('#app-dialog [data-copy-code]').count(),0);
        await rejected.close();
      }
    }

    const skill = await context.newPage();
    await skill.route('**/skills/use-rhyven/SKILL.md',route=>route.fulfill({body:payload,contentType:'text/plain'}));
    await skill.goto(base+'/#skills');
    await skill.locator('.skill-source').waitFor();
    assert.equal(await skill.locator('.skill-source code').textContent(),payload);
    assert.equal(await skill.locator('#skill-reader img, #skill-reader script').count(),0);
    assert.deepEqual(await context.cookies(),[]);
    assert.equal(await skill.evaluate(()=>localStorage.length+sessionStorage.length),0);
    assert.deepEqual(external,[],'Unexpected external requests');
    assert.deepEqual(errors,[],'Browser errors');
    console.log('PASS: security headers; search/catalog/Markdown escaping; allowlisted skill routing; 20 shell injection attempts rejected; normal install copy preserved; no cookies, storage or external requests.');
  } finally { await browser.close(); }
})().catch(error=> {console.error(error);process.exitCode=1;});

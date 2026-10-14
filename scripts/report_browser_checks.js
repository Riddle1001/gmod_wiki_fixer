(async () => {
  const results = [];
  window.__reportQAProgress = results;
  const check = (name, passed, detail = '') => results.push({time: performance.now(), name, passed: Boolean(passed), detail});
  const wait = () => new Promise(resolve => setTimeout(resolve, 75));
  const $ = id => document.getElementById(id);
  const route = async key => { location.hash = key; await wait(); };
  const records = [...document.querySelectorAll('#source-data tr[data-category]')];
  const nameIn = row => row.cells[row.cells.length === 5 || row.cells.length === 4 ? 1 : 0].querySelector('a')?.textContent;
  const pageCount = rows => new Set(rows.map(nameIn)).size;
  const cards = () => [...document.querySelectorAll('.finding-card')];
  check('capture canary', true, 'Browser assertions live; first-pass, no retries');
  check('default view is fixes', $('queue-title').textContent === 'Fixes');
  check('audit completion timestamp shown', $('audit-completed')?.dateTime && !$('audit-completed').textContent.includes('Unix'));
  const refresh = document.querySelector('.refresh-details');
  check('refresh schedule collapsed with next time', refresh && !refresh.open && $('refresh-summary').textContent.startsWith('Next:'));
  refresh.querySelector('summary').click(); await wait();
  check('refresh clocks accessible on expansion', refresh.open && $('next-refresh-local').dateTime && $('live-clock').dateTime);
  refresh.querySelector('summary').click(); await wait();
  check('progressive enhancement initialized', document.documentElement.classList.contains('js'));
  const views = {
    fixes: ['dead-files', 'realm-mismatches', 'malformed-source', 'identity', 'signature', 'returns', 'broken-link'],
    maintenance: ['line-ranges', 'wrong-files', 'missing-source', 'missing-realms'],
    opportunities: ['missing', 'missing-controls', 'examples'],
    diagnostics: ['unresolved', 'failed', 'unverified-link', 'load-realm']
  };
  for (const key of [...Object.keys(views), ...new Set(records.map(row => row.dataset.category)), 'all']) {
    await route(key);
    const expected = key === 'all' ? records : records.filter(row => (views[key] || [key]).includes(row.dataset.category));
    check('grouped page count: ' + key, cards().length === Math.min(pageCount(expected), 50));
    check('view finding count: ' + key, Number($('queue-total').textContent.replace(/[^0-9]/g, '')) === expected.length);
    if (key === 'all') {
      const actual = cards().map(card => card.dataset.entry);
      const expectedOrder = [...new Set(records.map(nameIn))].sort((a,b) => {
        const av = Object.hasOwn(pageViews, a) ? BigInt(pageViews[a]) : null;
        const bv = Object.hasOwn(pageViews, b) ? BigInt(pageViews[b]) : null;
        if (av !== bv) { if (av === null) return 1; if (bv === null) return -1; return av > bv ? -1 : 1; }
        return new Intl.Collator(undefined, {numeric:true, sensitivity:'base'}).compare(a,b);
      }).slice(0,50);
      check('priority uses page views highest first', JSON.stringify(actual) === JSON.stringify(expectedOrder));
      check('page views shown on cards', cards().every(card => card.querySelector('.finding-preview').textContent.startsWith('Page views:')));
    }
    check('every grouped finding preserved: ' + key, cards().reduce((sum, card) => sum + card.querySelectorAll('.finding-body').length, 0) === expected.filter(row => cards().some(card => card.dataset.entry === nameIn(row))).length);
  }
  await route('all');
  check('desktop does not overflow', document.documentElement.scrollWidth <= innerWidth);
  if (records.length) {
    const summary = document.querySelector('.finding-summary'); summary.click(); await wait();
    check('page opens in place with evidence and wiki action', summary.parentElement.open && summary.parentElement.querySelector('.wiki-action'));
    $('expand-all').click(); await wait();
    check('expand page', cards().every(card => card.open));
    $('expand-all').click(); await wait();
    check('collapse page', cards().every(card => !card.open));
    if (pageCount(records) > 50) {
      const first = cards()[0].dataset.entry; $('next').click(); await wait();
      check('next page changes records', $('page-position').textContent.includes('Page 2') && cards()[0].dataset.entry !== first);
      $('previous').click(); await wait();
      check('previous restores records', cards()[0].dataset.entry === first);
    }
    $('sort').value = 'page'; $('sort').dispatchEvent(new Event('change')); await wait();
    const names = cards().map(card => card.querySelector('h3').textContent);
    const collator = new Intl.Collator(undefined, {numeric:true, sensitivity:'base'});
    check('page sorting', names.every((name, i) => i === 0 || collator.compare(names[i-1], name) <= 0));
    $('search').value = 'never_matching_qa_918362'; $('search').dispatchEvent(new Event('input')); await wait();
    check('empty search state', !$('empty-state').hidden && !cards().length && $('next').disabled);
    $('reset').click(); await wait();
    check('clear restores focused default', $('queue-title').textContent === 'Fixes' && !$('hide-base').checked && !$('hide-panels').checked && !$('search').value);
    $('search').blur(); document.body.dispatchEvent(new KeyboardEvent('keydown', {key:'/',bubbles:true}));
    check('keyboard search shortcut', document.activeElement === $('search'));
  }
  await route('all');
  $('hide-base').click(); $('hide-panels').click(); await wait();
  check('combined filters exclude base and panels', cards().every(card => !/gamemodes[\\/]base[\\/]/i.test(card.textContent) && !card.querySelector('a[href*="/vgui/"],a[href*="/derma/"]')));
  for (const key of Object.keys(views)) {
    await route(key);
    check('filters persist across views: ' + key, $('hide-base').checked && $('hide-panels').checked);
  }
  $('search').value = 'Shared & symbols'; $('search').dispatchEvent(new Event('input')); await wait();
  const sharedUrl = location.href;
  check('URL retains filters and encoded search', new URL(sharedUrl).searchParams.get('q') === 'Shared & symbols' && new URL(sharedUrl).searchParams.get('base') === 'hide' && new URL(sharedUrl).searchParams.get('panels') === 'hide');
  const frame = document.createElement('iframe');
  frame.style.cssText = 'position:fixed;left:-2000px;top:0;width:390px;height:844px;border:0';
  const loaded = new Promise(resolve => frame.onload = resolve);
  frame.src = sharedUrl; document.body.append(frame); await loaded;
  const mobile = frame.contentWindow, doc = frame.contentDocument;
  check('shared URL restores state', doc.getElementById('search').value === 'Shared & symbols' && doc.getElementById('hide-base').checked && doc.getElementById('hide-panels').checked && doc.getElementById('queue-title').textContent === 'Audit diagnostics');
  check('mobile does not overflow', doc.documentElement.scrollWidth <= mobile.innerWidth);
  check('mobile category selector available', mobile.getComputedStyle(doc.querySelector('.mobile-filter')).display !== 'none');
  doc.getElementById('reset').click(); await wait();
  const picker = doc.getElementById('category-picker'); picker.value = 'maintenance'; picker.dispatchEvent(new mobile.Event('change')); await wait();
  check('mobile source maintenance navigation', doc.getElementById('queue-title').textContent === 'Source maintenance');
  if (doc.querySelector('.finding-summary')) { doc.querySelector('.finding-summary').click(); await wait(); check('mobile expansion does not overflow', doc.documentElement.scrollWidth <= mobile.innerWidth); }
  frame.remove();
  $('reset').click(); await wait();
  await route('coverage');
  check('coverage route', !$('coverage').hidden && $('review').hidden && document.querySelectorAll('table.coverage tbody tr').length > 0);
  const fallback = document.createElement('iframe');
  fallback.style.cssText = 'position:fixed;left:-3000px;top:0;width:800px;height:600px;border:0';
  const raw = await (await fetch(location.href.split('#')[0])).text();
  const fallbackLoaded = new Promise(resolve => fallback.onload = resolve);
  fallback.srcdoc = raw.replace(/<script>[\s\S]*?<\/script>/g, ''); document.body.append(fallback); await fallbackLoaded;
  check('no-JS retains complete record', fallback.contentDocument.querySelectorAll('tr[data-category]').length === records.length && fallback.contentWindow.getComputedStyle(fallback.contentDocument.getElementById('source-data')).display !== 'none');
  fallback.remove();
  check('no external assets', document.querySelectorAll('script[src],link[rel=stylesheet]').length === 0);
  check('untrusted markup remains text', document.querySelectorAll('.finding-card script,.finding-card img').length === 0);
  await route('fixes'); $('sort').value = 'priority'; $('sort').dispatchEvent(new Event('change')); $('search').blur(); scrollTo(0,0);
  return {passed:results.every(result => result.passed), count:records.length, results};
})()

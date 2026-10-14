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
  const cards = () => [...document.querySelectorAll('.page-rows')];
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
    fixes: ['dead-files', 'realm-mismatches', 'malformed-source', 'identity', 'signature', 'returns', 'broken-link', 'doc-critical'],
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
      check('page views shown on cards', cards().every(card => card.querySelector('.views')));
    }
    check('every grouped finding preserved: ' + key, cards().reduce((sum, card) => sum + card.querySelectorAll('.finding-row').length, 0) === expected.filter(row => cards().some(card => card.dataset.entry === nameIn(row))).length);
  }
  await route('all');
  check('desktop does not overflow', document.documentElement.scrollWidth <= innerWidth);
  const qualityRows = [...document.querySelectorAll('#quality-data tr[data-category]')];
  await route('documentation-quality');
  check('quality is separate with transparent controls', !$('quality-controls').hidden && $('quality-summary').textContent.includes('rated pages') && $('quality-controls').textContent.includes('log(1 + views)'));
  $('quality-band').value = 'all'; $('quality-band').dispatchEvent(new Event('input')); await wait();
  check('all rated pages are accessible', cards().length === Math.min(50, qualityRows.length));
  const maxLog = qualityRows.reduce((max, row) => Math.max(max, Math.log1p(Number(pageViews[nameIn(row)] || 0))), 0);
  for (const [need, viewsWeight] of [[100,0],[0,100],[60,40],[0,0]]) {
    $('need-weight').value = need; $('views-weight').value = viewsWeight;
    $('views-weight').dispatchEvent(new Event('input')); await wait();
    const n = need + viewsWeight ? need : 60, v = need + viewsWeight ? viewsWeight : 40;
    const priority = row => (n * (100 - Number(row.dataset.quality)) + v * (maxLog ? 100 * Math.log1p(Number(pageViews[nameIn(row)] || 0)) / maxLog : 0)) / (n+v);
    const expected = [...qualityRows].sort((a,b) => priority(b)-priority(a) || new Intl.Collator(undefined,{numeric:true,sensitivity:'base'}).compare(nameIn(a),nameIn(b))).slice(0,50).map(nameIn);
    check(`quality priority ${need}/${viewsWeight}`, JSON.stringify(cards().map(card => card.dataset.entry)) === JSON.stringify(expected));
  }
  if (cards().length) {
    cards()[0].querySelector('.finding-row').click();
    check('quality card explains all applicable points', $('finding-detail').querySelector('.quality-breakdown') && $('finding-detail').textContent.includes('applicable points'));
  }
  for (const band of ['poor','partial','good','improve']) {
    $('quality-band').value = band; $('quality-band').dispatchEvent(new Event('input')); await wait();
    check('quality band ' + band, cards().every(card => {
      const score = Number(qualityRows.find(row => nameIn(row) === card.dataset.entry).dataset.quality);
      return band === 'poor' ? score < 50 : band === 'partial' ? score >= 50 && score < 80 : band === 'good' ? score >= 80 : score < 100;
    }));
  }
  $('need-weight').value = 70; $('views-weight').value = 30; $('views-weight').dispatchEvent(new Event('input')); await wait();
  check('quality weights shareable in URL', new URL(location.href).searchParams.get('need') === '70' && new URL(location.href).searchParams.get('views') === '30');
  const qualityFrame = document.createElement('iframe');
  qualityFrame.style.cssText = 'position:fixed;left:-2000px;width:390px;height:844px';
  const qualityLoaded = new Promise(resolve => qualityFrame.onload = resolve);
  qualityFrame.src = location.href; document.body.append(qualityFrame); await qualityLoaded;
  const qualityDoc = qualityFrame.contentDocument;
  check('quality shared URL restores weights and band', qualityDoc.getElementById('need-weight').value === '70' && qualityDoc.getElementById('views-weight').value === '30' && qualityDoc.getElementById('quality-band').value === 'improve' && !qualityDoc.getElementById('quality-controls').hidden);
  check('quality mobile controls do not overflow', qualityDoc.documentElement.scrollWidth <= qualityFrame.contentWindow.innerWidth);
  qualityFrame.remove();
  $('reset').click(); await wait();
  await route('all');
  check('quality ratings do not flood All findings', cards().every(card => card.dataset.category !== 'documentation-quality'));
  if (records.length) {
    const row = document.querySelector('.finding-row'); row.click(); await wait();
    check('selection shows evidence and edit action', row.getAttribute('aria-current') === 'true' && $('finding-detail').querySelector('.btn-primary')?.href.endsWith('~edit'));
    check('selection has shareable URL', new URL(location.href).searchParams.has('finding'));
    check('confidence on every finding row', [...document.querySelectorAll('.finding-row')].every(row => row.querySelector('.badge')));
    const currentId = document.querySelector('.finding-row[aria-current]')?.dataset.id;
    document.body.dispatchEvent(new KeyboardEvent('keydown',{key:'j',bubbles:true})); await wait();
    check('j selects next finding', document.querySelectorAll('.finding-row').length < 2 || document.querySelector('.finding-row[aria-current]')?.dataset.id !== currentId);
    document.body.dispatchEvent(new KeyboardEvent('keydown',{key:'k',bubbles:true})); await wait();
    check('k returns to finding', document.querySelector('.finding-row[aria-current]')?.dataset.id === currentId);
    if (pageCount(records) > 50) {
      const first = cards()[0].dataset.entry; $('next').click(); await wait();
      check('next page changes records', $('page-position').textContent.includes('Page 2') && cards()[0].dataset.entry !== first);
      $('previous').click(); await wait();
      check('previous restores records', cards()[0].dataset.entry === first);
    }
    $('sort').value = 'page'; $('sort').dispatchEvent(new Event('change')); await wait();
    const names = cards().map(card => card.querySelector('.sym').textContent);
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
  if (doc.querySelector('.finding-row')) { doc.querySelector('.finding-row').click(); await wait(); check('mobile opens full screen detail', mobile.getComputedStyle(doc.getElementById('finding-detail')).position === 'fixed'); doc.querySelector('.detail-back').click(); check('mobile back returns to queue', !doc.getElementById('finding-detail').classList.contains('mobile-open')); }
  frame.remove();
  $('reset').click(); await wait();
  await route('coverage');
  check('coverage route', !$('coverage').hidden && $('review').hidden && document.querySelectorAll('table.coverage tbody tr').length > 0);
  const fallback = document.createElement('iframe');
  fallback.style.cssText = 'position:fixed;left:-3000px;top:0;width:800px;height:600px;border:0';
  const raw = await (await fetch(location.href.split('#')[0])).text();
  const fallbackLoaded = new Promise(resolve => fallback.onload = resolve);
  fallback.srcdoc = raw.replace(/<script>[\s\S]*?<\/script>/g, ''); document.body.append(fallback); await fallbackLoaded;
  check('no-JS retains complete record', fallback.contentDocument.querySelectorAll('tr[data-category]').length === records.length + qualityRows.length && fallback.contentWindow.getComputedStyle(fallback.contentDocument.getElementById('source-data')).display !== 'none' && fallback.contentWindow.getComputedStyle(fallback.contentDocument.getElementById('quality-data')).display !== 'none');
  fallback.remove();
  check('no external assets', document.querySelectorAll('script[src],link[rel=stylesheet]').length === 0);
  check('untrusted markup remains text', document.querySelectorAll('.finding-card script,.finding-card img').length === 0);
  await route('fixes'); $('sort').value = 'priority'; $('sort').dispatchEvent(new Event('change')); $('search').blur(); scrollTo(0,0);
  return {passed:results.every(result => result.passed), count:records.length, results};
})()

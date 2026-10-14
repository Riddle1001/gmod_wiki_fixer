(async () => {
  const results = [];
  window.__reportQAProgress = results;
  const check = (name, passed, detail = '') => results.push({time: performance.now(), name, passed: Boolean(passed), detail});
  const wait = () => new Promise(resolve => setTimeout(resolve, 50));
  const $ = id => document.getElementById(id);
  const route = async key => { location.hash = key; await wait(); };
  check('capture canary', true, 'Browser assertions live; no retries');
  check('progressive enhancement initialized', document.documentElement.classList.contains('js'));
  const records = [...document.querySelectorAll('#source-data tr[data-category]')];
  const count = records.length;
  await route('all');
  $('search').value = ''; $('search').dispatchEvent(new Event('input')); await wait();
  check('all findings retained', Number($('queue-total').textContent.replaceAll(',', '')) === count, {count});
  check('rendered page bounded to 50 cards', document.querySelectorAll('.finding-card').length === Math.min(count, 50));
  check('desktop does not overflow', document.documentElement.scrollWidth <= innerWidth);
  if (count) {
    const summary = document.querySelector('.finding-summary'); summary.click(); await wait();
    check('finding opens in place', summary.parentElement.open);
    check('evidence and wiki action accessible', summary.parentElement.querySelector('.evidence-block') && summary.parentElement.querySelector('.wiki-action'));
    $('expand-all').click(); await wait();
    check('expand page', [...document.querySelectorAll('.finding-card')].every(d => d.open));
    $('expand-all').click(); await wait();
    check('collapse page', [...document.querySelectorAll('.finding-card')].every(d => !d.open));
    for (const key of new Set(records.map(row => row.dataset.category))) {
      await route(key);
      const expected = records.filter(row => row.dataset.category === key).length;
      check('category ' + key, document.querySelectorAll('.finding-card').length === Math.min(expected, 50) && [...document.querySelectorAll('.finding-card')].every(d => d.dataset.category === key));
    }
    await route('all');
    if (count > 50) {
      const first = document.querySelector('.finding-card').dataset.entry;
      $('next').click(); await wait();
      check('next page uses different records', $('page-position').textContent.includes('Page 2') && document.querySelector('.finding-card').dataset.entry !== first);
      $('previous').click(); await wait();
      check('previous page restores order', document.querySelector('.finding-card').dataset.entry === first);
    }
    $('sort').value = 'page'; $('sort').dispatchEvent(new Event('change')); await wait();
    const names = [...document.querySelectorAll('.finding-identity h3')].map(n => n.textContent);
    const collator = new Intl.Collator(undefined, {numeric:true, sensitivity:'base'});
    check('page sorting', names.every((name, i) => i === 0 || collator.compare(names[i-1], name) <= 0));
    $('search').value = 'never_matching_qa_918362'; $('search').dispatchEvent(new Event('input')); await wait();
    check('empty search state', !$('empty-state').hidden && document.querySelectorAll('.finding-card').length === 0 && $('next').disabled);
    $('reset').click(); await wait();
    check('clear restores all categories', document.querySelectorAll('.finding-card').length === Math.min(count, 50) && $('search').value === '');
    $('search').blur(); document.body.dispatchEvent(new KeyboardEvent('keydown', {key:'/',bubbles:true}));
    check('search keyboard shortcut', document.activeElement === $('search'));
  } else {
    check('empty report is explicit', !$('empty-state').hidden && $('empty-state').textContent.includes('Nothing to review') && $('next').disabled && $('previous').disabled);
  }
  await route('all');
  const checkbox = $('hide-panels');
  check('prominent panel checkbox', checkbox && checkbox.getBoundingClientRect().width >= 28 && checkbox.closest('label').getBoundingClientRect().height >= 80);
  const namesIn = row => row.cells[row.cells.length === 5 || row.cells.length === 4 ? 1 : 0].querySelector('a')?.textContent;
  const sampleNames = ['SpawnIcon:GetSkinID', 'PANEL:OnStartDragging', 'ImageCheckBox:Set'];
  for (const name of sampleNames.filter(name => records.some(row => namesIn(row) === name))) {
    $('search').value = name; $('search').dispatchEvent(new Event('input')); await wait();
    check('panel example visible: ' + name, [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === name));
    checkbox.click(); await wait();
    check('panel example hidden: ' + name, document.querySelectorAll('.finding-card').length === 0);
    checkbox.click(); await wait();
    check('panel example restored: ' + name, [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === name));
  }
  $('search').value = ''; $('search').dispatchEvent(new Event('input')); await wait();
  if (!$('next').disabled) { $('next').click(); await wait(); }
  checkbox.click(); await wait();
  check('hiding resets pagination', $('page-position').textContent.includes('Page 1') || $('page-position').textContent === 'No results');
  check('clear filter available while only panel hiding active', !$('reset').hidden);
  for (const key of new Set(records.map(row => row.dataset.category))) {
    await route(key);
    check('panel hiding stays active in ' + key, checkbox.checked && ![...document.querySelectorAll('.finding-identity h3')].some(node => sampleNames.includes(node.textContent)));
    const categoryCount = Number(document.querySelector(`.category-link[data-key="${key}"] .category-count`).textContent.replaceAll(',', ''));
    check('filtered category count matches output: ' + key, document.querySelectorAll('.finding-card').length === Math.min(categoryCount, 50));
  }
  await route('missing-controls');
  check('undocumented controls all hidden', !$('empty-state').hidden && document.querySelectorAll('.finding-card').length === 0);
  checkbox.click(); await wait();
  const missingControls = records.filter(row => row.dataset.category === 'missing-controls');
  check('undocumented controls restored', document.querySelectorAll('.finding-card').length === Math.min(missingControls.length, 50));
  await route('all');
  const nonPanel = records.map(namesIn).find(name => /^(Global|table|string|math)[.:]/.test(name));
  if (nonPanel) {
    $('search').value = nonPanel; $('search').dispatchEvent(new Event('input')); await wait();
    checkbox.click(); await wait();
    check('unrelated API retained', [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === nonPanel));
  }
  $('reset').click(); await wait();
  check('clear filters also restores panels', !checkbox.checked && $('search').value === '' && Number($('queue-total').textContent.replaceAll(',', '')) === count);
  const baseBox = $('hide-base');
  check('prominent base checkbox', baseBox && baseBox.getBoundingClientRect().width >= 28 && baseBox.closest('label').getBoundingClientRect().height >= 80);
  check('Color Copy resolved to line-range evidence', records.some(row => namesIn(row) === 'Color:Copy' && row.dataset.category === 'line-ranges') && !records.some(row => namesIn(row) === 'Color:Copy' && row.dataset.category === 'unresolved'));
  const baseExample = 'GM:OnPhysgunReload';
  $('search').value = baseExample; $('search').dispatchEvent(new Event('input')); await wait();
  check('base example initially visible', [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === baseExample));
  baseBox.click(); await wait();
  check('base example hidden', document.querySelectorAll('.finding-card').length === 0);
  baseBox.click(); await wait();
  check('base example restored', [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === baseExample));
  $('search').value = ''; $('search').dispatchEvent(new Event('input')); await wait();
  if (!$('next').disabled) { $('next').click(); await wait(); }
  baseBox.click(); await wait();
  check('base hiding resets pagination', $('page-position').textContent.includes('Page 1'));
  const baseOnlyCount = Number($('queue-total').textContent.replaceAll(',', ''));
  checkbox.click(); await wait();
  check('filters combine', Number($('queue-total').textContent.replaceAll(',', '')) < baseOnlyCount && baseBox.checked && checkbox.checked);
  for (const key of new Set(records.map(row => row.dataset.category))) {
    await route(key);
    check('base filter stays active in ' + key, baseBox.checked && checkbox.checked && ![...document.querySelectorAll('.finding-card')].some(card => /gamemodes\/base\//.test(card.textContent)));
    const expected = Number(document.querySelector(`.category-link[data-key="${key}"] .category-count`).textContent.replaceAll(',', ''));
    check('combined category counts: ' + key, document.querySelectorAll('.finding-card').length === Math.min(expected, 50));
  }
  await route('all');
  $('search').value = 'ImageCheckBox:Set'; $('search').dispatchEvent(new Event('input')); await wait();
  checkbox.click(); await wait();
  check('base filter preserves VGUI example when VGUI filter off', [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === 'ImageCheckBox:Set'));
  $('search').value = nonPanel; $('search').dispatchEvent(new Event('input')); await wait();
  check('base filter preserves unrelated API', [...document.querySelectorAll('.finding-identity h3')].some(node => node.textContent === nonPanel));
  $('reset').click(); await wait();
  check('clear resets both filters', !baseBox.checked && !checkbox.checked && Number($('queue-total').textContent.replaceAll(',', '')) === count);
  await route('coverage');
  check('coverage route', !$('coverage').hidden && $('review').hidden && document.querySelectorAll('table.coverage tbody tr').length > 0);
  await route('all');
  check('return to findings', !$('review').hidden && $('coverage').hidden);
  check('no external assets', document.querySelectorAll('script[src],link[rel=stylesheet]').length === 0);
  check('untrusted markup never becomes elements', document.querySelectorAll('.finding-card script,.finding-card img').length === 0);
  const frame = document.createElement('iframe');
  frame.style.cssText = 'position:fixed;left:-2000px;top:0;width:390px;height:844px;border:0';
  const loaded = new Promise(resolve => frame.onload = resolve);
  frame.src = location.href.split('#')[0] + '#all'; document.body.append(frame); await loaded;
  const mobile = frame.contentWindow, doc = frame.contentDocument;
  check('mobile layout breakpoint active', mobile.matchMedia('(max-width:760px)').matches);
  check('mobile page does not overflow', doc.documentElement.scrollWidth <= mobile.innerWidth);
  check('mobile panel checkbox is prominent', doc.getElementById('hide-panels').getBoundingClientRect().width >= 28 && doc.getElementById('hide-panels').closest('label').getBoundingClientRect().height >= 80);
  check('mobile base checkbox is prominent', doc.getElementById('hide-base').getBoundingClientRect().width >= 28 && doc.getElementById('hide-base').closest('label').getBoundingClientRect().height >= 80);
  check('mobile category control available', mobile.getComputedStyle(doc.querySelector('.mobile-filter')).display !== 'none' && mobile.getComputedStyle(doc.querySelector('.filters')).display === 'none');
  const picker = doc.getElementById('category-picker'); picker.value = 'failed'; picker.dispatchEvent(new mobile.Event('change')); await wait();
  check('mobile category navigation', doc.getElementById('queue-title').textContent === 'Failed checks');
  if (doc.querySelector('.finding-summary')) {
    doc.querySelector('.finding-summary').click(); await wait();
    check('mobile expanded content does not overflow', doc.documentElement.scrollWidth <= mobile.innerWidth);
  }
  frame.remove();
  const fallback = document.createElement('iframe');
  fallback.style.cssText = 'position:fixed;left:-3000px;top:0;width:800px;height:600px;border:0';
  const raw = await (await fetch(location.href.split('#')[0])).text();
  const fallbackLoaded = new Promise(resolve => fallback.onload = resolve);
  fallback.srcdoc = raw.replace(/<script>[\s\S]*?<\/script>/g, ''); document.body.append(fallback); await fallbackLoaded;
  check('no-JS retains all findings', fallback.contentDocument.querySelectorAll('tr[data-category]').length === count && fallback.contentWindow.getComputedStyle(fallback.contentDocument.getElementById('source-data')).display !== 'none');
  check('no-JS hides inactive controls', fallback.contentWindow.getComputedStyle(fallback.contentDocument.getElementById('review')).display === 'none');
  fallback.remove();
  $('sort').value = 'priority'; $('sort').dispatchEvent(new Event('change')); $('search').blur(); scrollTo(0,0);
  return {passed: results.every(r => r.passed), count, results};
})()

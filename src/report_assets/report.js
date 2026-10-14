(() => {
  'use strict';
  const groups = [
    ['all', 'All findings', 'All findings and incomplete checks, with their original evidence.', ''],
    ['missing', 'Missing pages', 'Exported APIs absent after alias, inheritance, redirect, and live-page checks.', 'Documentation'],
    ['missing-controls', 'Undocumented controls', 'Named VGUI controls with no overview page after index, alias, redirect, and live-page checks.', ''],
    ['missing-source', 'Missing source tags', 'Documented APIs with Lua implementations but no source tag.', 'Source references'],
    ['line-ranges', 'Line range drift', 'Wiki ranges that differ from the exact function boundaries.', ''],
    ['wrong-files', 'Wrong file paths', 'Source tags that point to a different file from the resolved implementation.', ''],
    ['dead-files', 'Dead source links', 'Tagged source files that do not exist in the snapshot, with no replacement resolved.', ''],
    ['realm-mismatches', 'Realm mismatches', 'Direct source availability does not overlap the wiki declaration.', 'Realms'],
    ['missing-realms', 'Missing realm tags', 'Resolved definitions with realm evidence but no wiki realm declaration.', ''],
    ['load-realm', 'Load path review', 'Static paths suggest additional realms. Runtime behavior remains unproven.', ''],
    ['malformed-source', 'Malformed source tags', 'Source markup with invalid, missing, duplicate, or out-of-bounds attributes.', 'Metadata & links'],
    ['identity', 'Function identity', 'Page identity and function metadata disagree without a resolved alias.', ''],
    ['signature', 'Signatures', 'Argument counts or ordering differ from the supported source contracts.', ''],
    ['returns', 'Return values', 'Documented return counts or known types conflict with source evidence.', ''],
    ['broken-link', 'Broken wiki links', 'Targets absent after index, alias, redirect, and live verification.', ''],
    ['unverified-link', 'Unverified links', 'Link checks that could not be completed. These are not confirmed broken links.', 'Incomplete checks'],
    ['unresolved', 'Unresolved sources', 'Source definitions that could not be matched confidently. Not clean results.', ''],
    ['failed', 'Failed checks', 'Checks interrupted by fetch, parse, or resolution errors. Not missing or clean.', ''],
  ];
  const labels = new Map(groups.map(g => [g[0], g[1]]));
  const $ = id => document.getElementById(id);
  const create = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const panelNames = new Set(registeredPanelNames.map(name => name.toLocaleLowerCase()));
  const isPanelName = name => {
    const parent = name.trim().split(/[.:]/)[0].toLocaleLowerCase();
    return panelNames.has(parent) || ['panel', 'vgui', 'derma', 'controlpanel'].includes(parent);
  };
  const rows = [...document.querySelectorAll('#source-data tr[data-category]')];
  const entries = rows.map((row, index) => {
    const cells = [...row.cells];
    const source = cells.length === 5;
    const metadata = cells.length === 4;
    const pageCell = cells[source || metadata ? 1 : 0];
    const pageLink = pageCell.querySelector('a');
    const key = row.dataset.category;
    const reason = source ? cells[4].textContent : metadata ? cells[2].textContent :
      key === 'missing-controls' ? 'Registered VGUI control has no matching wiki overview page.' : key === 'missing' ? 'No matching wiki page after documentation and live-page checks.' : cells[1].textContent;
    const panelRelated = key === 'missing-controls' || isPanelName(pageLink?.textContent || pageCell.textContent) ||
      isPanelName(pageCell.querySelector('.symbol')?.textContent || '') ||
      cells.some(cell => [...cell.querySelectorAll('.evidence a')].some(link => /\/(vgui|derma)\//i.test(link.href)));
    const baseRelated = /\bgamemodes[\/]base[\/]/i.test(row.textContent) ||
      cells.some(cell => [...cell.querySelectorAll('.evidence a')].some(link => /\/gamemodes\/base\//i.test(link.href)));
    return {index, key, panelRelated, baseRelated, source, metadata, cells, pageCell, pageLink,
      page: pageLink?.textContent || pageCell.textContent,
      reason: reason.trim(), score: source ? BigInt(cells[0].textContent.trim() || '0') : 0n,
      search: (row.textContent + ' ' + labels.get(key)).toLocaleLowerCase()};
  });
  const count = key => key === 'all' ? entries.length : entries.filter(e => e.key === key).length;
  for (const [key, title, , group] of groups) {
    if (group) $('category-list').append(create('p', 'filter-group', group));
    const link = create('a', 'category-link');
    link.href = '#' + key;
    link.dataset.key = key;
    link.append(create('span', '', title), create('span', 'category-count', count(key).toLocaleString()));
    $('category-list').append(link);
    const option = create('option', '', `${title} (${count(key).toLocaleString()})`);
    option.value = key;
    $('category-picker').append(option);
  }
  $('queue-total').textContent = entries.length.toLocaleString();
  let active = 'all', page = 0, expanded = false;
  const pageSize = 50;
  const collator = new Intl.Collator(undefined, {numeric: true, sensitivity: 'base'});
  const openEntries = new Set();
  const appendClone = (parent, cell) => {
    for (const child of cell.childNodes) parent.append(child.cloneNode(true));
  };
  function card(entry) {
    const detail = create('details', 'finding-card');
    detail.dataset.category = entry.key;
    detail.dataset.entry = entry.index;
    detail.open = expanded || openEntries.has(entry.index);
    const summary = create('summary', 'finding-summary');
    const identity = create('div', 'finding-identity');
    identity.append(create('span', 'finding-type type-' + entry.key, labels.get(entry.key)), create('h3', '', entry.page));
    summary.append(identity, create('p', 'finding-preview', entry.reason), create('span', 'disclosure', '+'));
    summary.lastChild.setAttribute('aria-hidden', 'true');
    detail.append(summary);
    const body = create('div', 'finding-body');
    body.append(create('p', 'finding-explanation', entry.reason));
    if (entry.source) {
      const comparison = create('div', 'comparison');
      const wiki = create('div', 'comparison-cell wiki-value');
      wiki.append(create('span', 'detail-label', 'ON THE WIKI'));
      appendClone(wiki, entry.cells[2]);
      const expected = create('div', 'comparison-cell source-value');
      expected.append(create('span', 'detail-label', 'IN THE SOURCE'));
      const value = entry.cells[3].querySelector('.expected');
      if (value) expected.append(value.cloneNode(true));
      comparison.append(wiki, expected);
      body.append(comparison);
      const symbol = entry.pageCell.querySelector('.symbol');
      if (symbol) body.append(create('p', 'symbol-note', 'Symbol: ' + symbol.textContent));
    }
    const evidence = create('div', 'evidence-block');
    evidence.append(create('span', 'detail-label', 'SUPPORTING EVIDENCE'));
    const evidenceCell = entry.cells[entry.source ? 3 : entry.metadata ? 3 : 1];
    if (entry.source) {
      const links = evidenceCell.querySelectorAll('.evidence');
      if (links.length) for (const link of links) evidence.append(link.cloneNode(true));
      else evidence.append(create('p', 'muted', 'No exact source link is available for this finding.'));
    } else if (entry.metadata || ['missing', 'missing-controls'].includes(entry.key)) {
      if (evidenceCell.textContent.trim()) appendClone(evidence, evidenceCell);
      else evidence.append(create('p', 'muted', 'No additional source evidence was recorded.'));
    } else {
      evidence.append(create('p', 'muted', entry.key === 'failed' ? 'This check is incomplete. Retry it before drawing a conclusion.' : 'No confident source match was established.'));
    }
    body.append(evidence);
    const actions = create('div', 'finding-actions');
    if (entry.pageLink) {
      const link = entry.pageLink.cloneNode(true);
      link.textContent = 'Open wiki page ↗';
      link.className = 'wiki-action';
      actions.append(link);
    }
    if (entry.source) actions.append(create('span', 'priority-note', `Priority score ${entry.score.toLocaleString()}`));
    body.append(actions);
    detail.append(body);
    detail.addEventListener('toggle', () => {
      if (detail.open) openEntries.add(entry.index); else openEntries.delete(entry.index);
    });
    return detail;
  }
  function render() {
    const terms = $('search').value.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const hidePanels = $('hide-panels').checked;
    const hideBase = $('hide-base').checked;
    const visible = entries.filter(e => (!hidePanels || !e.panelRelated) && (!hideBase || !e.baseRelated));
    for (const [key, title] of groups) {
      const total = key === 'all' ? visible.length : visible.filter(e => e.key === key).length;
      document.querySelector(`.category-link[data-key="${key}"] .category-count`).textContent = total.toLocaleString();
      $('category-picker').querySelector(`option[value="${key}"]`).textContent = `${title} (${total.toLocaleString()})`;
    }
    $('queue-total').textContent = visible.length.toLocaleString();
    const baseTotal = entries.filter(e => e.baseRelated).length;
    $('base-filter-count').textContent = `${baseTotal.toLocaleString()} ${hideBase ? 'hidden' : 'findings'}`;
    const panelTotal = entries.filter(e => e.panelRelated).length;
    $('panel-filter-count').textContent = `${panelTotal.toLocaleString()} ${hidePanels ? 'hidden' : 'findings'}`;
    const filtered = visible.filter(e => (active === 'all' || active === e.key) && terms.every(term => e.search.includes(term)));
    const sort = $('sort').value;
    filtered.sort((a, b) => {
      if (sort === 'priority' && a.score !== b.score) return a.score > b.score ? -1 : 1;
      if (sort === 'category') { const order = collator.compare(labels.get(a.key), labels.get(b.key)); if (order) return order; }
      return collator.compare(a.page, b.page) || a.index - b.index;
    });
    page = Math.min(page, Math.max(0, Math.ceil(filtered.length / pageSize) - 1));
    const from = page * pageSize;
    $('finding-list').replaceChildren(...filtered.slice(from, from + pageSize).map(card));
    $('empty-state').hidden = filtered.length !== 0;
    $('empty-state').querySelector('h3').textContent = entries.length === 0 ? 'Nothing to review in this run.' : 'No matching findings.';
    $('empty-state').querySelector('p').textContent = entries.length === 0 ? 'Check coverage for the scope and limits of this result.' : 'Try a different search or choose another category.';
    $('result-count').textContent = filtered.length ? `${from + 1}–${Math.min(from + pageSize, filtered.length)} of ${filtered.length.toLocaleString()} items` : '0 matching items';
    $('page-position').textContent = filtered.length ? `Page ${page + 1} of ${Math.ceil(filtered.length / pageSize)}` : 'No results';
    $('previous').disabled = page === 0;
    $('next').disabled = from + pageSize >= filtered.length;
    $('expand-all').disabled = filtered.length === 0;
    $('expand-all').textContent = expanded ? 'Collapse this page' : 'Expand this page';
    $('expand-all').setAttribute('aria-pressed', String(expanded));
    $('reset').hidden = active === 'all' && !terms.length && !hidePanels && !hideBase;
  }
  function route() {
    const aliases = {'overview':'all', 'missing-pages':'missing', 'load-realms':'load-realm', 'all-metrics':'coverage', 'source-summary':'all', 'source-issues':'all', 'metadata':'all'};
    let key = location.hash.slice(1) || 'all';
    key = aliases[key] || key;
    const coverage = key === 'coverage';
    if (!coverage && !labels.has(key)) key = 'all';
    if (!coverage) active = key;
    $('review').hidden = coverage;
    $('coverage').hidden = !coverage;
    for (const [id, selected] of [['review-tab', !coverage], ['coverage-tab', coverage]]) {
      if (selected) $(id).setAttribute('aria-current', 'page'); else $(id).removeAttribute('aria-current');
    }
    for (const link of document.querySelectorAll('.category-link')) {
      if (link.dataset.key === active) link.setAttribute('aria-current', 'true'); else link.removeAttribute('aria-current');
    }
    $('category-picker').value = active;
    $('queue-title').textContent = labels.get(active);
    $('queue-description').textContent = groups.find(g => g[0] === active)[2];
    $('queue-eyebrow').textContent = active === 'all' ? 'THE COMPLETE PICTURE' : 'FOCUSED REVIEW';
    document.title = (coverage ? 'Coverage' : labels.get(active)) + " · Garry's Mod Wiki Audit";
    page = 0;
    render();
  }
  $('search').addEventListener('input', () => { page = 0; render(); });
  $('hide-base').addEventListener('change', () => { page = 0; render(); });
  $('hide-panels').addEventListener('change', () => { page = 0; render(); });
  $('sort').addEventListener('change', () => { page = 0; render(); });
  $('category-picker').addEventListener('change', () => { location.hash = $('category-picker').value; });
  $('reset').addEventListener('click', () => { $('hide-panels').checked = false; $('hide-base').checked = false; $('search').value = ''; page = 0; if (active !== 'all') location.hash = 'all'; else render(); $('search').focus(); });
  $('expand-all').addEventListener('click', () => {
    expanded = !expanded;
    for (const detail of $('finding-list').children) {
      detail.open = expanded;
      if (expanded) openEntries.add(Number(detail.dataset.entry)); else openEntries.delete(Number(detail.dataset.entry));
    }
    $('expand-all').textContent = expanded ? 'Collapse this page' : 'Expand this page';
    $('expand-all').setAttribute('aria-pressed', String(expanded));
  });
  for (const [id, delta] of [['previous', -1], ['next', 1]]) $(id).addEventListener('click', () => { page += delta; expanded = false; render(); $('queue-title').scrollIntoView({block:'start'}); $(id).focus({preventScroll:true}); });
  document.addEventListener('keydown', event => {
    if (event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.target.matches('input,textarea,select,[contenteditable]')) {
      event.preventDefault(); if ($('review').hidden) location.hash = active; $('search').focus();
    }
  });
  addEventListener('hashchange', route);
  route();
  // Only hide the fallback record after successful initialization.
  document.documentElement.classList.add('js');
})();

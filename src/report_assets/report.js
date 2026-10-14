(() => {
  'use strict';
  const groups = [
    ['fixes', 'Fixes', 'Broken links, invalid source paths, and concrete API discrepancies.', 'Views'],
    ['maintenance', 'Source maintenance', 'Exact line accuracy, replacement paths, and source or realm tags.', ''],
    ['opportunities', 'Documentation opportunities', 'Supported missing-page candidates and optional example contributions.', ''],
    ['diagnostics', 'Audit diagnostics', 'Incomplete checks and static load-path review; these are not confirmed wiki defects.', ''],
    ['documentation-quality', 'Documentation quality', 'Heuristic coverage ratings, with priority weighted by documentation need and page views.', ''],
    ['examples', 'Example opportunities', 'Pages without example blocks. Start with a small batch of useful snippets.', 'Contributions'],
    ['all', 'All findings', 'All findings and incomplete checks, with their original evidence.', ''],
    ['missing', 'Missing page candidates', 'Exported APIs absent after alias, inheritance, redirect, and live-page checks.', 'Documentation'],
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
    ['doc-critical', 'Empty documentation', 'No usable purpose or guidance; low scores alone do not enter Fixes.', ''],
  ];
  // Views classify existing records; HTML remains the authoritative complete record.
  // Revisit if another presentation consumer needs this classification in a shared format.
  const views = {
    fixes: ['dead-files', 'realm-mismatches', 'malformed-source', 'identity', 'signature', 'returns', 'broken-link', 'doc-critical'],
    maintenance: ['line-ranges', 'wrong-files', 'missing-source', 'missing-realms'],
    opportunities: ['missing', 'missing-controls', 'examples'],
    diagnostics: ['unresolved', 'failed', 'unverified-link', 'load-realm']
  };
  const belongs = (entry, key) => key === 'all' ? entry.key !== 'documentation-quality' : (views[key] || [key]).includes(entry.key);
  const labels = new Map(groups.map(g => [g[0], g[1]]));
  const $ = id => document.getElementById(id);
  const liveClock = $('live-clock');
  const nextRefreshLocal = $('next-refresh-local');
  const nextRefreshUtc = $('next-refresh-utc');
  const refreshCountdown = $('refresh-countdown');
  const localClockFormatter = new Intl.DateTimeFormat(undefined, {
    hour: 'numeric', minute: '2-digit', second: '2-digit', timeZoneName: 'short'
  });
  const localDateFormatter = new Intl.DateTimeFormat(undefined, {
    weekday: 'short', month: 'short', day: 'numeric', hour: 'numeric', minute: '2-digit', timeZoneName: 'short'
  });
  const utcDateFormatter = new Intl.DateTimeFormat('en-GB', {
    weekday: 'short', day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit',
    timeZone: 'UTC', hourCycle: 'h23'
  });
  const completed = $('audit-completed');
  if (completed) {
    const date = new Date(Number(completed.dataset.epoch) * 1000);
    completed.dateTime = date.toISOString();
    completed.textContent = localDateFormatter.format(date);
  }
  function updateRefreshClock() {
    const currentTime = new Date();
    liveClock.dateTime = currentTime.toISOString();
    liveClock.textContent = localClockFormatter.format(currentTime);

    const today = [0, 8, 16].map(hour => new Date(Date.UTC(
      currentTime.getUTCFullYear(), currentTime.getUTCMonth(), currentTime.getUTCDate(), hour, 17
    )));
    let nextRefresh = today.find(candidate => candidate > currentTime);
    if (!nextRefresh) {
      nextRefresh = new Date(Date.UTC(
        currentTime.getUTCFullYear(), currentTime.getUTCMonth(), currentTime.getUTCDate() + 1, 0, 17
      ));
    }
    if ($('refresh-summary')) $('refresh-summary').textContent = `Next: ${localDateFormatter.format(nextRefresh)}`;
    nextRefreshLocal.dateTime = nextRefresh.toISOString();
    nextRefreshLocal.textContent = localDateFormatter.format(nextRefresh);
    nextRefreshUtc.textContent = `${utcDateFormatter.format(nextRefresh)} UTC`;

    const secondsLeft = Math.max(0, Math.floor((nextRefresh.getTime() - currentTime.getTime()) / 1000));
    const hours = Math.floor(secondsLeft / 3600);
    const minutes = Math.floor((secondsLeft % 3600) / 60);
    const seconds = secondsLeft % 60;
    refreshCountdown.textContent = `in ${String(hours).padStart(2, '0')}:${String(minutes).padStart(2, '0')}:${String(seconds).padStart(2, '0')}`;
    const navCountdown = $('nav-refresh-countdown');
    if (navCountdown) {
      navCountdown.textContent = refreshCountdown.textContent.slice(3);
      navCountdown.title = localDateFormatter.format(nextRefresh);
    }
  }
  updateRefreshClock();
  window.setInterval(updateRefreshClock, 1000);
  const create = (tag, className, text) => {
    const node = document.createElement(tag);
    if (className) node.className = className;
    if (text !== undefined) node.textContent = text;
    return node;
  };
  const nf = new Intl.NumberFormat();
  document.querySelectorAll('.stat strong, td.number, .section-count, .total-badge').forEach(el => {
    if (/^\d+$/.test(el.textContent.trim())) el.textContent = nf.format(BigInt(el.textContent.trim()));
  });
  const stats = [...document.querySelectorAll('.stat strong')].map(el => Number(el.textContent.replace(/[^0-9]/g,'')));
  const actionNote = document.querySelector('.stat:nth-child(3) small'); if(actionNote) actionNote.textContent = 'fixes need attention';
  const coverageStatus = create('div', 'coverage-status');
  coverageStatus.textContent = `${nf.format(stats[1] || 0)} clean source pages / ${nf.format(stats[0] || 0)} checked · Change since last run unavailable`;
  const bar = create('div', 'coverage-bar'); const clean = create('span');
  clean.style.width = Math.min(100, 100 * (stats[1] || 0) / Math.max(1, stats[0] || 0)) + '%'; bar.append(clean);
  bar.setAttribute('role','img'); bar.setAttribute('aria-label',coverageStatus.textContent);
  const summary = document.querySelector('.stats'); summary.after(coverageStatus); coverageStatus.prepend(bar);
  const coverageBar = coverageStatus.cloneNode(true); $('coverage').prepend(coverageBar);
  const metricRows = [...document.querySelectorAll('table.coverage tbody tr')];
  const sections = new Map([['Scope',[]],['Coverage',[]],['Results',[]]]);
  for (const row of metricRows) {
    const title = row.cells[0].textContent;
    const group = /exclud|skipp|metamethod/i.test(title) ? 'Scope' : /checked|analyzed|matching|clean|alias|comparisons|indexed/i.test(title) ? 'Coverage' : 'Results';
    sections.get(group).push(row);
  }
  const metricsBody = document.querySelector('table.coverage tbody');
  if(metricsBody) for(const [title, rows] of sections) {
    const heading = create('tr','metric-group'); const th = create('th','',title); th.colSpan=3; th.scope='rowgroup'; heading.append(th); metricsBody.append(heading,...rows);
  }
  const panelNames = new Set(registeredPanelNames.map(name => name.toLocaleLowerCase()));
  const isPanelName = name => {
    const parent = name.trim().split(/[.:]/)[0].toLocaleLowerCase();
    return panelNames.has(parent) || ['panel', 'vgui', 'derma', 'controlpanel'].includes(parent);
  };
  const rows = [...document.querySelectorAll('#source-data tr[data-category], #quality-data tr[data-category]')];
  const entries = rows.map((row, index) => {
    const cells = [...row.cells];
    const source = cells.length === 5;
    const metadata = cells.length === 4;
    const pageCell = cells[source || metadata ? 1 : 0];
    const pageLink = pageCell.querySelector('a');
    const key = row.dataset.category;
    const reason = source ? cells[4].textContent : metadata ? cells[2].textContent :
      key === 'missing-controls' ? 'Registered VGUI control has no matching wiki overview page.' : key === 'missing' ? 'No matching wiki page after documentation and live-page checks.' : cells[1].textContent;
    const panelRelated = row.dataset.panel === 'true' || key === 'missing-controls' || isPanelName(pageLink?.textContent || pageCell.textContent) ||
      isPanelName(pageCell.querySelector('.symbol')?.textContent || '') ||
      cells.some(cell => [...cell.querySelectorAll('.evidence a')].some(link => /\/(vgui|derma)\//i.test(link.href)));
    const baseRelated = row.dataset.base === 'true' || /\bgamemodes[\/]base[\/]/i.test(row.textContent) ||
      cells.some(cell => [...cell.querySelectorAll('.evidence a')].some(link => /\/gamemodes\/base\//i.test(link.href)));
    return {index, key, quality: key === 'documentation-quality' ? Number(row.dataset.quality) : null, panelRelated, baseRelated, source, metadata, cells, pageCell, pageLink,
      page: pageLink?.textContent || pageCell.textContent,
      reason: reason.trim(), views: Object.hasOwn(pageViews, pageLink?.textContent || pageCell.textContent) ? BigInt(pageViews[pageLink?.textContent || pageCell.textContent]) : null,
      search: (row.textContent + ' ' + labels.get(key)).toLocaleLowerCase()};
  });
  const rated = entries.filter(e => e.quality !== null);
  const maximumViewsLog = rated.reduce((max, e) => Math.max(max, Math.log1p(Number(e.views || 0n))), 0);
  const reach = entry => maximumViewsLog ? 100 * Math.log1p(Number(entry.views || 0n)) / maximumViewsLog : 0;
  function weights() {
    const need = Number($('need-weight').value), views = Number($('views-weight').value);
    return need + views ? [need, views] : [60, 40];
  }
  function qualityPriority(entry) {
    const [need, views] = weights();
    return (need * (100 - entry.quality) + views * reach(entry)) / (need + views);
  }
  const count = key => entries.filter(e => belongs(e, key)).length;
  for (const [key, title, , group] of groups) {

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
  let active = 'fixes', page = 0, expanded = false;
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
    if (entry.quality !== null) {
      body.append(create('p', 'finding-explanation', `Quality: ${entry.quality}/100 · Review priority: ${qualityPriority(entry).toFixed(1)}/100 · Documentation need: ${100 - entry.quality}/100 · Reach: ${reach(entry).toFixed(1)}/100`));
      body.append(create('p', '', entry.reason));
    } else body.append(create('p', 'finding-explanation', entry.reason));
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
    evidence.append(create('span', 'detail-label', entry.quality !== null ? 'WHY THIS SCORE' : 'SUPPORTING EVIDENCE'));
    const evidenceCell = entry.cells[entry.source ? 3 : entry.metadata ? 3 : 1];
    if (entry.source) {
      const links = evidenceCell.querySelectorAll('.evidence');
      if (links.length) for (const link of links) evidence.append(link.cloneNode(true));
      else evidence.append(create('p', 'muted', 'No exact source link is available for this finding.'));
    } else if (entry.metadata || ['missing', 'missing-controls'].includes(entry.key)) {
      if (evidenceCell.textContent.trim()) appendClone(evidence, evidenceCell);
      else evidence.append(create('p', 'muted', 'No additional source evidence was recorded.'));
    } else {
      evidence.append(create('p', 'muted', entry.key === 'failed' ? 'This check is incomplete. Retry it before drawing a conclusion.' : entry.key === 'unresolved' ? entry.reason : 'No confident source match was established.'));
    }
    if (!['examples', 'unresolved'].includes(entry.key)) body.append(evidence);
    const actions = create('div', 'finding-actions');
    if (entry.pageLink) {
      const link = entry.pageLink.cloneNode(true);
      link.textContent = 'Open wiki page ↗';
      link.className = 'wiki-action';
      actions.append(link);
    }
    actions.append(create('span', 'priority-note', `Page views: ${entry.views === null ? 'unavailable' : entry.views.toLocaleString()}`));
    body.append(actions);
    detail.append(body);
    return detail;
  }
  // Selection belongs to this standalone view; move it only if another UI consumes it.
  // Revisit if findings gain stable IDs from an upstream audit store.
  const findingId = entry => encodeURIComponent(entry.key + '/' + entry.page + '/' + entry.reason);
  let selected = null;
  const confidence = e => ['failed','unresolved','unverified-link','load-realm'].includes(e.key) ? 'Inconclusive' :
    ['signature','returns','missing','missing-controls','examples','documentation-quality','doc-critical','identity'].includes(e.key) ? 'Likely' : 'Confirmed';
  const badge = e => { const node = create('span', 'badge badge-' + confidence(e).toLowerCase(), e.quality !== null ? 'Heuristic' : confidence(e)); node.title = e.quality !== null ? 'Visible documentation coverage, not a judgment of correctness or writing quality.' : confidence(e) === 'Confirmed' ? 'Definite result within the static check’s stated scope; runtime behavior is not established.' : confidence(e) === 'Likely' ? 'Review the documented contract and context before editing.' : 'Evidence is incomplete or static reachability needs review.'; return node; };
  const pane = create('article', 'detail'); pane.id = 'finding-detail';
  pane.setAttribute('aria-label', 'Finding detail');
  $('review').append(pane);
  const back = create('button', 'detail-back', 'Back to queue'); back.type = 'button';
  // These are display excerpts from the recorded explanation, never new audit rules.
  // Unknown wording keeps the complete explanation visible instead of guessing values.
  function comparedValues(entry) {
    let m = entry.reason.match(/^Documented arguments \((.*?)\) differ from source declarations: (.*?)\. Review/s);
    if(m) return [entry.page + '(' + m[1] + ')', m[2]];
    m = entry.reason.match(/^Wiki documents (.*?) return values; explicit source paths return (.*?) values\./s);
    return m ? [m[1] + ' return values', m[2] + ' return values'] : null;
  }
  function problemLabel(entry) {
    const values = comparedValues(entry);
    return values ? `Wiki: ${values[0]} · Source: ${values[1]}` : entry.reason.split(/(?<=\.)\s+Review /)[0];
  }
  function showFinding(entry, updateUrl = true) {
    selected = entry;
    document.querySelectorAll('.finding-row').forEach(row => {
      if (row.dataset.id === findingId(entry)) row.setAttribute('aria-current', 'true');
      else row.removeAttribute('aria-current');
    });
    pane.replaceChildren();
    const close = back.cloneNode(true); close.onclick = () => { pane.classList.remove('mobile-open'); document.querySelector('.finding-row[aria-current]')?.focus(); };
    pane.append(close);
    const head = create('header', 'detail-head');
    const title = create('h2'); title.append(create('code', '', entry.page)); head.append(title);
    if (entry.pageLink) {
      const edit = entry.pageLink.cloneNode(true); edit.href += '~edit'; edit.className = 'btn-primary'; edit.textContent = 'Edit on wiki'; head.append(edit);
    }
    pane.append(head);
    const meta = create('p', 'detail-meta'); meta.append(create('span', 'tag', labels.get(entry.key)), badge(entry),
      create('span', '', entry.views === null ? 'Views unavailable' : new Intl.NumberFormat().format(entry.views) + ' views'));
    pane.append(meta);
    const original = card(entry).querySelector('.finding-body');
    pane.append(original);
    const values = comparedValues(entry);
    if(values && !pane.querySelector('.comparison')) {
      const comparison = create('div','comparison');
      values.forEach((value,i) => {const cell = create('section','comparison-cell ' + (i ? 'source-value' : 'wiki-value')); cell.append(create('h3','detail-label',i ? 'Source says' : 'Wiki says'),create('code','',value)); comparison.append(cell);});
      original.querySelector('.finding-explanation').after(comparison);
    }
    pane.querySelectorAll('.wiki-line').forEach(line => line.remove());
    // Highlight differing tokens only; the values themselves remain the recorded evidence.
    const wiki = pane.querySelector('.wiki-value'), source = pane.querySelector('.source-value');
    if (wiki && source && wiki.textContent !== source.textContent) { wiki.classList.add('diff-wiki'); source.classList.add('diff-source');
      const left = wiki.querySelector('code'), right = source.querySelector('code');
      if(left && right) {
        const a = left.textContent, b = right.textContent; let prefix = 0, suffix = 0;
        while(prefix < Math.min(a.length,b.length) && a[prefix] === b[prefix]) prefix++;
        while(suffix < Math.min(a.length,b.length) - prefix && a[a.length-1-suffix] === b[b.length-1-suffix]) suffix++;
        for(const [node,value] of [[left,a],[right,b]]) { const end = value.length - suffix; node.replaceChildren(document.createTextNode(value.slice(0,prefix)), create('mark','',value.slice(prefix,end)), document.createTextNode(value.slice(end))); }
      } }
    for (const link of pane.querySelectorAll('.evidence a')) {
      const snippet = typeof sourceSnippets !== 'undefined' ? sourceSnippets[link.href] : null;
      if (snippet) { const pre = create('pre', 'source-code');
        snippet.lines.forEach((line, i) => { const row = create('span', 'code-line'); row.append(create('span', 'line-number', new Intl.NumberFormat().format(snippet.start + i)), create('code', '', line)); pre.append(row); });
        link.parentElement.append(pre);
        if (snippet.truncated) link.parentElement.append(create('p', 'muted', 'Excerpt limited to 80 lines. Open the pinned source for the full definition.'));
      } else link.parentElement.append(create('p', 'muted', 'Code excerpt unavailable in this report. Open the pinned source to inspect it.'));
    }
    const action = pane.querySelector('.wiki-action'); if (action) action.textContent = 'View wiki page';
    pane.querySelectorAll('.evidence a').forEach(link => { const label = create('span','source-link-label','View pinned source on GitHub ↗'); link.append(label); });
    pane.append(create('p', 'muted keyboard-note', 'j / k: next / previous · /: search · e: edit'));
    pane.querySelectorAll('.evidence a').forEach(link => { const label = create('span','source-link-label','View pinned source on GitHub ↗'); link.append(label); });
    pane.append(create('p', 'muted keyboard-note', 'j / k: next / previous · /: search · e: edit'));
    if (updateUrl) { const params = new URLSearchParams(location.search); params.set('finding', findingId(entry)); history.replaceState(null, '', location.pathname + '?' + params + location.hash); }
  }
  function pageCard(items) {
    const wrap = create('div', 'page-rows'); wrap.dataset.entry = items[0].page; wrap.dataset.category = items[0].key;
    for (const entry of items) {
      const row = create('a', 'finding-row'); row.href = '?' + new URLSearchParams({finding:findingId(entry)}) + '#' + active;
      row.dataset.id = findingId(entry); row.dataset.entry = entry.index;
      const main = create('div', 'row-main'); main.append(create('code', 'sym', entry.page), create('span', 'row-problem', problemLabel(entry)));
      const meta = create('div', 'row-meta'); meta.append(create('span', 'tag', labels.get(entry.key)), badge(entry), create('span', 'views', entry.views === null ? '—' : new Intl.NumberFormat(undefined, {notation:'compact', maximumFractionDigits:1}).format(entry.views)));
      if (entry.quality !== null) meta.prepend(create('span', 'tag', `Quality ${entry.quality}/100`), create('span', 'tag', `Priority ${qualityPriority(entry).toFixed(1)}`));
      row.append(main, meta); row.onclick = e => { if(e.ctrlKey || e.metaKey || e.shiftKey || e.altKey) return; e.preventDefault(); showFinding(entry); pane.classList.add('mobile-open'); };
      wrap.append(row);
    }
    return wrap;
  }
  function saveState() {
    const params = new URLSearchParams();
    if (selected) params.set('finding', findingId(selected));
    if ($('search').value) params.set('q', $('search').value);
    if ($('hide-panels').checked) params.set('panels', 'hide');
    if ($('hide-base').checked) params.set('base', 'hide');
    if ($('sort').value !== 'priority') params.set('sort', $('sort').value);
    if ($('need-weight').value !== '60') params.set('need', $('need-weight').value);
    if ($('views-weight').value !== '40') params.set('views', $('views-weight').value);
    if ($('quality-band').value !== 'improve') params.set('quality', $('quality-band').value);
    history.replaceState(null, '', `${location.pathname}${params.size ? '?' + params : ''}${location.hash || '#fixes'}`);
  }
  function render() {
    const terms = $('search').value.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
    const hidePanels = $('hide-panels').checked;
    const hideBase = $('hide-base').checked;
    const visible = entries.filter(e => (!hidePanels || !e.panelRelated) && (!hideBase || !e.baseRelated));
    const qualityView = active === 'documentation-quality';
    $('quality-controls').hidden = !qualityView;
    $('sort').querySelector('[value="priority"]').textContent = qualityView ? 'Weighted documentation priority' : 'Page views — highest first';
    if (qualityView) {
      const poor = rated.filter(e => e.quality < 50).length, good = rated.filter(e => e.quality >= 80).length;
      const average = rated.length ? (rated.reduce((sum, e) => sum + e.quality, 0) / rated.length).toFixed(1) : 'unavailable';
      $('quality-summary').textContent = `${rated.length.toLocaleString()} rated pages · Average ${average}/100 · Poor coverage: ${poor.toLocaleString()} · Partial: ${(rated.length - poor - good).toLocaleString()} · Good coverage: ${good.toLocaleString()}. Entire rated snapshot; filters do not change these counts.`;
      const [need, views] = weights();
      $('need-weight-value').value = $('need-weight').value;
      $('views-weight-value').value = $('views-weight').value;
      $('quality-formula').textContent = `Priority = (${need} × documentation need + ${views} × logarithmic reach) / ${need + views}. Higher priority is reviewed first; higher quality is better.`;
    }
    for (const [key, title] of groups) {
      const total = visible.filter(e => belongs(e, key)).length;
      document.querySelector(`.category-link[data-key="${key}"] .category-count`).textContent = total.toLocaleString();
      $('category-picker').querySelector(`option[value="${key}"]`).textContent = `${title} (${total.toLocaleString()})`;
    }
    $('queue-total').textContent = `${visible.filter(e => belongs(e, active)).length.toLocaleString()} findings`;
    const section = views[active] ? active : Object.keys(views).find(key => views[key].includes(active));
    const scope = section || active;
    const categoryLinks = [...document.querySelectorAll('.category-link')];
    const categoryKeys = groups.filter(([key]) => !views[key] && !['all','documentation-quality'].includes(key) && (scope === 'all' || (views[scope] || [scope]).includes(key))).map(g => g[0]);
    const totalFor = key => visible.filter(e => belongs(e, key)).length;
    categoryLinks.sort((a,b) => totalFor(b.dataset.key) - totalFor(a.dataset.key));
    const zero = create('details', 'zero-categories'); zero.append(create('summary', '', 'No issues'));
    const allLink = create('a', 'category-link', 'All categories'); allLink.href = '#' + scope;
    $('category-list').replaceChildren(allLink);
    for (const link of categoryLinks) {
      link.hidden = !categoryKeys.includes(link.dataset.key);
      if (!link.hidden) (totalFor(link.dataset.key) ? $('category-list') : zero).append(link);
      else $('category-list').append(link);
    }
    if (zero.children.length > 1) $('category-list').append(zero);
    for (const option of $('category-picker').options) option.hidden = !categoryKeys.includes(option.value) && option.value !== scope;
    const scoped = entries.filter(e => belongs(e, scope));
    $('base-filter-count').textContent = new Intl.NumberFormat().format(scoped.filter(e => e.baseRelated).length) + ' in scope';
    $('panel-filter-count').textContent = new Intl.NumberFormat().format(scoped.filter(e => e.panelRelated).length) + ' in scope';
    for (const tab of document.querySelectorAll('.tab')) {
      const key = tab.hash.slice(1); if (key === scope) tab.setAttribute('aria-current', 'page'); else tab.removeAttribute('aria-current');
    }
    const band = $('quality-band').value;
    const filtered = visible.filter(e => belongs(e, active) && terms.every(term => e.search.includes(term)) && (!qualityView ||
      band === 'all' || (band === 'improve' && e.quality < 100) || (band === 'poor' && e.quality < 50) || (band === 'partial' && e.quality >= 50 && e.quality < 80) || (band === 'good' && e.quality >= 80)));
    const sort = $('sort').value;
    filtered.sort((a, b) => {
      if (sort === 'priority' && qualityView) {
        const priority = qualityPriority(b) - qualityPriority(a);
        return priority || collator.compare(a.page, b.page) || a.index - b.index;
      }
      if (sort === 'priority' && a.views !== b.views) { if (a.views === null) return 1; if (b.views === null) return -1; return a.views > b.views ? -1 : 1; }
      if (sort === 'category') { const order = collator.compare(labels.get(a.key), labels.get(b.key)); if (order) return order; }
      return collator.compare(a.page, b.page) || a.index - b.index;
    });
    const byPage = new Map();
    for (const entry of filtered) {
      if (!byPage.has(entry.page)) byPage.set(entry.page, []);
      byPage.get(entry.page).push(entry);
    }
    const pageGroups = [...byPage.values()];
    const requested = new URLSearchParams(location.search).get('finding');
    if (!selected && requested) { const groupIndex = pageGroups.findIndex(items => items.some(e => findingId(e) === requested)); if (groupIndex >= 0) page = Math.floor(groupIndex / pageSize); }
    page = Math.min(page, Math.max(0, Math.ceil(pageGroups.length / pageSize) - 1));
    const from = page * pageSize;
    if (!$('keyboard-help')) { const help = create('p','muted keyboard-help','Keyboard: j / k select · / search · e edit'); help.id = 'keyboard-help'; document.querySelector('.queue-tools')?.after(help); }
    $('finding-list').replaceChildren(...pageGroups.slice(from, from + pageSize).map(pageCard));
    const targetId = new URLSearchParams(location.search).get('finding');
    const target = filtered.find(e => findingId(e) === targetId) || filtered.find(e => selected && findingId(e) === findingId(selected)) || filtered[0];
    if (target) showFinding(target, false); else { selected = null; pane.replaceChildren(create('p', 'muted', 'Select a finding to inspect its evidence.')); pane.classList.remove('mobile-open'); }
    $('empty-state').hidden = filtered.length !== 0;
    $('empty-state').querySelector('h3').textContent = entries.length === 0 ? 'Nothing to review in this run.' : 'No matching findings.';
    $('empty-state').querySelector('p').textContent = entries.length === 0 ? 'Check coverage for the scope and limits of this result.' : 'Try a different search or choose another category.';
    $('result-count').textContent = filtered.length ? `${from + 1}–${Math.min(from + pageSize, pageGroups.length)} of ${pageGroups.length.toLocaleString()} pages · ${filtered.length.toLocaleString()} ${qualityView ? 'ratings' : 'findings'}` : '0 matching items';
    $('page-position').textContent = filtered.length ? `Page ${page + 1} of ${Math.ceil(pageGroups.length / pageSize)}` : 'No results';
    $('previous').disabled = page === 0;
    $('next').disabled = from + pageSize >= pageGroups.length;
    $('expand-all').disabled = filtered.length === 0;
    $('expand-all').hidden = true;
    $('expand-all').setAttribute('aria-pressed', String(expanded));
    saveState();
    $('reset').hidden = active === 'fixes' && !terms.length && !hidePanels && !hideBase;
  }
  function route() {
    const aliases = {'overview':'all', 'missing-pages':'missing', 'load-realms':'load-realm', 'all-metrics':'coverage', 'source-summary':'all', 'source-issues':'all', 'metadata':'all'};
    let key = location.hash.slice(1) || 'fixes';
    key = aliases[key] || key;
    const coverage = key === 'coverage';
    if (!coverage && !labels.has(key)) key = 'fixes';
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
  for (const id of ['need-weight', 'views-weight', 'quality-band']) $(id).addEventListener('input', () => { page = 0; render(); });
  $('category-picker').addEventListener('change', () => { location.hash = $('category-picker').value; });
  $('reset').addEventListener('click', () => { $('need-weight').value = '60'; $('views-weight').value = '40'; $('quality-band').value = 'improve'; $('hide-panels').checked = false; $('hide-base').checked = false; $('search').value = ''; page = 0; if (active !== 'fixes') location.hash = 'fixes'; else render(); $('search').focus(); });
  $('expand-all').addEventListener('click', () => {
    expanded = !expanded;
    for (const detail of $('finding-list').children) {
      detail.open = expanded;
      if (expanded) openEntries.add(detail.dataset.entry); else openEntries.delete(detail.dataset.entry);
    }
    $('expand-all').hidden = true;
    $('expand-all').setAttribute('aria-pressed', String(expanded));
  });
  for (const [id, delta] of [['previous', -1], ['next', 1]]) $(id).addEventListener('click', () => { page += delta; selected = null; const params = new URLSearchParams(location.search); params.delete('finding'); history.replaceState(null, '', location.pathname + '?' + params + location.hash); expanded = false; render(); $('queue-title').scrollIntoView({block:'start'}); $(id).focus({preventScroll:true}); });
  document.addEventListener('keydown', event => {
    if (event.ctrlKey || event.metaKey || event.altKey || event.target.matches('input,textarea,select,[contenteditable]')) return;
    if (event.key === 'Escape') { pane.classList.remove('mobile-open'); document.querySelector('.finding-row[aria-current]')?.focus(); }
    if (['j','k'].includes(event.key) && !$('review').hidden) {
      event.preventDefault(); const rows = [...document.querySelectorAll('.finding-row')];
      const i = rows.findIndex(r => r.getAttribute('aria-current') === 'true');
      const row = rows[Math.max(0, Math.min(rows.length - 1, i + (event.key === 'j' ? 1 : -1)))];
      if (row) { row.click(); row.scrollIntoView({block:'nearest'}); }
    }
    if(event.key === 'e' && !$('review').hidden) pane.querySelector('.btn-primary')?.click();
    if (event.key === '/' && !event.ctrlKey && !event.metaKey && !event.altKey && !event.target.matches('input,textarea,select,[contenteditable]')) {
      event.preventDefault(); if ($('review').hidden) location.hash = active; $('search').focus();
    }
  });
  addEventListener('hashchange', route);
  const initial = new URLSearchParams(location.search);
  for (const [key, id] of [['need', 'need-weight'], ['views', 'views-weight']]) {
    if (initial.has(key) && /^\d+$/.test(initial.get(key))) $(id).value = String(Math.min(100, Number(initial.get(key))));
  }
  if (['improve', 'all', 'poor', 'partial', 'good'].includes(initial.get('quality'))) $('quality-band').value = initial.get('quality');
  $('search').value = initial.get('q') || '';
  $('hide-panels').checked = initial.get('panels') === 'hide';
  $('hide-base').checked = initial.get('base') === 'hide';
  if (['priority', 'page', 'category'].includes(initial.get('sort'))) $('sort').value = initial.get('sort');
  const nav = document.querySelector('nav[aria-label="Report sections"]');
  for (const [key, title] of [['all','All findings'],['documentation-quality','Documentation quality']]) {
    const link = create('a','tab',title); link.href = '#' + key; nav.append(link);
  }
  route();
  // Only hide the fallback record after successful initialization.
  document.documentElement.classList.add('js');
})();

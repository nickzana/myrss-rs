const $ = selector => document.querySelector(selector);
const $$ = selector => [...document.querySelectorAll(selector)];
const paths = {
  rss: '<path d="M5 5a14 14 0 0 1 14 14M5 11a8 8 0 0 1 8 8"/><circle cx="5" cy="19" r="1"/>',
  inbox: '<path d="m4 4-2 10v6h20v-6L20 4ZM2 14h6l2 3h4l2-3h6"/>',
  layers: '<rect x="5" y="3" width="16" height="16" rx="2"/><path d="M17 23H3a2 2 0 0 1-2-2V7"/>',
  star: '<path d="m12 3 2.8 5.7 6.3.9-4.5 4.4 1.1 6.2-5.7-3-5.7 3 1.1-6.2L3 9.6l6.2-.9Z"/>',
  folder: '<path d="M3 7V4h6l3 3h9v13H3Z"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  close: '<path d="m6 6 12 12M6 18 18 6"/>',
  menu: '<path d="M4 6h16M4 12h16M4 18h16"/>',
  search: '<circle cx="10.5" cy="10.5" r="6.5"/><path d="m16 16 5 5"/>',
  arrow: '<path d="M4 12h16m-6-6 6 6-6 6"/>',
  back: '<path d="M20 12H4m6-6-6 6 6 6"/>',
  chevron: '<path d="m9 6 6 6-6 6"/>',
  up: '<path d="m6 15 6-6 6 6"/>',
  down: '<path d="m6 9 6 6 6-6"/>',
  refresh: '<path d="M20 7a9 9 0 1 0 1 9M20 3v5h-5"/>',
  filter: '<path d="M3 5h18l-7 8v6l-4 2v-8Z"/>',
  cards: '<rect x="4" y="3" width="16" height="7" rx="1"/><rect x="4" y="14" width="16" height="7" rx="1"/>',
  list: '<path d="M8 5h13M8 12h13M8 19h13M3 5h.1M3 12h.1M3 19h.1"/>',
  settings: '<path d="m9 3-1 3-3 1-2 4 2 2v4l4 2 3-1 3 1 4-2v-4l2-2-2-4-3-1-1-3Z"/><circle cx="12" cy="12" r="3"/>',
  sliders: '<path d="M4 7h6m4 0h6M4 17h10m4 0h2"/><circle cx="12" cy="7" r="2"/><circle cx="16" cy="17" r="2"/>',
  circle: '<circle cx="12" cy="12" r="8"/>',
  check: '<path d="m5 12 4 4L19 6"/>',
  external: '<path d="M14 3h7v7M21 3 10 14M10 3H3v18h18v-7"/>',
  expand: '<path d="M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5"/>',
  book: '<path d="M12 5v16M12 5C9 2 4 3 2 4v16c4-2 7-1 10 1 3-2 6-3 10-1V4c-2-1-7-2-10 1Z"/>',
  upload: '<path d="M12 16V3m-5 5 5-5 5 5M3 16v5h18v-5"/>',
  download: '<path d="M12 3v13m-5-5 5 5 5-5M3 16v5h18v-5"/>',
  phone: '<rect x="6" y="2" width="12" height="20" rx="2"/><path d="M11 18h2"/>',
  keyboard: '<rect x="2" y="5" width="20" height="14" rx="2"/><path d="M6 9h.1M10 9h.1M14 9h.1M18 9h.1M6 12h.1M10 12h.1M14 12h.1M18 12h.1M7 16h10"/>',
  logout: '<path d="M9 3H3v18h6m6-15 6 6-6 6M8 12h13"/>',
  pause: '<path d="M8 5v14M16 5v14"/>',
  play: '<path d="m7 4 13 8-13 8Z"/>',
  trash: '<path d="M3 6h18M9 6V3h6v3M5 6l1 15h12l1-15M10 10v7M14 10v7"/>',
  alert: '<path d="m12 3 10 18H2ZM12 9v5M12 17h.1"/>',
};
function icon(name) {
  return '<svg class="icon" viewBox="0 0 24 24" aria-hidden="true">' + (paths[name] || paths.rss) + '</svg>';
}
$$('[data-icon]').forEach(el => { el.innerHTML = icon(el.dataset.icon); });

let csrf = '', filter = 'unread', feedId = null, folder = null, label = '', query = '';
let nextBefore = null, entries = [], selected = null, current = null, snapshot = null;
let listRequest = 0, articleRequest = 0, countRequest = 0, noticeTimer;
let loading = false;
const settingsHashes = ['#settings', '#appearance', '#subscriptions', '#reader-apps'];
const preferences = { theme: 'system', density: 'comfortable', textSize: 'regular' };
const preferenceOptions = { theme: ['system', 'light', 'dark'], density: ['comfortable', 'compact'], textSize: ['regular', 'large'] };
try {
  const saved = JSON.parse(localStorage.getItem('myrss.preferences') || '{}');
  for (const key of Object.keys(preferences)) {
    if (preferenceOptions[key].includes(saved?.[key])) preferences[key] = saved[key];
  }
} catch { /* Storage may be unavailable in private browsing. */ }
function applyPreferences() {
  document.documentElement.dataset.theme = preferences.theme;
  document.documentElement.dataset.density = preferences.density;
  document.documentElement.dataset.textSize = preferences.textSize;
  for (const key of Object.keys(preferences)) $('#' + key).value = preferences[key];
  $$('button[data-density]').forEach(b => b.setAttribute('aria-pressed', b.dataset.density === preferences.density));
}
function savePreference(key, value) {
  preferences[key] = value;
  applyPreferences();
  try { localStorage.setItem('myrss.preferences', JSON.stringify(preferences)); } catch { /* Optional persistence. */ }
}
applyPreferences();
for (const key of Object.keys(preferences)) $('#' + key).onchange = e => savePreference(key, e.target.value);
$$('button[data-density]').forEach(b => b.onclick = () => savePreference('density', b.dataset.density));

function esc(value) {
  return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
}
function date(value, short = false) {
  if (!value) return '';
  const d = new Date(value);
  return Number.isNaN(d.getTime()) ? '' : short
    ? d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' })
    : d.toLocaleString(undefined, { dateStyle: 'medium', timeStyle: 'short' });
}
function notify(message, error = false) {
  clearTimeout(noticeTimer);
  $('#noticeText').textContent = message;
  $('#notice').classList.toggle('error', error);
  $('#notice').hidden = false;
  if (!error) noticeTimer = setTimeout(() => { $('#notice').hidden = true; }, 6000);
}
$('#dismissNotice').onclick = () => { $('#notice').hidden = true; };
async function run(action, button) {
  if (button?.disabled) return;
  if (button) button.disabled = true;
  try { return await action(); }
  catch (err) { if (err.message !== 'Session expired') notify(err.message, true); }
  finally { if (button) button.disabled = false; }
}
async function request(path, options = {}) {
  const response = await fetch(path, { ...options, headers: {
    'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'X-MyRSS-CSRF': 'proxy', ...options.headers,
  } });
  if (response.status === 401) {
    await showLogin();
    throw Error('Session expired');
  }
  if (!response.ok) {
    const detail = (await response.text()).trim();
    throw Error(detail || 'Request failed (' + response.status + '). Please try again.');
  }
  return response;
}
async function api(path, options = {}) {
  const response = await request(path, options);
  return response.status === 204 ? null : response.json();
}
async function showLogin() {
  listRequest++; articleRequest++; countRequest++;
  setDrawer(false);
  $('#feedDialog').close();
  $('#login').hidden = false;
  $('#app').hidden = true;
  $('#credentialResult').hidden = true;
  $('#credOut').textContent = '';
  const mode = await fetch('/api/v1/auth-mode').then(r => r.json()).then(v => v.mode).catch(() => 'local');
  const sso = mode === 'oidc';
  $('#password').hidden = sso;
  $('#password').required = !sso;
  $('#passwordLabel').hidden = sso;
  $('#loginForm button').textContent = sso ? 'Continue with SSO' : 'Sign in';
}
async function enterApp(session) {
  csrf = session.mode === 'proxy' ? 'proxy' : session.csrf_token;
  $('#login').hidden = true;
  $('#app').hidden = false;
  route();
  await refresh();
}
async function boot() {
  const response = await fetch('/api/v1/session');
  if (!response.ok) { await showLogin(); return; }
  await enterApp(await response.json());
}
$('#loginForm').onsubmit = e => {
  e.preventDefault();
  if ($('#password').hidden) { location.href = '/auth/login'; return; }
  run(async () => {
    $('#loginError').textContent = '';
    const response = await fetch('/auth/login', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ password: $('#password').value }) });
    if (!response.ok) { $('#loginError').textContent = 'Could not sign in. Check your password and try again.'; return; }
    $('#password').value = '';
    await enterApp(await response.json());
  }, $('#loginForm button'));
};
$('#logout').onclick = () => run(async () => {
  await request('/auth/logout', { method: 'POST' });
  location.reload();
}, $('#logout'));

function setDrawer(open, restoreFocus = false) {
  $('#app').classList.toggle('drawer-open', open);
  $('#drawerBackdrop').hidden = !open;
  $('#menu').setAttribute('aria-expanded', open);
  $('#workspace').inert = open;
  $('#settings').inert = open;
  if (open) $('#closeMenu').focus();
  else if (restoreFocus) $('#menu').focus();
}
$('#menu').onclick = () => setDrawer(true);
$('#closeMenu').onclick = $('#drawerBackdrop').onclick = () => setDrawer(false, true);
matchMedia('(max-width: 760px)').addEventListener('change', () => setDrawer(false));
function route() {
  const settings = settingsHashes.includes(location.hash);
  const wasSettings = !$('#settings').hidden;
  $('#workspace').hidden = settings;
  $('#settings').hidden = !settings;
  $('#settingsLink').classList.toggle('active', settings);
  $('#settingsLink').setAttribute('aria-current', settings ? 'page' : 'false');
  setDrawer(false);
  if (settings && !$('#app').hidden) {
    if (!wasSettings) { $('#settings h1').focus({ preventScroll: true }); run(loadCreds); }
    if (location.hash !== '#settings') $(location.hash).scrollIntoView();
  }
  if (!settings && wasSettings) { backToList(false); $('#statusFilter').focus(); }
  if (!settings) { $('#credentialResult').hidden = true; $('#credOut').textContent = ''; }
  document.title = settings ? 'Settings · myrss' : ($('#viewTitle').textContent + ' · myrss');
}
window.addEventListener('hashchange', route);
async function refreshCounts() {
  const version = ++countRequest;
  const result = await api('/api/v1/snapshot');
  if (version !== countRequest) return;
  snapshot = result;
  renderNav();
  renderManagedFeeds();
}
async function refresh() {
  await refreshCounts();
  await loadEntries(true);
}
function renderNav() {
  if (!snapshot) return;
  $('#unreadCount').textContent = snapshot.unread;
  $('#totalCount').textContent = snapshot.total;
  $('#starredCount').textContent = snapshot.starred;
  $('#folders').innerHTML = snapshot.folders.map(f => '<button class="folder" data-folder="' + esc(f) + '">' + icon('folder') + '<span>' + esc(f) + '</span></button>').join('') || '<p class="sidebar-empty">Give your feeds a home.</p>';
  $('#folderSuggestions').innerHTML = snapshot.folders.map(f => '<option value="' + esc(f) + '"></option>').join('');
  $('#feeds').innerHTML = snapshot.feeds.map(f => '<button class="feed" data-id="' + f.id + '" title="' + esc(f.last_error || f.title) + '">' + icon(f.paused ? 'pause' : 'rss') + '<span class="feed-name">' + esc(f.title) + '</span>' + (f.last_error ? '<span class="feed-problem" aria-label="Feed refresh failed">' + icon('alert') + '</span>' : '') + '<span class="count">' + f.unread + '</span></button>').join('') || '<p class="sidebar-empty">Add your first favorite source.</p>';
  $$('.folder').forEach(b => b.onclick = () => chooseScope(null, b.dataset.folder));
  $$('.feed').forEach(b => b.onclick = () => chooseScope(Number(b.dataset.id), null));
  $('#labelFilterWrap').hidden = !snapshot.labels.length;
  $('#labelFilter').innerHTML = '<option value="">All labels</option>' + snapshot.labels.map(l => '<option value="' + esc(l) + '">' + esc(l) + '</option>').join('');
  $('#labelFilter').value = label;
  updateFilters();
}
function updateFilters() {
  const names = { unread: 'Unread', all: 'All articles', starred: 'Starred', read: 'Read' };
  const scope = feedId ? snapshot?.feeds.find(f => f.id === feedId)?.title : folder;
  $('#viewTitle').textContent = scope || names[filter];
  $('#statusFilter').value = filter;
  $$('.nav[data-filter]').forEach(b => {
    const active = !feedId && !folder && b.dataset.filter === filter;
    b.classList.toggle('active', active);
    b.setAttribute('aria-pressed', active);
  });
  $$('.feed').forEach(b => { const active = Number(b.dataset.id) === feedId; b.classList.toggle('active', active); b.setAttribute('aria-pressed', active); });
  $$('.folder').forEach(b => { const active = b.dataset.folder === folder; b.classList.toggle('active', active); b.setAttribute('aria-pressed', active); });
  $('#activeFilters').hidden = !(scope || query || label);
  $('#filterSummary').textContent = [scope, query && '“' + query + '”', label].filter(Boolean).join(' · ');
  if ($('#settings').hidden) document.title = $('#viewTitle').textContent + ' · myrss';
}
function chooseScope(id, name) {
  feedId = id; folder = name;
  location.hash = 'reader';
  setDrawer(false);
  backToList(false);
  run(() => loadEntries(true));
}
$$('.nav[data-filter]').forEach(b => b.onclick = () => {
  filter = b.dataset.filter; feedId = null; folder = null; label = ''; query = '';
  $('#q').value = ''; $('#labelFilter').value = '';
  chooseScope(null, null);
});
$('#statusFilter').onchange = e => { filter = e.target.value; run(() => loadEntries(true)); };
$('#labelFilter').onchange = e => { label = e.target.value; run(() => loadEntries(true)); };
$('#search').onsubmit = e => { e.preventDefault(); query = $('#q').value.trim(); run(() => loadEntries(true)); };
$('#q').addEventListener('search', () => { if (!$('#q').value) { query = ''; run(() => loadEntries(true)); } });
function clearFilters() {
  feedId = null; folder = null; query = ''; label = '';
  $('#q').value = ''; $('#labelFilter').value = '';
  run(() => loadEntries(true));
}
$('#clearFilters').onclick = clearFilters;
function emptyList(title, message, action = '') {
  return '<div class="empty-list">' + icon('inbox') + '<h3>' + esc(title) + '</h3><p>' + esc(message) + '</p>' + action + '</div>';
}
async function loadEntries(reset) {
  if (!reset && loading) return;
  const version = ++listRequest;
  loading = true;
  updateFilters();
  $('#entries').setAttribute('aria-busy', 'true');
  $('#more').disabled = true;
  if (reset) {
    entries = []; nextBefore = null;
    $('#entries').innerHTML = emptyList('Loading your articles…', 'Just a moment.');
    $('#more').hidden = true;
  }
  const params = new URLSearchParams({ filter, limit: '60' });
  if (feedId) params.set('feed_id', feedId);
  if (folder) params.set('folder', folder);
  if (label) params.set('label', label);
  if (query) params.set('q', query);
  if (nextBefore) params.set('before', nextBefore);
  try {
    const result = await api('/api/v1/entries?' + params);
    if (version !== listRequest) return;
    entries = entries.concat(result.entries);
    nextBefore = result.next_before;
    renderEntries();
    updateArticleActions();
  } catch (err) {
    if (version !== listRequest) return;
    if (reset) {
      $('#entries').innerHTML = emptyList('Couldn’t load articles', 'Check your connection and try again.', '<button id="retryEntries">Try again</button>');
      $('#retryEntries').onclick = () => run(() => loadEntries(true));
    }
    throw err;
  } finally {
    if (version === listRequest) { loading = false; $('#entries').setAttribute('aria-busy', 'false'); $('#more').disabled = false; }
  }
}
function renderEntries() {
  const focusedId = document.activeElement?.dataset?.entryId;
  $('#entries').innerHTML = entries.map(e =>
    '<button class="entry ' + (e.read ? 'read ' : '') + (selected === e.id ? 'selected' : '') + '" data-entry-id="' + e.id + '" aria-current="' + (selected === e.id) + '">' +
    '<span class="entry-meta"><span class="unread-dot" aria-label="' + (e.read ? 'Read' : 'Unread') + '"></span><span class="entry-feed">' + esc(e.feed_title) + '</span>' + (e.starred ? '<span class="entry-star" aria-label="Starred">' + icon('star') + '</span>' : '') + '<time title="' + esc(date(e.published_at || e.discovered_at)) + '">' + esc(date(e.published_at || e.discovered_at, true)) + '</time></span>' +
    '<h3>' + esc(e.title || 'Untitled article') + '</h3>' + (e.author ? '<span class="entry-author">' + esc(e.author) + '</span>' : '') + '</button>'
  ).join('');
  if (!entries.length) {
    const constrained = feedId || folder || query || label;
    const noFeeds = !snapshot?.feeds.length;
    $('#entries').innerHTML = emptyList(
      noFeeds ? 'Your next read starts here' : constrained ? 'No matching articles' : filter === 'unread' ? 'You’re all caught up' : filter === 'starred' ? 'Keep the good ones' : 'Nothing here yet',
      noFeeds ? 'Add a favorite source or import your subscriptions in Settings.' : constrained ? 'Try a different reading status or clear your filters.' : filter === 'starred' ? 'Star an article to find it here later.' : 'Pick another view, or refresh your feeds.',
      noFeeds ? '<button id="emptyAdd">' + icon('plus') + 'Add a feed</button>' : constrained ? '<button id="emptyClear">Clear filters</button>' : ''
    );
    if ($('#emptyAdd')) $('#emptyAdd').onclick = showFeedDialog;
    if ($('#emptyClear')) $('#emptyClear').onclick = clearFilters;
  }
  $$('[data-entry-id]').forEach(b => b.onclick = () => run(() => openArticle(Number(b.dataset.entryId))));
  $('#more').hidden = !nextBefore;
  $('#viewCount').textContent = entries.length + (nextBefore ? '+' : '') + ' article' + (entries.length === 1 ? '' : 's');
  if (focusedId) $('[data-entry-id="' + focusedId + '"]')?.focus({ preventScroll: true });
}
$('#more').onclick = () => run(() => loadEntries(false));
function updateArticleActions() {
  $('#articleActions').hidden = !current;
  if (!current) return;
  $('#star').setAttribute('aria-pressed', current.starred);
  $('#star').disabled = $('#markRead').disabled = pendingStates.has(current.id);
  $('#star').title = $('#star').ariaLabel = current.starred ? 'Unstar article (s)' : 'Star article (s)';
  $('#markRead').title = $('#markRead').ariaLabel = current.read ? 'Mark unread (m)' : 'Mark read (m)';
  $('#markRead').innerHTML = icon(current.read ? 'circle' : 'check');
  const i = entries.findIndex(e => e.id === selected);
  $('#previous').disabled = i <= 0;
  $('#next').disabled = i < 0 || i >= entries.length - 1;
}
async function openArticle(id) {
  const version = ++articleRequest;
  selected = id; current = null;
  renderEntries(); updateArticleActions();
  $('#workspace').classList.add('article-open');
  $('#readerEmpty').hidden = true;
  $('#article').hidden = false;
  $('#article').innerHTML = '<p class="article-meta" role="status">Opening article…</p>';
  $('#reader').scrollTop = 0;
  $('#reader').focus({ preventScroll: true });
  try {
    const entry = await api('/api/v1/entries/' + id);
    if (version !== articleRequest) return;
    current = entry;
    $('#article').innerHTML = '<div class="article-source">' + esc(entry.feed_title) + '</div><h1 class="article-title">' + esc(entry.title || 'Untitled article') + '</h1><div class="article-meta">' + [entry.author, date(entry.published_at || entry.discovered_at)].filter(Boolean).map(esc).join(' · ') + '</div><div class="article-content">' + (entry.content || entry.summary || '<p>No preview available. Open the original to read this article.</p>') + '</div>';
    const url = safeUrl(entry.url);
    $('#original').hidden = !url;
    if (url) $('#original').href = url;
    else $('#original').removeAttribute('href');
    updateArticleActions();
    if (!entry.read) await changeState(entry, { read: true });
  } catch (err) {
    if (version !== articleRequest) return;
    if (!current) {
      $('#article').innerHTML = emptyList('Couldn’t open this article', 'Try again, or choose another article.', '<button id="retryArticle">Try again</button>');
      $('#retryArticle').onclick = () => run(() => openArticle(id));
    }
    throw err;
  }
}
function safeUrl(value) {
  try { const url = new URL(value); return ['https:', 'http:'].includes(url.protocol) ? url.href : null; } catch { return null; }
}
const pendingStates = new Set();
async function changeState(entry, state) {
  if (pendingStates.has(entry.id)) return;
  pendingStates.add(entry.id);
  updateArticleActions();
  try {
    await api('/api/v1/entries/' + entry.id, { method: 'POST', body: JSON.stringify(state) });
    Object.assign(entry, state);
    const item = entries.find(e => e.id === entry.id);
    if (item) Object.assign(item, state);
    renderEntries(); updateArticleActions();
    await refreshCounts();
  } finally { pendingStates.delete(entry.id); updateArticleActions(); }
}
$('#star').onclick = () => { if (current) run(() => changeState(current, { starred: !current.starred }), $('#star')); };
$('#markRead').onclick = () => { if (current) run(() => changeState(current, { read: !current.read }), $('#markRead')); };
function moveArticle(delta) {
  const i = entries.findIndex(e => e.id === selected);
  const next = entries[i + delta];
  if (next) run(() => openArticle(next.id));
}
$('#previous').onclick = () => moveArticle(-1);
$('#next').onclick = () => moveArticle(1);
function setReaderFocus(focused) {
  $('#workspace').classList.toggle('reader-focused', focused);
  $('#focusReader').setAttribute('aria-pressed', focused);
  $('#focusReader').title = $('#focusReader').ariaLabel = focused ? 'Show article list' : 'Expand reader';
}
$('#focusReader').onclick = () => setReaderFocus(!$('#workspace').classList.contains('reader-focused'));
function backToList(focus = true) {
  articleRequest++;
  $('#workspace').classList.remove('article-open');
  setReaderFocus(false);
  if (focus) ($('[data-entry-id="' + selected + '"]') || $('#statusFilter')).focus({ preventScroll: true });
}
$('#backToList').onclick = () => backToList();

function showFeedDialog() {
  setDrawer(false);
  $('#feedError').textContent = '';
  $('#feedDialog').showModal();
  $('#feedUrl').focus();
}
$$('[data-add-feed]').forEach(b => b.onclick = showFeedDialog);
$('#closeFeedDialog').onclick = () => $('#feedDialog').close();
$('#addFeed').onsubmit = async e => {
  e.preventDefault();
  $('#feedError').textContent = '';
  $('#submitFeed').disabled = true;
  try {
    await api('/api/v1/feeds', { method: 'POST', body: JSON.stringify({ url: $('#feedUrl').value, folder: $('#feedFolder').value.trim() || null }) });
    $('#addFeed').reset(); $('#feedDialog').close();
    notify('Feed added. Articles will appear after it finishes fetching.');
    await refresh();
  } catch (err) { $('#feedError').textContent = 'Could not add this feed. Check the URL and try again. ' + err.message; }
  finally { $('#submitFeed').disabled = false; }
};
$('#refreshAll').onclick = () => run(async () => {
  $('#refreshAll').classList.add('spinning');
  try {
    const results = await Promise.allSettled((snapshot?.feeds || []).filter(f => !f.paused).map(f => api('/api/v1/feeds/' + f.id + '/refresh', { method: 'POST' })));
    const failed = results.filter(r => r.status === 'rejected').length;
    await refresh();
    notify(failed ? failed + ' feed(s) could not refresh. See Settings for details.' : 'Your feeds are up to date.', !!failed);
  } finally { $('#refreshAll').classList.remove('spinning'); }
}, $('#refreshAll'));
function renderManagedFeeds() {
  $('#managedFeeds').innerHTML = snapshot.feeds.map(f => '<div class="managed-feed">' + icon('rss') + '<div><strong>' + esc(f.title) + '</strong><small>' + esc([f.folder, f.paused ? 'Paused' : f.url].filter(Boolean).join(' · ')) + '</small>' + (f.last_error ? '<span class="error">' + esc(f.last_error) + '</span>' : '') + '</div><button class="icon-button" data-pause="' + f.id + '" aria-label="' + (f.paused ? 'Resume ' : 'Pause ') + esc(f.title) + '" title="' + (f.paused ? 'Resume feed' : 'Pause feed') + '">' + icon(f.paused ? 'play' : 'pause') + '</button><button class="icon-button" data-delete="' + f.id + '" aria-label="Remove ' + esc(f.title) + '" title="Remove feed">' + icon('trash') + '</button></div>').join('') || '<p>No subscriptions yet. Add a feed to get started.</p>';
  $$('[data-pause]').forEach(b => b.onclick = () => run(async () => {
    const feed = snapshot.feeds.find(f => f.id === Number(b.dataset.pause));
    await api('/api/v1/feeds/' + feed.id, { method: 'POST', body: JSON.stringify({ paused: !feed.paused, folder: feed.folder }) });
    await refreshCounts();
  }, b));
  $$('[data-delete]').forEach(b => b.onclick = () => run(async () => {
    const feed = snapshot.feeds.find(f => f.id === Number(b.dataset.delete));
    if (!confirm('Remove “' + feed.title + '” and all its articles? This cannot be undone.')) return;
    await api('/api/v1/feeds/' + feed.id, { method: 'DELETE' });
    if (feedId === feed.id) feedId = null;
    if (current?.feed_id === feed.id || entries.some(e => e.id === selected && e.feed_id === feed.id)) {
      articleRequest++; selected = null; current = null;
      $('#article').hidden = true; $('#readerEmpty').hidden = false;
      updateArticleActions(); backToList(false);
    }
    await refresh();
    notify('Subscription removed.');
  }, b));
}
$('#exportOpml').onclick = () => run(async () => {
  const text = await request('/api/v1/opml').then(r => r.text());
  const url = URL.createObjectURL(new Blob([text], { type: 'text/x-opml' }));
  const a = document.createElement('a'); a.href = url; a.download = 'myrss.opml'; a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}, $('#exportOpml'));
$('#importOpml').onclick = () => run(async () => {
  const file = $('#opmlFile').files[0];
  if (!file) { notify('Choose an OPML file first.', true); return; }
  const result = await api('/api/v1/opml', { method: 'POST', body: await file.text(), headers: { 'Content-Type': 'text/plain' } });
  $('#opmlFile').value = '';
  await refresh();
  notify(result.imported + ' subscription(s) imported. Refresh feeds to fetch articles.');
}, $('#importOpml'));
$('#credForm').onsubmit = e => {
  e.preventDefault();
  run(async () => {
    const c = await api('/api/v1/app-credentials', { method: 'POST', body: JSON.stringify({ name: $('#credName').value.trim() }) });
    $('#credOut').textContent = 'FreshRSS URL: ' + location.origin + '/api/greader.php\nFever URL: ' + location.origin + '/api/fever.php\nUsername: ' + c.username + '\nPassword: ' + c.password + '\nFever API key: MD5(username:password)';
    $('#credentialResult').hidden = false; $('#credName').value = '';
    await loadCreds();
  }, $('#credForm button'));
};
$('#dismissCredential').onclick = () => { $('#credentialResult').hidden = true; $('#credOut').textContent = ''; };
async function loadCreds() {
  const credentials = await api('/api/v1/app-credentials');
  $('#creds').innerHTML = credentials.map(c => '<div class="credential"><div><strong>' + esc(c.name) + '</strong><small>' + esc(c.username) + ' · ' + (c.enabled ? 'Active' : 'Revoked') + '</small></div>' + (c.enabled ? '<button data-revoke="' + c.id + '">Revoke</button>' : '') + '</div>').join('') || '<p>No apps connected yet.</p>';
  $$('[data-revoke]').forEach(b => b.onclick = () => run(async () => {
    if (!confirm('Revoke this credential? The app will no longer be able to sync.')) return;
    await api('/api/v1/app-credentials/' + b.dataset.revoke, { method: 'DELETE' });
    await loadCreds(); notify('App credential revoked.');
  }, b));
}
document.addEventListener('keydown', e => {
  if ($('#feedDialog').open) return;
  if ($('#app').classList.contains('drawer-open')) {
    if (e.key === 'Escape') { e.preventDefault(); setDrawer(false, true); }
    if (e.key === 'Tab') {
      const nodes = $$('#sidebar button, #sidebar a').filter(n => !n.disabled && n.getClientRects().length);
      const first = nodes[0], last = nodes[nodes.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
    return;
  }
  if ($('#app').hidden || !$('#settings').hidden || e.target.closest('input, textarea, select, [contenteditable="true"]') || e.ctrlKey || e.metaKey || e.altKey) return;
  if (e.key === 'Escape') backToList();
  if (e.key === 'j' || e.key === 'k') { e.preventDefault(); moveArticle(e.key === 'j' ? 1 : -1); }
  if (e.key === 's' && current) $('#star').click();
  if (e.key === 'm' && current) $('#markRead').click();
});
run(boot);

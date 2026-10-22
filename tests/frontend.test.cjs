const { test } = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const vm = require('node:vm');
const source = fs.readFileSync(require('node:path').join(__dirname, '../static/app.js'), 'utf8');

function setup(saved = '{}') {
  const elements = new Map();
  const element = selector => {
    if (!elements.has(selector)) {
      const classes = new Set();
      elements.set(selector, {
        hidden: false, value: '', innerHTML: '', textContent: '', disabled: false,
        dataset: {}, attrs: {}, classList: {
          add: name => classes.add(name), remove: name => classes.delete(name),
          contains: name => classes.has(name),
          toggle: (name, on) => on ? classes.add(name) : classes.delete(name),
        },
        setAttribute(name, value) { this.attrs[name] = value; },
        removeAttribute(name) { delete this.attrs[name]; },
        addEventListener() {}, focus() {}, scrollIntoView() {}, close() {},
      });
    }
    return elements.get(selector);
  };
  const context = vm.createContext({
    document: { querySelector: element, querySelectorAll: () => [], documentElement: { dataset: {} }, addEventListener() {}, activeElement: null },
    window: { addEventListener() {} },
    location: { hash: '#reader', origin: 'https://reader.example' },
    localStorage: { getItem: () => saved, setItem() {} },
    matchMedia: () => ({ addEventListener() {} }),
    fetch: () => new Promise(() => {}),
    URL, URLSearchParams, setTimeout, clearTimeout, console,
  });
  vm.runInContext(source, context);
  return { context, element, evaluate: code => vm.runInContext(code, context) };
}

test('preferences accept known values and recover from malformed storage', () => {
  let app = setup('{"theme":"dark","density":"compact","textSize":"large"}');
  assert.equal(app.context.document.documentElement.dataset.theme, 'dark');
  assert.equal(app.element('#density').value, 'compact');
  app = setup('{"theme":"invalid","density":null}');
  assert.equal(app.element('#theme').value, 'system');
  assert.equal(setup('broken').element('#density').value, 'comfortable');
});

test('reading status, feed, folder, labels and search compose in server requests', async () => {
  const app = setup();
  const urls = [];
  app.context.api = async url => { urls.push(url); return { entries: [], next_before: null }; };
  await app.evaluate("filter = 'starred'; feedId = 7; label = 'Read later'; query = 'quiet design'; loadEntries(true)");
  let params = new URL(urls[0], 'https://test').searchParams;
  assert.equal(params.get('filter'), 'starred');
  assert.equal(params.get('feed_id'), '7');
  assert.equal(params.get('label'), 'Read later');
  assert.equal(params.get('q'), 'quiet design');
  await app.evaluate("feedId = null; folder = 'Design & web'; filter = 'read'; loadEntries(true)");
  params = new URL(urls[1], 'https://test').searchParams;
  assert.equal(params.get('folder'), 'Design & web');
  assert.equal(params.get('filter'), 'read');
  assert.equal(params.has('feed_id'), false);
});

test('late list responses cannot overwrite a newer filter selection', async () => {
  const app = setup();
  const pending = [];
  app.context.api = () => new Promise(resolve => pending.push(resolve));
  const first = app.evaluate('loadEntries(true)');
  const second = app.evaluate("filter = 'all'; loadEntries(true)");
  pending[1]({ entries: [{ id: 2, title: 'New result', feed_title: 'Feed' }], next_before: null });
  await second;
  pending[0]({ entries: [{ id: 1, title: 'Stale result' }], next_before: null });
  await first;
  assert.equal(app.evaluate('entries[0].id'), 2);
  assert.match(app.element('#entries').innerHTML, /New result/);
  assert.doesNotMatch(app.element('#entries').innerHTML, /Stale result/);
});

test('pagination retains filters, appends entries, and blocks duplicate requests', async () => {
  const app = setup();
  let resolve, requested;
  app.context.api = url => { requested = url; return new Promise(r => { resolve = r; }); };
  app.evaluate("entries = [{id: 10, title: 'First'}]; nextBefore = 10; folder = 'Tech'; query = 'Rust'");
  const page = app.evaluate('loadEntries(false)');
  await app.evaluate('loadEntries(false)');
  const params = new URL(requested, 'https://test').searchParams;
  assert.equal(params.get('before'), '10');
  assert.equal(params.get('folder'), 'Tech');
  assert.equal(params.get('q'), 'Rust');
  resolve({ entries: [{ id: 9, title: 'Second' }], next_before: null });
  await page;
  assert.equal(app.evaluate('entries.length'), 2);
});

test('late article responses cannot replace the article currently being read', async () => {
  const app = setup();
  const pending = [];
  app.context.api = () => new Promise(resolve => pending.push(resolve));
  const first = app.evaluate('openArticle(1)');
  const second = app.evaluate('openArticle(2)');
  pending[1]({ id: 2, title: 'Current article', read: true, url: 'https://example.com/article' });
  await second;
  pending[0]({ id: 1, title: 'Old article', read: true });
  await first;
  assert.equal(app.evaluate('current.id'), 2);
  assert.match(app.element('#article').innerHTML, /Current article/);
});

test('opening an unread article updates local read state after the server succeeds', async () => {
  const app = setup();
  const calls = [];
  app.context.api = async (url, options) => {
    calls.push([url, options]);
    if (options?.method === 'POST') return null;
    if (url.endsWith('snapshot')) return { feeds: [], folders: [], labels: [], total: 1, unread: 0, starred: 0 };
    return { id: 1, title: 'Article', read: false };
  };
  app.evaluate("entries = [{id: 1, title: 'Article', read: false}]");
  await app.evaluate('openArticle(1)');
  assert.equal(app.evaluate('entries[0].read'), true);
  assert.equal(app.evaluate('current.read'), true);
  assert.equal(calls.filter(([, options]) => options?.method === 'POST').length, 1);
});

test('settings hash routes and mobile back navigation restore the reader list', () => {
  const app = setup();
  app.context.location.hash = '#subscriptions';
  app.evaluate('route()');
  assert.equal(app.element('#settings').hidden, false);
  assert.equal(app.element('#workspace').hidden, true);
  app.evaluate("setDrawer(true); backToList();");
  assert.equal(app.element('#workspace').classList.contains('article-open'), false);
  app.context.location.hash = '#reader';
  app.evaluate('route()');
  assert.equal(app.element('#workspace').hidden, false);
  assert.equal(app.element('#workspace').inert, false);
});

test('article titles are escaped and original links reject executable URLs', () => {
  const app = setup();
  assert.equal(app.evaluate("safeUrl('javascript:alert(1)')"), null);
  assert.equal(app.evaluate("safeUrl('https://example.com/a')"), 'https://example.com/a');
  assert.equal(app.evaluate("esc('<img onerror=alert(1)>')"), '&lt;img onerror=alert(1)&gt;');
});

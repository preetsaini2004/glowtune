const { readFileSync } = require('node:fs');
const { runInNewContext } = require('node:vm');
const { test } = require('node:test');
const assert = require('node:assert/strict');

const script = readFileSync(`${__dirname}/music_snapshot.js`, 'utf8');

function row(title, path, host, playing = false) {
  const link = new URL(path, `https://${host}`);
  link.textContent = title;
  return {
    querySelectorAll: () => [link],
    querySelector: selector => selector.includes('title') || selector.includes('internal-track-link')
      ? { textContent: title } : null,
    matches: () => playing,
  };
}

function read(host, rows, queued = [], title = '') {
  return runInNewContext(script, {
    location: { hostname: host, href: `https://${host}/` },
    document: {
      querySelector(selector) {
        if (selector.includes('queue-panel') && queued.length) return { querySelectorAll: () => queued };
        if (selector.includes('now-playing-widget')) return { textContent: title };
        return null;
      },
      querySelectorAll: selector => selector === 'a, button' ? [] : rows,
    },
  });
}

test('shelves deduplicate while queues preserve repeated songs after the current row', () => {
  const host = 'open.spotify.com';
  const current = row('Current song', '/track/current', host, true);
  const next = row('Next song', '/track/next', host);
  const result = read(host, [current, next, next], [current, next, next], 'Current song');
  assert.equal(result.items.length, 2);
  assert.equal(result.queue.length, 2);
  assert.equal(result.queue[0].title, 'Next song');
  assert.equal(result.queue[1].title, 'Next song');
  assert.equal(result.current, 'Current song');
  assert.equal(result.queueVisible, true);
});

test('provider links retain the correct music kind', () => {
  for (const [host, path, kind] of [
    ['music.youtube.com', '/browse/UC123', 'artist'],
    ['music.youtube.com', '/browse/VL123', 'playlist'],
    ['music.youtube.com', '/browse/MPRE123', 'album'],
    ['music.apple.com', '/us/album/name/123?i=456', 'song'],
    ['soundcloud.com', '/person/sets/set-name', 'playlist'],
  ]) {
    assert.equal(read(host, [row('Title', path, host)]).items[0]?.kind, kind, host + path);
  }
});

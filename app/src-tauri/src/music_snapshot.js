(() => {
  const text = (node) => (node?.textContent || '').replace(/\s+/g, ' ').trim().slice(0, 180);
  const host = location.hostname;
  const spotify = host === 'open.spotify.com';
  const youtube = host === 'music.youtube.com';
  const apple = host === 'music.apple.com';
  const service = spotify ? 'spotify' : youtube ? 'youtube-music' : apple ? 'apple-music' : 'soundcloud';
  // The heading of the shelf a card sits on (keeps "Made For You" apart from "Recently played").
  const SHELF = 'section, ytmusic-carousel-shelf-renderer, ytmusic-shelf-renderer, .shelf-grid, [class*="shelf"], .soundList, .lazyLoadingList';
  const sectionOf = (row) => {
    const shelf = typeof row.closest === 'function' ? row.closest(SHELF) : null;
    const heading = shelf?.querySelector('h2, [data-testid="section-header"], ytmusic-carousel-shelf-basic-header-renderer .title, .shelf-header__title, .soundList__title');
    return text(heading).slice(0, 60) || null;
  };
  const item = (row) => {
    const links = [...row.querySelectorAll('a[href]')];
    const link = links.find(a => /\/(track|album|playlist|artist|watch|browse|song)\b/.test(a.pathname))
      || (service === 'soundcloud' ? links.find(a => a.pathname.split('/').filter(Boolean).length >= 2) : null);
    if (!link) return null;
    const title = text(row.querySelector('[data-testid="internal-track-link"], .title, .soundTitle__title, [class*="title"], h3')) || text(link);
    if (!title) return null;
    const artistLink = links.find(a => a !== link && /\/(artist|channel)\//.test(a.pathname));
    const artist = text(artistLink) || text(row.querySelector('.secondary-flex-columns, .subtitle, .soundTitle__username, [class*="subtitle"]'));
    const img = row.querySelector('img');
    // Lazy images may only carry a srcset until they scroll into view.
    const srcset = (img?.getAttribute('srcset') || '').split(',').map(s => s.trim().split(' ')[0]).filter(Boolean);
    let artwork = img?.currentSrc || (img?.src && !img.src.startsWith('data:') ? img.src : '') || srcset[srcset.length - 1] || null;
    const background = row.querySelector('[style*="background-image"]');
    if (!artwork && background) artwork = background.style.backgroundImage.match(/url\(["']?(.*?)["']?\)/)?.[1] || null;
    let kind = 'song';
    if (link.pathname.includes('/playlist') || link.pathname.includes('/sets/')) kind = 'playlist';
    else if (link.pathname.includes('/artist/') || link.pathname.includes('/browse/UC')) kind = 'artist';
    else if (link.pathname.includes('/browse/VL')) kind = 'playlist';
    else if (link.pathname.includes('/album/') || link.pathname.includes('/browse/')) kind = 'album';
    if (apple && link.searchParams.has('i')) kind = 'song';
    return { service, kind, title, artist, artwork, url: link.href, section: sectionOf(row) };
  };
  // YouTube Music queue rows have no links (clicks are scripted): the video id
  // lives on the row element's data.
  const ytQueueItem = (row) => {
    const d = row.data || row.__data?.data;
    const id = d?.videoId || d?.navigationEndpoint?.watchEndpoint?.videoId;
    const title = text(row.querySelector?.('.song-title, .title'));
    if (!id || !title || !/^[\w-]{6,20}$/.test(id)) return item(row);
    const img = row.querySelector?.('img');
    return {
      service, kind: 'song', title,
      artist: text(row.querySelector?.('.byline, .subtitle')),
      artwork: img?.currentSrc || img?.src || null,
      url: `https://music.youtube.com/watch?v=${id}`,
      section: null,
    };
  };
  const collect = (rows, deduplicate = true, map = item) => {
    const seen = new Set();
    return [...rows].map(map)
      .filter(x => x && (!deduplicate || (!seen.has(x.url) && seen.add(x.url))))
      .slice(0, 120);
  };
  const selectors = spotify
    ? '[data-testid="card"], [data-testid="tracklist-row"], [data-encore-id="card"], [data-testid="rootlist-item"]'
    : youtube ? 'ytmusic-two-row-item-renderer, ytmusic-responsive-list-item-renderer'
    : apple ? '.shelf-grid__list-item, .songs-list-row, .track-lockup, .product-lockup'
    : '.soundList__item, .systemPlaylistListItem, .playableTile';
  // Spotify's queue panel: by its own markers first, else by its headings
  // ("Queue", "Next in queue", "Next from: …").
  const findSpotifyQueue = () => {
    const direct = document.querySelector('[data-testid="queue-panel"], [data-testid="queue-list"], [data-testid*="queue" i]:not(button):not(a), section[aria-label="Queue" i], [aria-label="Queue" i]:not(button):not(a)');
    if (direct) return direct;
    const label = [...document.querySelectorAll('h1, h2, h3, [data-encore-id="text"], span')]
      .find(n => n.childElementCount === 0 && /^(queue|next in queue|next from:?.*)$/i.test(text(n)));
    if (!label) return null;
    return label.closest('aside, [role="complementary"], [data-testid*="panel" i], [data-testid*="sidebar" i]')
      || label.parentElement?.parentElement?.parentElement || null;
  };
  // Spotify queue rows may not link the track itself: use the track link, else
  // the album link, with the row's title text and artist links.
  // Spotify's queue rows don't link the song: its id ("spotify:track:…") lives in
  // the page's own component data on the row (or an element inside it).
  const spotifyTrackId = (row) => {
    const els = [row, ...(row.querySelectorAll?.('*') || [])].slice(0, 12);
    for (const el of els) {
      const key = Object.keys(el).find(k => k.startsWith('__reactFiber') || k.startsWith('__reactInternalInstance'));
      let f = key ? el[key] : null;
      for (let i = 0; f && i < 25; i++, f = f.return) {
        const p = f.memoizedProps;
        if (!p || typeof p !== 'object') continue;
        for (const v of [p.uri, p.trackUri, p.item?.uri, p.track?.uri, p.item?.track?.uri, p.data?.uri, p.entity?.uri]) {
          const m = typeof v === 'string' && v.match(/^spotify:track:([A-Za-z0-9]{22})$/);
          if (m) return m[1];
        }
      }
    }
    return null;
  };
  const spQueueItem = (row) => {
    const links = [...(row.querySelectorAll?.('a[href]') || [])];
    const track = links.find(a => a.pathname?.startsWith('/track/'));
    const album = links.find(a => a.pathname?.startsWith('/album/'));
    const artists = links.filter(a => a.pathname?.startsWith('/artist/')).map(text).filter(Boolean);
    const title = text(row.querySelector?.('[data-testid="internal-track-link"], [data-encore-id="listRowTitle"], [data-encore-id="text"]')) || text(track);
    const id = track ? null : spotifyTrackId(row);
    const url = track?.href || (id ? `https://open.spotify.com/track/${id}` : null) || album?.href;
    if (!title || !url) return item(row);
    const img = row.querySelector?.('img');
    return { service, kind: 'song', title, artist: artists.join(', '), artwork: img?.currentSrc || img?.src || null, url, section: null };
  };
  // SoundCloud's "Next up" panel is always in the page, but its rows only
  // appear once it has been opened (and they stay after it closes): it counts
  // as readable only when it has rows.
  const scQueue = document.querySelector('.queue');
  // SoundCloud queue row: song link, uploader, and the cover (a background image,
  // asked for at 300 px instead of the row's 50 px).
  const scQueueItem = (row) => {
    const link = row.querySelector('.queueItemView__title a[href]');
    const title = text(link);
    if (!link || !title) return null;
    const bg = row.querySelector('[style*="background-image"]')?.style.backgroundImage || '';
    const art = bg.match(/url\(["']?(.*?)["']?\)/)?.[1] || null;
    return {
      service, kind: 'song', title,
      artist: text(row.querySelector('.queueItemView__username')),
      artwork: art ? art.replace(/-t\d+x\d+\./, '-t300x300.') : null,
      url: link.href,
      section: null,
    };
  };
  const queueRoot = spotify ? findSpotifyQueue()
    : youtube ? document.querySelector('ytmusic-player-queue')
    : apple ? document.querySelector('[aria-label="Playing Next"], .queue')
    : (scQueue?.querySelector('.queueItemView') ? scQueue : null);
  const queueRows0 = queueRoot ? [...queueRoot.querySelectorAll(spotify ? '[data-testid="tracklist-row"], [role="row"], [data-encore-id="listRow"]' : youtube ? (queueRoot.querySelector('ytmusic-player-queue-item') ? 'ytmusic-player-queue-item' : 'ytmusic-responsive-list-item-renderer') : apple ? '.songs-list-row, .track-lockup' : '.queueItemView:not(.m-dimmed)')] : [];
  // Rows can nest (a row inside a list row): keep only the innermost ones.
  const queueRows = spotify ? queueRows0.filter(r => !queueRows0.some(o => o !== r && r.contains?.(o))) : queueRows0;
  const current = spotify ? document.querySelector('[data-testid="now-playing-widget"] [data-testid="context-item-link"]')
    : youtube ? document.querySelector('ytmusic-player-bar .title')
    : apple ? document.querySelector('.lcd-meta__primary') : document.querySelector('.playbackSoundBadge__titleLink');
  // SoundCloud's title link text also holds a hidden "Current track:" label and
  // the title twice; its `title` attribute is the plain song title.
  const currentText = (service === 'soundcloud' && current?.getAttribute('title')) || text(current);
  // Only explicit current-row markers establish queue order. Never infer it from recommendations.
  const currentIndex = queueRows.findIndex(r => r.matches('[aria-current="true"], [playing], [selected], [play-button-state="playing"], [play-button-state="paused"], .queueItemView.m-active') || r.querySelector('[aria-current="true"], [data-testid="equaliser"]'));
  const upcoming = currentIndex >= 0 ? queueRows.slice(currentIndex + 1) : spotify && queueRoot ? queueRows : [];
  // When Spotify's queue can't be read, describe the panel (markers + headings only)
  // so the lookup can be fixed. Written to a local file by the app.
  let debug = null;
  const queueItems = collect(upcoming, false, youtube ? ytQueueItem : spotify ? spQueueItem : service === 'soundcloud' ? scQueueItem : item).filter(x => x.kind === 'song');
  if (spotify && !queueItems.length) {
    const scope = queueRoot || document.querySelector('aside, [role="complementary"]') || document.body;
    const seen = new Set();
    scope.querySelectorAll?.('[data-testid], [aria-label], [data-encore-id], [role="row"], h1, h2, h3').forEach(n => {
      if (seen.size >= 150) return;
      const heading = /^H[1-3]$/.test(n.tagName) ? ' text=' + text(n).slice(0, 30) : '';
      seen.add(`${n.tagName.toLowerCase()} testid=${n.getAttribute('data-testid') || ''} aria=${n.getAttribute('aria-label') || ''} role=${n.getAttribute('role') || ''} encore=${n.getAttribute('data-encore-id') || ''}${heading}`);
    });
    // Structure of the first rows (link paths, markers, whether an id was found): no song text.
    const rowInfo = queueRows.slice(0, 4).map((r, i) => {
      const paths = [...(r.querySelectorAll?.('a[href]') || [])].map(a => a.pathname.split('/').slice(0, 2).join('/')).join(',');
      const fiber = Object.keys(r).some(k => k.startsWith('__react'));
      return `row${i} tag=${r.tagName?.toLowerCase()} testid=${r.getAttribute?.('data-testid') || ''} role=${r.getAttribute?.('role') || ''} links=[${paths}] react=${fiber} id=${spotifyTrackId(r) ? 'yes' : 'no'}`;
    });
    debug = `root=${queueRoot ? queueRoot.tagName + ' ' + (queueRoot.getAttribute('data-testid') || queueRoot.getAttribute('aria-label') || '') : 'none'} rows=${queueRows0.length} upcoming=${upcoming.length} current=${currentIndex}\n` + rowInfo.join('\n') + '\n' + [...seen].join('\n');
  }
  const signIn = [...document.querySelectorAll('a, button')].some(n => /^(sign in|log in)$/i.test(text(n)) && n.getClientRects().length);
  return {
    pageUrl: location.href,
    items: collect(document.querySelectorAll(selectors)),
    queue: queueItems,
    current: currentText,
    debug,
    queueVisible: Boolean(queueRoot),
    // SoundCloud's "Autoplay station" switch (shown under the queue).
    autoplay: service === 'soundcloud' && Boolean(document.querySelector('.queueFallback__toggle input')?.checked),
    signIn,
  };
})()

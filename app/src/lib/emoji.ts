// Words that become emoji when they're sung (Scene menu → Lyrics → Emoji).
//
// Precision first: a missing emoji beats a wrong one. Only words whose
// meaning in lyrics is clear get one. English, plus Hindi and Punjabi as
// they're written in roman letters (most synced lyrics for those songs are).
// Left out on purpose: words that mean something else in Hinglish ("sun" is
// "listen", "sona" is "gold" or "sleep"), and anything about guns or dying.

const WORDS: Record<string, string> = {};
function add(emoji: string, words: string) {
  for (const w of words.split(" ")) if (w) WORDS[w] = emoji;
}

// Love and heartbreak
add("❤️", "love loved lovin loving lover lovers heart hearts dil pyaar pyar ishq mohabbat prem");
add("💞", "jaan jaanu");
add("💔", "heartbreak heartbroken heartbreaker dard");
add("💋", "kiss kisses kissed kissin kissing");
// Eyes, tears, smiles
add("👀", "eyes aankh aankhein ankhein akhiyan ankhiyan akhian nain naina");
add("😢", "tears cry cryin crying cried aansu hanju");
add("😊", "smile smiles smilin smiling muskaan");
add("😂", "laugh laughing laughin");
// Sky and weather
add("🌙", "moon moonlight chand chaand chann");
add("☀️", "sunshine sunlight sunrise");
add("⭐", "star stars taare tare sitare sitaare");
add("🌌", "sky skies galaxy aasmaan asmaan aasman");
add("🌧️", "rain rainin raining baarish barish");
add("❄️", "snow snowin snowing");
add("🥶", "cold freezing frozen");
add("🌊", "ocean oceans waves");
add("⚡", "lightning thunder electric");
add("🌃", "midnight raat raatan");
// Fire and shine
add("🔥", "fire flame flames burn burnin burning lit aag");
add("💎", "diamond diamonds");
add("🧊", "ice icy iced");
add("✨", "shine shinin shining sparkle glitter");
add("💧", "drip drippin dripping");
// Money and cars
add("💸", "money cash bands racks dollars bills guap paisa paise");
add("💰", "rich richer");
add("🏎️", "car cars whip benz lambo ferrari porsche gaddi gaadi");
add("✈️", "plane flight jet airport");
add("🚀", "rocket");
// Night out
add("🎉", "party partyin partying");
add("💃", "dance dancin dancing nach nachna nache");
add("🥂", "champagne cheers");
add("🥃", "drink drinks drinkin drinking daaru sharaab sharab");
add("🍷", "wine");
add("💨", "smoke smokin smoking");
add("🎵", "music song songs melody");
// People and the rest
add("👑", "king queen crown");
add("😈", "devil");
add("😇", "angel angels");
add("👻", "ghost ghosted ghostin");
add("🙏", "pray prayer prayers prayin praying rab khuda");
add("💭", "dream dreams dreamin dreaming sapna sapne khwab khwaab");
add("🌹", "rose roses gulaab gulab");
add("🌸", "flower flowers phool");
add("🦋", "butterfly butterflies");
add("🌍", "world");
add("🏠", "home ghar");
add("📱", "phone");
add("🤝", "yaar yaari dost");

/** The emoji for a sung word, or undefined (most words have none). */
export function emojiFor(word: string): string | undefined {
  const w = word
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[^a-z]/g, "");
  if (w.length < 2) return undefined;
  // NFKD splits accents off their letters; the filter then drops them.
  return WORDS[w] ?? WORDS[w.replace(/s$/, "")];
}

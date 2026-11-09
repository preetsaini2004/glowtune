// "Hide explicit words": masks swear words in displayed lyrics (the audio is
// untouched). Keeps the first letter so lines still read: "f***", "s***".
//
// Two lists, to avoid hiding innocent words:
//   ROOTS  match as word starts with any ending: "fuck" → fucking, fucked…
//   WORDS  match only as whole words (plus a plural "s"): "ass" but not "class"
//          or "assume".
// English plus common Punjabi / Hindi (romanized) swears, since that's what
// Glowtune's first listener plays most.

const ROOTS = [
  "fuck", "motherfuck", "fck", "fuk", "shit", "bitch", "bullshit", "cunt", "nigg", "dickhead",
  "pussy", "asshole", "bastard", "whore", "slut", "faggot", "retard", "cock", "jackass",
  // Punjabi / Hindi (romanized)
  "bhenchod", "behenchod", "bhanchod", "madarchod", "maderchod", "chutiya", "chutiye", "chootiya",
  "bhosdi", "bhosad", "gaand", "gandu", "lund", "lauda", "lavda", "randi", "harami", "kamina",
  "kanjar", "kutti", "teri maa", "pencho", "penchod",
];
const WORDS = ["ass", "dick", "hoe", "tits", "twat", "prick", "mc", "bc", "fu", "wtf", "stfu"];

function escape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

const PATTERN = new RegExp(
  `(?<![\\p{L}\\p{N}])(?:(?:${ROOTS.map(escape).join("|")})[\\p{L}]*|(?:${WORDS.map(escape).join("|")})s?(?![\\p{L}\\p{N}]))`,
  "giu",
);

function mask(word: string): string {
  const chars = [...word];
  return chars[0] + chars.slice(1).map((c) => (c === " " ? " " : "*")).join("");
}

/** The line with explicit words masked. */
export function clean(text: string): string {
  return text.replace(PATTERN, mask);
}

/** `clean(text)` when the setting is on, else the text as is. */
export function showLyric(text: string, on: boolean): string {
  return on && text ? clean(text) : text;
}

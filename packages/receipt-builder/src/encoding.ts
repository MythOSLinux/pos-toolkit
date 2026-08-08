/**
 * Character handling for ESC/POS thermal printers (extracted from medusa-pos).
 *
 * - "ascii"  (default): NFD-normalizes, strips combining marks (so "Crème" →
 *   "Creme") and anything outside printable ASCII. Safe on any printer
 *   regardless of firmware. (Diverges from medusa-pos's item-title sanitizer,
 *   which also stripped ASCII punctuation — receipts legitimately need
 *   "()", "*", "%", …)
 * - "utf8":  only strips raw ESC/POS control bytes; all Unicode passes through.
 *   Requires printer firmware with UTF-8 support (needed for e.g. Georgian).
 * - "cp852": maps Central-European characters to ASCII lookalikes, then "?"
 *   for anything else non-ASCII.
 * - "translit": romanizes non-Latin scripts, then falls through the ascii path.
 *
 * ## "translit" was "translit-ka", and that was a market leaking into a library
 *
 * The romanizing encoding shipped as `translit-ka`, labelled "Georgian to
 * Latin", because Georgian is where the need was found: the Rongta RP850P has
 * no UTF-8 mode and no Georgian codepage. But the option a venue is choosing is
 * "romanize what my printer cannot carry", which is the same choice in Kyiv,
 * Athens or Yerevan — and the Georgian table was the ONLY table, so every other
 * non-Latin script was silently stripped to nothing. Blank item names are worse
 * than romanized ones.
 *
 * So: `translit` is the encoding, `translit-ka` remains a working alias
 * (medusa-pos consumes this type, and venue config already holds the old
 * string), and romanization is table-driven per script.
 *
 * ## Why tables here and `any-ascii` as an injection
 *
 * `any-ascii` is the universal answer and was the first choice, until measured:
 * its data is one 546 KB module (`block.js`, ~200 KB gzipped), loaded whole,
 * not tree-shakeable. This library has no dependencies and ships as TypeScript
 * source — every consumer, including a future mobile app, would carry all of it.
 *
 * And most of what it buys is coverage a receipt should not use: romanizing
 * 北京烤鸭 to "beijing kaoya" gives a cook and a guest something neither
 * recognizes. A CJK market needs a printer with a native codepage, which is a
 * different feature. Romanization earns its keep on *alphabetic* scripts, where
 * a reader can sound the word back out — and those tables are a few KB.
 *
 * `setRomanizer` is the seam: a host that wants full-Unicode coverage passes
 * `any-ascii` in and pays for it deliberately.
 */

/** Encodings a printer row can be set to. `translit-ka` is the pre-2026-08-08
 *  name for `translit` and still resolves to it. */
export type PrinterEncoding = "ascii" | "utf8" | "cp852" | "translit" | "translit-ka";

/**
 * Georgian mkhedruli → Latin, the **2002 national transliteration system**.
 * Kept as a table rather than delegated: it is a published standard, and a
 * generic romanizer would quietly render ღ and ყ differently on the paper of
 * the market this product launches in.
 */
// prettier-ignore
const KA_TRANSLIT: Record<string, string> = {
  "ა": "a", "ბ": "b", "გ": "g", "დ": "d", "ე": "e", "ვ": "v", "ზ": "z",
  "თ": "t", "ი": "i", "კ": "k", "ლ": "l", "მ": "m", "ნ": "n", "ო": "o",
  "პ": "p", "ჟ": "zh", "რ": "r", "ს": "s", "ტ": "t", "უ": "u", "ფ": "p",
  "ქ": "k", "ღ": "gh", "ყ": "q", "შ": "sh", "ჩ": "ch", "ც": "ts", "ძ": "dz",
  "წ": "ts", "ჭ": "ch", "ხ": "kh", "ჯ": "j", "ჰ": "h",
};

/**
 * Cyrillic → Latin (BGN/PCGN-flavoured). Covers Russian, Ukrainian, Bulgarian
 * and Serbian letters, including the Ukrainian і/ї/є/ґ that plain Russian
 * tables miss.
 *
 * **Known limit: one table cannot serve every Cyrillic language.** `и` is "i"
 * in Russian and "y" in Ukrainian, so Київ romanizes here as "Kiyiv" rather
 * than the official "Kyiv". Readable, and it sounds right when a courier says
 * it back — which is the bar for a line on a receipt. Doing better needs the
 * document's locale threaded into `sanitizePrinterString`, which is a bigger
 * change than this encoding warrants until a venue asks.
 */
// prettier-ignore
const CYRILLIC_TRANSLIT: Record<string, string> = {
  "а": "a", "б": "b", "в": "v", "г": "g", "ґ": "g", "д": "d", "е": "e", "є": "ye",
  "ё": "yo", "ж": "zh", "з": "z", "и": "i", "і": "i", "ї": "yi", "й": "y", "к": "k",
  "л": "l", "м": "m", "н": "n", "о": "o", "п": "p", "р": "r", "с": "s", "т": "t",
  "у": "u", "ў": "u", "ф": "f", "х": "kh", "ц": "ts", "ч": "ch", "ш": "sh",
  "щ": "shch", "ъ": "", "ы": "y", "ь": "", "э": "e", "ю": "yu", "я": "ya",
  "ђ": "dj", "ј": "j", "љ": "lj", "њ": "nj", "ћ": "c", "џ": "dz", "ѕ": "dz",
};

/** Greek → Latin (ELOT 743 / UN-flavoured, without the digraph refinements a
 *  fixed-width receipt cannot justify). */
// prettier-ignore
const GREEK_TRANSLIT: Record<string, string> = {
  "α": "a", "β": "v", "γ": "g", "δ": "d", "ε": "e", "ζ": "z", "η": "i",
  "θ": "th", "ι": "i", "κ": "k", "λ": "l", "μ": "m", "ν": "n", "ξ": "x",
  "ο": "o", "π": "p", "ρ": "r", "σ": "s", "ς": "s", "τ": "t", "υ": "y",
  "φ": "f", "χ": "ch", "ψ": "ps", "ω": "o",
  "ά": "a", "έ": "e", "ή": "i", "ί": "i", "ό": "o", "ύ": "y", "ώ": "o",
  "ϊ": "i", "ϋ": "y", "ΐ": "i", "ΰ": "y",
};

/**
 * Greek digraphs, applied before the per-character pass.
 *
 * Single characters are not enough for Greek: ΕΛΟΤ 743 romanizes ου as "ou",
 * so a character table alone turns Σουβλάκι into "Soyvlaki" — a word no reader
 * recognizes, on the one line of a receipt that has to be recognized. These
 * three vowel pairs are the ones that actually occur in menu words; the
 * consonant clusters (μπ→b, ντ→d) are deliberately left out, since they are
 * ambiguous mid-word and a wrong guess reads worse than the literal letters.
 */
// prettier-ignore
const GREEK_DIGRAPHS: Array<[RegExp, string]> = [
  [/ου/g, "ou"], [/ού/g, "ou"], [/Ου/g, "Ou"], [/ΟΥ/g, "OU"],
  [/αυ/g, "av"], [/αύ/g, "av"], [/Αυ/g, "Av"], [/ΑΥ/g, "AV"],
  [/ευ/g, "ev"], [/εύ/g, "ev"], [/Ευ/g, "Ev"], [/ΕΥ/g, "EV"],
];

/** Armenian → Latin (BGN/PCGN). */
// prettier-ignore
const ARMENIAN_TRANSLIT: Record<string, string> = {
  "ա": "a", "բ": "b", "գ": "g", "դ": "d", "ե": "e", "զ": "z", "է": "e",
  "ը": "y", "թ": "t", "ժ": "zh", "ի": "i", "լ": "l", "խ": "kh", "ծ": "ts",
  "կ": "k", "հ": "h", "ձ": "dz", "ղ": "gh", "ճ": "ch", "մ": "m", "յ": "y",
  "ն": "n", "շ": "sh", "ո": "o", "չ": "ch", "պ": "p", "ջ": "j", "ռ": "r",
  "ս": "s", "վ": "v", "տ": "t", "ր": "r", "ց": "ts", "ւ": "w", "փ": "p",
  "ք": "k", "օ": "o", "ֆ": "f", "և": "ev",
};

/** Hebrew → Latin, consonantal (a receipt has no room for vowel pointing). */
// prettier-ignore
const HEBREW_TRANSLIT: Record<string, string> = {
  "א": "", "ב": "b", "ג": "g", "ד": "d", "ה": "h", "ו": "v", "ז": "z",
  "ח": "ch", "ט": "t", "י": "y", "כ": "k", "ך": "k", "ל": "l", "מ": "m",
  "ם": "m", "נ": "n", "ן": "n", "ס": "s", "ע": "", "פ": "p", "ף": "f",
  "צ": "ts", "ץ": "ts", "ק": "k", "ר": "r", "ש": "sh", "ת": "t",
};

/**
 * One lookup, built once. Upper-case forms are derived rather than typed out:
 * every script here is either caseless or has a mechanical `toLowerCase`, so a
 * hand-written upper table would only be a second place to make a typo.
 */
const TRANSLIT: Map<string, string> = (() => {
  const map = new Map<string, string>();
  for (const table of [KA_TRANSLIT, CYRILLIC_TRANSLIT, GREEK_TRANSLIT, ARMENIAN_TRANSLIT, HEBREW_TRANSLIT]) {
    for (const [char, latin] of Object.entries(table)) {
      map.set(char, latin);
      const upper = char.toUpperCase();
      // Georgian and Hebrew are caseless: toUpperCase() returns the same
      // character, and setting it again is a no-op rather than a wrong entry.
      if (upper !== char && !map.has(upper)) {
        map.set(upper, latin.charAt(0).toUpperCase() + latin.slice(1));
      }
    }
  }
  return map;
})();

/**
 * A host-supplied romanizer for characters no built-in table covers — pass
 * `any-ascii` here to buy full-Unicode coverage (546 KB of data) knowingly.
 * Returning "" or the input unchanged leaves the character to the ascii strip.
 */
export type Romanizer = (text: string) => string;

let injectedRomanizer: Romanizer | null = null;

/** Install (or clear, with `null`) the fallback romanizer. Process-wide, set
 *  once at startup — this is a print path, not a per-call option. */
export function setRomanizer(romanizer: Romanizer | null): void {
  injectedRomanizer = romanizer;
}

/** Which scripts romanize without a host romanizer — for a settings screen
 *  that wants to say so, and for tests. */
export function builtInTranslitScripts(): string[] {
  return ["Georgian", "Cyrillic", "Greek", "Armenian", "Hebrew"];
}

// prettier-ignore
const CP852_MAP: Record<string, string> = {
  "ą": "a", "ć": "c", "ę": "e", "ł": "l", "ń": "n", "ó": "o", "ś": "s", "ź": "z", "ż": "z",
  "Ą": "A", "Ć": "C", "Ę": "E", "Ł": "L", "Ń": "N", "Ó": "O", "Ś": "S", "Ź": "Z", "Ż": "Z",
  "á": "a", "à": "a", "â": "a", "ä": "a", "ã": "a", "å": "a",
  "Á": "A", "À": "A", "Â": "A", "Ä": "A", "Ã": "A", "Å": "A",
  "é": "e", "è": "e", "ê": "e", "ë": "e", "É": "E", "È": "E", "Ê": "E", "Ë": "E",
  "í": "i", "ì": "i", "î": "i", "ï": "i", "Í": "I", "Ì": "I", "Î": "I", "Ï": "I",
  "ú": "u", "ù": "u", "û": "u", "ü": "u", "Ú": "U", "Ù": "U", "Û": "U", "Ü": "U",
  "ö": "o", "ô": "o", "ò": "o", "õ": "o", "ø": "o",
  "Ö": "O", "Ô": "O", "Ò": "O", "Õ": "O", "Ø": "O",
  "ñ": "n", "Ñ": "N", "ý": "y", "ÿ": "y", "Ý": "Y", "ß": "ss",
  "č": "c", "Č": "C", "š": "s", "Š": "S", "ž": "z", "Ž": "Z",
  "ř": "r", "Ř": "R", "ď": "d", "Ď": "D", "ť": "t", "Ť": "T",
  "ľ": "l", "Ľ": "L", "ĺ": "l", "Ĺ": "L", "ŕ": "r", "Ŕ": "R",
  "ě": "e", "Ě": "E", "ů": "u", "Ů": "U", "ő": "o", "Ő": "O", "ű": "u", "Ű": "U",
  "ā": "a", "Ā": "A", "ē": "e", "Ē": "E", "ī": "i", "Ī": "I", "ū": "u", "Ū": "U",
  "ģ": "g", "Ģ": "G", "ķ": "k", "Ķ": "K", "ļ": "l", "Ļ": "L", "ņ": "n", "Ņ": "N",
};

// eslint-disable-next-line no-control-regex
const ESCPOS_CONTROL_BYTES = /[\x00-\x08\x0B\x0C\x0E-\x1F\x7F]/g;

/** Romanize with the built-in tables, then whatever the host injected. */
function romanize(text: string): string {
  let source = text;
  for (const [pattern, latin] of GREEK_DIGRAPHS) source = source.replace(pattern, latin);
  const tabled = [...source].map((ch) => TRANSLIT.get(ch) ?? ch).join("");
  if (!injectedRomanizer) return tabled;
  // Only the characters still outside ASCII are worth the injected pass.
  // eslint-disable-next-line no-control-regex
  return /[^\x00-\x7F]/.test(tabled) ? injectedRomanizer(tabled) : tabled;
}

export function sanitizePrinterString(
  text: string,
  encoding: PrinterEncoding = "ascii",
  onUnmapped?: (char: string) => void
): string {
  if (!text) return "";

  if (encoding === "utf8") {
    return text.replace(ESCPOS_CONTROL_BYTES, "").replace(/\s+/g, " ").trim();
  }

  if (encoding === "cp852") {
    const mapped = text
      .split("")
      .map((ch) => {
        if (CP852_MAP[ch] !== undefined) return CP852_MAP[ch];
        if (ch.charCodeAt(0) > 127) {
          onUnmapped?.(ch);
          return "?";
        }
        return ch;
      })
      .join("");
    return mapped.replace(ESCPOS_CONTROL_BYTES, "").replace(/\s+/g, " ").trim();
  }

  // "translit" (and its "translit-ka" alias): romanize what we can, then the
  // ascii path cleans whatever remains — other scripts, accents.
  const source = encoding === "translit" || encoding === "translit-ka" ? romanize(text) : text;

  // "ascii"
  const normalized = source.normalize("NFD");
  return normalized
    .replace(/[̀-ͯ]/g, "")
    .replace(/[^\x20-\x7E]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

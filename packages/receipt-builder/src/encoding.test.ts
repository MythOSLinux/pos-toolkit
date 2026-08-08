import { describe, expect, it, afterEach } from "vitest";
import { sanitizePrinterString, setRomanizer, builtInTranslitScripts } from "./encoding";

afterEach(() => setRomanizer(null));

describe("sanitizePrinterString", () => {
  it("ascii strips what a plain head cannot print, keeping receipt punctuation", () => {
    expect(sanitizePrinterString("Crème brûlée (2x)", "ascii")).toBe("Creme brulee (2x)");
    expect(sanitizePrinterString("ჩვენი დუქანი", "ascii")).toBe("");
  });

  it("utf8 passes script through and only drops control bytes", () => {
    expect(sanitizePrinterString("ჩვენი დუქანი", "utf8")).toBe("ჩვენი დუქანი");
    expect(sanitizePrinterString("a\x00b", "utf8")).toBe("ab");
  });

  it("cp852 maps Central European and question-marks the rest", () => {
    expect(sanitizePrinterString("Żółć", "cp852")).toBe("Zolc");
    expect(sanitizePrinterString("ბარი", "cp852")).toBe("????");
  });
});

describe("translit", () => {
  /**
   * Georgian is a published standard (2002 national transliteration) and the
   * launch market's script — these values are the contract, not a snapshot.
   */
  it("romanizes Georgian by the 2002 national system", () => {
    expect(sanitizePrinterString("ჩვენი დუქანი", "translit")).toBe("chveni dukani");
    expect(sanitizePrinterString("ღვინო", "translit")).toBe("ghvino");
    expect(sanitizePrinterString("ყველი", "translit")).toBe("qveli");
  });

  it("translit-ka is the old name for the same encoding", () => {
    // Venue config written before 2026-08-08 holds the old string, and
    // medusa-pos consumes this type — the alias has to keep working.
    expect(sanitizePrinterString("ჩვენი დუქანი", "translit-ka")).toBe(
      sanitizePrinterString("ჩვენი დუქანი", "translit")
    );
  });

  it("romanizes the other alphabetic scripts that used to strip to nothing", () => {
    // Each of these produced "" before the encoding stopped being Georgian-only.
    expect(sanitizePrinterString("Борщ", "translit")).toBe("Borshch");
    // "Kiyiv", not the official Ukrainian "Kyiv": и is "y" in Ukrainian and
    // "i" in Russian, and one table cannot know which language a line is in
    // (see the note on TRANSLIT). Readable and sounds right, which is the bar
    // for an item name on a receipt; getting it exactly right needs the
    // document's locale threaded into sanitizePrinterString.
    expect(sanitizePrinterString("Київ", "translit")).toBe("Kiyiv");
    // ου is a digraph: a character table alone gives "Soyvlaki", which no
    // reader recognizes (see GREEK_DIGRAPHS).
    expect(sanitizePrinterString("Σουβλάκι", "translit")).toBe("Souvlaki");
    expect(sanitizePrinterString("αυγολέμονο", "translit")).toBe("avgolemono");
    expect(sanitizePrinterString("Խորոված", "translit")).toBe("Khorovats");
    // Consonantal, so vowel-less and only just readable — but a cook reading
    // "shvvrmh" can still find the dish, and a blank line cannot.
    expect(sanitizePrinterString("שווארמה", "translit")).toBe("shvvrmh");
    expect(builtInTranslitScripts()).toContain("Cyrillic");
  });

  it("still falls through the ascii path for accents and stray scripts", () => {
    expect(sanitizePrinterString("Crème + ღვინო", "translit")).toBe("Creme + ghvino");
    // No table and no injected romanizer: stripped, exactly as before.
    expect(sanitizePrinterString("北京烤鸭", "translit")).toBe("");
  });

  it("an injected romanizer covers what the tables do not", () => {
    // What a host buys by passing any-ascii in: coverage beyond the built-ins,
    // paid for deliberately. Only the still-non-ASCII text reaches it.
    setRomanizer((text) => text.replace("北京烤鸭", "beijing kaoya"));
    expect(sanitizePrinterString("北京烤鸭", "translit")).toBe("beijing kaoya");
    // Built-in tables run first, so a standard stays a standard.
    expect(sanitizePrinterString("ღვინო", "translit")).toBe("ghvino");
  });

  it("does not call the injected romanizer when the tables already finished", () => {
    let calls = 0;
    setRomanizer((text) => {
      calls += 1;
      return text;
    });
    sanitizePrinterString("ჩვენი დუქანი", "translit");
    expect(calls).toBe(0);
  });
});

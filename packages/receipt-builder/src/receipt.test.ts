import { describe, expect, it } from "vitest";
import {
  buildReceiptText,
  centerText,
  padLine,
  PAPER_CONFIG,
  sanitizePrinterString,
  wrapText,
  type ReceiptDoc,
} from "./index";

/** GEL tetri → "12.50" (what a POS host would pass). */
const tetri = (minor: number) => (minor / 100).toFixed(2);

const doc: ReceiptDoc = {
  headerLines: ["Chveni Duqani", "12 Rustaveli Ave, Tbilisi", "Tel: +995 555 123 456"],
  title: "SALES RECEIPT",
  metaRows: [
    { label: "Date", value: "2026-07-17 14:05" },
    { label: "Order", value: "#A-102" },
    { label: "Table", value: "T4" },
  ],
  itemsHeading: "ITEMS",
  items: [
    { title: "Khinkali (beef)", qty: 5, unitPrice: 250 },
    {
      title: "Kharcho",
      qty: 1,
      unitPrice: 1600,
      total: 1800,
      sublines: [{ text: "+ extra walnuts", amount: 200 }, { text: "no cilantro" }],
    },
  ],
  totalRows: [
    { label: "VAT 18% (incl.)", amount: 466 },
    { label: "Total", amount: 3050 },
  ],
  paymentRows: [
    { label: "Cash", amount: 4000 },
    { label: "Change", amount: 950 },
  ],
  footerLines: ["Thank you for your visit!"],
};

describe("buildReceiptText", () => {
  it("lays out an 80mm receipt with padded money rows", () => {
    const text = buildReceiptText(doc, { formatAmount: tetri });
    const lines = text.split("\n");

    for (const line of lines) {
      expect(line.length).toBeLessThanOrEqual(PAPER_CONFIG["80mm"].lineWidth);
    }
    expect(text).toContain("=".repeat(48));
    expect(lines.find((l) => l.startsWith("Khinkali"))).toMatch(/Khinkali \(beef\) +12\.50$/);
    expect(text).toContain("  5 x 2.50");
    // explicit item total wins over qty × unit
    expect(lines.find((l) => l.startsWith("Kharcho"))).toMatch(/18\.00$/);
    expect(lines.find((l) => l.includes("extra walnuts"))).toMatch(/2\.00$/);
    expect(text).toContain("  no cilantro");
    expect(lines.find((l) => l.startsWith("Total:"))).toMatch(/Total: +30\.50$/);
    expect(lines.find((l) => l.startsWith("Change:"))).toMatch(/9\.50$/);
  });

  it("truncates long item titles to the 57mm budget", () => {
    const text = buildReceiptText(
      {
        headerLines: [],
        items: [{ title: "An extremely long menu item name that cannot fit", qty: 1, unitPrice: 100 }],
        totalRows: [],
      },
      { formatAmount: tetri, paperWidth: "57mm" }
    );
    const itemLine = text.split("\n").find((l) => l.startsWith("An "))!;
    expect(itemLine.length).toBeLessThanOrEqual(PAPER_CONFIG["57mm"].lineWidth);
    expect(itemLine).toContain("An extremely long".substring(0, PAPER_CONFIG["57mm"].maxItemTitleLen).trim());
  });

  it("renders a named payment row beside the money rows", () => {
    const text = buildReceiptText(
      {
        headerLines: [],
        items: [],
        totalRows: [{ label: "Total", amount: 3050 }],
        paymentRows: [
          { label: "Payment Method", value: "Cash" },
          { label: "Amount Paid", amount: 4000 },
        ],
      },
      { formatAmount: tetri }
    );
    const lines = text.split("\n");
    expect(lines.find((l) => l.startsWith("Payment Method:"))).toMatch(/Payment Method: +Cash$/);
    expect(lines.find((l) => l.startsWith("Amount Paid:"))).toMatch(/40\.00$/);
  });

  it("reports cp852 characters it had to replace", () => {
    const unmapped: string[] = [];
    buildReceiptText(
      {
        headerLines: ["Тбилиси"],
        items: [{ title: "Khinkali", qty: 1, unitPrice: 250 }],
        totalRows: [],
      },
      { formatAmount: tetri, encoding: "cp852", onUnmapped: (c) => unmapped.push(c) }
    );
    expect(unmapped).toContain("Т");
    expect(unmapped.length).toBeGreaterThan(0);
  });

  it("renders centered messages (unpaid banner)", () => {
    const text = buildReceiptText(
      { headerLines: [], items: [], totalRows: [], messages: ["** UNPAID **"] },
      { formatAmount: tetri }
    );
    expect(text).toContain(centerText("** UNPAID **", 48));
  });

  it("passes Georgian through on utf8 and strips it on ascii", () => {
    const georgian: ReceiptDoc = {
      headerLines: ["ჩვენი დუქანი"],
      items: [{ title: "ხინკალი", qty: 3, unitPrice: 250 }],
      totalRows: [],
    };
    expect(buildReceiptText(georgian, { formatAmount: tetri, encoding: "utf8" })).toContain("ხინკალი");
    expect(buildReceiptText(georgian, { formatAmount: tetri, encoding: "ascii" })).not.toContain("ხინკალი");
  });
});

describe("helpers", () => {
  it("padLine keeps at least one space when overflowing", () => {
    expect(padLine("aaaaaaaaaa", "bbbbbbbbbb", 10)).toBe("aaaaaaaaaa bbbbbbbbbb");
  });

  it("wrapText wraps greedily and hard-breaks long words", () => {
    expect(wrapText("thank you for your visit", 10)).toEqual(["thank you", "for your", "visit"]);
    expect(wrapText("supercalifragilistic", 10)).toEqual(["supercalif", "ragilistic"]);
  });

  it("sanitizePrinterString maps cp852 and flags unmapped chars", () => {
    const unmapped: string[] = [];
    expect(sanitizePrinterString("Żubrówka łagodna", "cp852", (c) => unmapped.push(c))).toBe(
      "Zubrowka lagodna"
    );
    expect(unmapped).toEqual([]);
    expect(sanitizePrinterString("ხინკალი", "cp852", (c) => unmapped.push(c))).toBe("???????");
    expect(unmapped).toHaveLength(7);
  });

  it("translit-ka maps Georgian to the 2002 national Latin transliteration", () => {
    expect(sanitizePrinterString("ხინკალი", "translit-ka")).toBe("khinkali");
    expect(sanitizePrinterString("საფერავის სანგრია", "translit-ka")).toBe("saperavis sangria");
    // mixed input: Latin passes through, leftover non-ASCII still strips
    expect(sanitizePrinterString("Qvevri ღვინო №5", "translit-ka")).toBe("Qvevri ghvino 5");
  });

  it("ascii sanitize strips accents like the medusa-pos original", () => {
    expect(sanitizePrinterString("Crème brûlée", "ascii")).toBe("Creme brulee");
  });
});

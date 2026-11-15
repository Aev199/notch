import { describe, expect, it } from "vitest";
import { direction, previewText } from "./api";

describe("direction", () => {
  // The same cases as crates/core/src/translate.rs: both sides must agree.
  it("treats Cyrillic-heavy text as Russian", () => {
    expect(direction("Привет, мир")).toEqual(["ru", "en"]);
    expect(direction("Запусти cargo build и проверь")).toEqual(["ru", "en"]);
  });
  it("treats everything else as English", () => {
    expect(direction("Hello world")).toEqual(["en", "ru"]);
    expect(direction("A long English sentence with one слово in it")).toEqual(["en", "ru"]);
    expect(direction("12345")).toEqual(["en", "ru"]);
  });
});

describe("previewText", () => {
  it("shows copied code compactly without changing what is stored", () => {
    expect(previewText("def f():\r\n\tif x:\r\n\n\n\t\treturn  1\r\n")).toBe("def f():\nif x:\nreturn 1");
    expect(previewText("a\tb\u00a0\u00a0c")).toBe("a b c");
  });
  it("keeps only the first lines", () => {
    expect(previewText("1\n2\n3\n4\n5\n6")).toBe("1\n2\n3\n4");
  });
});

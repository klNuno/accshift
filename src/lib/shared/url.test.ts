import { describe, it, expect } from "vitest";
import { isSafeHttpUrl } from "./url";

describe("isSafeHttpUrl", () => {
  it("returns true for https:// URLs", () => {
    expect(isSafeHttpUrl("https://example.com")).toBe(true);
    expect(isSafeHttpUrl("https://example.com/path?q=1")).toBe(true);
  });

  it("returns true for http:// URLs", () => {
    expect(isSafeHttpUrl("http://example.com")).toBe(true);
  });

  it("returns false for javascript: URLs", () => {
    expect(isSafeHttpUrl("javascript:alert(1)")).toBe(false);
  });

  it("returns false for data: URLs", () => {
    expect(isSafeHttpUrl("data:text/html,<h1>hi</h1>")).toBe(false);
  });

  it("returns false for empty strings", () => {
    expect(isSafeHttpUrl("")).toBe(false);
  });

  it("returns false for malformed URLs", () => {
    expect(isSafeHttpUrl("not a url at all")).toBe(false);
    expect(isSafeHttpUrl("://missing-scheme")).toBe(false);
  });

  it("returns false for ftp:// URLs", () => {
    expect(isSafeHttpUrl("ftp://files.example.com/readme.txt")).toBe(false);
  });
});

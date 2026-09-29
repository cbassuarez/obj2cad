import { describe, expect, it } from "vitest";
import { explain, problemUrl } from "@/lib/errors";
globalThis.__APP_VERSION__ = "0.0.0-test";
const parse = (kind, line = 7) => ({ file: "model.obj", kind, line, message: "raw engine text", issues: [{ line, kind, message: "raw engine text" }], truncated: false });
describe("error copy", () => {
    it("names the cause and the line, and keeps the raw line underneath", () => {
        const e = explain({ kind: "parse", parse: parse("comma_decimal", 1234) });
        expect(e.title).toBe("Line 1,234 uses a comma as the decimal point");
        expect(e.lines).toEqual(["Line 1,234: raw engine text"]);
        expect(explain({ kind: "parse", parse: parse("index_out_of_range") }).title).toMatch(/vertex that doesn't exist/);
        expect(explain({ kind: "parse", parse: parse("hidden_characters") }).title).toMatch(/invisible characters/);
    });
    it("gives every parse error kind its own title", () => {
        const kinds = ["comma_decimal", "invalid_number", "non_finite", "wrong_arity", "index_out_of_range", "hidden_characters", "encoding", "too_large", "ambiguous_columns"];
        const titles = new Set(kinds.map((k) => explain({ kind: "parse", parse: parse(k) }).title));
        expect(titles.size).toBe(kinds.length);
    });
    it("offers a report only for obj2cad's own failures", () => {
        expect(explain({ kind: "parse", parse: parse("invalid_number") }).report).toBeUndefined();
        expect(explain({ kind: "crash", message: "unreachable" }).report).toBe(true);
        expect(explain({ kind: "engine", message: "no wasm" }).report).toBe(true);
        expect(explain({ kind: "parse", parse: parse("too_large") }).cli).toBe(true);
    });
    it("prefills an issue with version and error, never file contents", () => {
        const url = new URL(problemUrl("Crash", "unreachable"));
        expect(url.pathname).toBe("/cbassuarez/obj2cad/issues/new");
        const body = url.searchParams.get("body");
        expect(body).toContain("unreachable");
        expect(body).toContain("obj2cad 0.0.0-test");
    });
});

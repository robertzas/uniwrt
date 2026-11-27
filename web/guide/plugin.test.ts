import { describe, expect, it } from "vitest";
import { literal } from "./plugin";

describe("the guide's plugin", () => {
  it("writes literals that no text can break out of", () => {
    const text = `</script><!-- a/b ${String.fromCharCode(0x2028, 0x2029)}"`;
    const code = literal({ html: text });
    expect(code).not.toMatch(new RegExp(`[<>/${String.fromCharCode(0x2028, 0x2029)}]`));
    expect(new Function(`return ${code}`)()).toEqual({ html: text });
  });
});

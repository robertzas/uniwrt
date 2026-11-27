/**
 * The user guide, rendered at build time by `web/guide/plugin.ts` (D-151). The shapes are
 * `HelpChapter` and `HelpSection` of `lib/help.ts` (an ambient module can't import them).
 */
declare module "virtual:uniwrt-guide" {
  /** Every chapter, the contents (`index`) first, in the order the contents link them. */
  export const chapters: readonly {
    slug: string;
    title: string;
    sections: readonly { anchor: string; text: string; depth: number }[];
  }[];
  /** A chapter's HTML, or `undefined` for a chapter the guide doesn't have. */
  export function loadChapter(slug: string): Promise<string | undefined>;
}

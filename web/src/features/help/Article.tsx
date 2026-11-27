import { createEffect } from "solid-js";
import { type HelpTopic, parseHelpHref } from "../../lib/help";

/**
 * A chapter of the guide, as the build rendered it (trusted HTML: `web/guide/render.ts` escapes
 * the Markdown's raw HTML). Shows `anchor`'s section at the top of `scroller` (the panel's
 * body), or of the window. Links to other help topics go to `onNavigate`; the rest open in a
 * new tab.
 */
export function Article(props: {
  html: string;
  anchor?: string;
  onNavigate: (topic: HelpTopic) => void;
  scroller?: () => HTMLElement | undefined;
  class?: string;
}) {
  let root: HTMLDivElement | undefined;
  createEffect(
    () => [props.html, props.anchor] as const,
    ([, anchor]) => {
      requestAnimationFrame(() => {
        if (root) reveal(root, anchor, props.scroller?.());
      });
    },
  );
  const onClick = (event: MouseEvent) => {
    if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) {
      return;
    }
    const link = event.target instanceof Element ? event.target.closest("a") : null;
    const topic = parseHelpHref(link?.getAttribute("href") ?? "");
    if (!topic) return;
    event.preventDefault();
    props.onNavigate(topic);
  };
  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents lint/a11y/noStaticElementInteractions: it only catches the clicks of its links, which Enter fires too.
    <div ref={root} class={["guide", props.class]} innerHTML={props.html} onClick={onClick} />
  );
}

function reveal(root: HTMLElement, anchor: string | undefined, scroller: HTMLElement | undefined) {
  const target = anchor ? root.querySelector(`[data-anchor="${CSS.escape(anchor)}"]`) : null;
  if (scroller) {
    const top = target
      ? target.getBoundingClientRect().top -
        scroller.getBoundingClientRect().top +
        scroller.scrollTop
      : 0;
    scroller.scrollTop = Math.max(top - 12, 0);
  } else if (target) {
    target.scrollIntoView({ block: "start" });
  } else {
    root.ownerDocument.defaultView?.scrollTo?.({ top: 0 });
  }
}

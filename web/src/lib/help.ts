/**
 * In-app help (D-151): where each page's help is in the user guide, and the `/help` paths. The
 * guide itself (`docs/guide/`) is rendered at build time (`web/guide/`) and loads only when
 * help opens. Anchors are GitHub's, so they match the repo's copy; `help.test.ts` checks that
 * every one here exists, so renaming a heading in the guide fails the tests.
 */

/** A chapter of the guide (`devices`, or `index` for its contents) and maybe a section in it. */
export interface HelpTopic {
  chapter: string;
  anchor?: string;
}

/** A section of a chapter, as the build lists them. */
export interface HelpSection {
  anchor: string;
  text: string;
  /** 2 for a chapter's sections, 3 for theirs, … */
  depth: number;
}

export interface HelpChapter {
  slug: string;
  title: string;
  sections: readonly HelpSection[];
}

export const HELP_PATH = "/help";
export const HELP_INDEX = "index";

/** `/help`, `/help/devices` or `/help/devices#discovered`. */
export function helpHref(topic: HelpTopic): string {
  const path = topic.chapter === HELP_INDEX ? HELP_PATH : `${HELP_PATH}/${topic.chapter}`;
  return topic.anchor ? `${path}#${topic.anchor}` : path;
}

/** The topic of a `/help` link, or `undefined` for any other. */
export function parseHelpHref(href: string): HelpTopic | undefined {
  const match = /^\/help(?:\/([\w-]+))?\/?(?:#([\w-]+))?$/.exec(href);
  if (!match) return undefined;
  return { chapter: match[1] ?? HELP_INDEX, ...(match[2] && { anchor: match[2] }) };
}

/** Whether a path is the help center's. */
export function isHelpPath(pathname: string): boolean {
  return pathname === HELP_PATH || pathname.startsWith(`${HELP_PATH}/`);
}

const topic = (ref: string): HelpTopic => {
  const [chapter = HELP_INDEX, anchor] = ref.split("#");
  return { chapter, ...(anchor && { anchor }) };
};

/** Each page's section, by path. */
const pages: Readonly<Record<string, string>> = {
  "/": "dashboard-topology#dashboard",
  "/topology": "dashboard-topology#topology",
  "/devices": "devices#devices",
  "/clients": "clients#clients-page",
  "/wifi": "wifi#wi-fi",
  // The title "# Networks" takes `networks`, so GitHub numbers the page's section.
  "/networks": "networks#networks-1",
  "/internet": "internet#connection",
  "/security": "security-vpn#security",
  "/vpn": "security-vpn#vpn",
  "/insights/logs": "insights#logs",
  "/insights/events": "insights#events",
  "/insights/alerts": "insights#alerts",
  "/insights/metrics": "insights#metrics",
  "/insights/rf": "insights#rf",
  "/insights/floor-plans": "insights#floor-plans",
  "/updates": "updates#controller",
  "/settings": "settings#site",
  "/changes": "changes#review-changes",
  "/settings/import": "changes#import-from-the-devices",
  "/settings/import/file": "changes#import-a-site-file",
};

/** Pages under a path (`/devices/3`), by the path's prefix. */
const subpages: readonly [prefix: string, ref: string][] = [
  ["/devices/", "devices#device-page"],
  ["/clients/", "clients#client-page"],
];

/** Pages whose `?tab=` names a section of its own, by path: each tab's section. */
const tabs: Readonly<Record<string, Readonly<Record<string, string>>>> = {
  "/devices/": {
    overview: "devices#overview",
    ports: "devices#ports",
    mesh: "devices#mesh",
    config: "devices#config",
    tools: "devices#tools",
    terminal: "devices#terminal",
    uci: "devices#uci",
  },
  "/insights/alerts": {
    active: "insights#active",
    history: "insights#history",
    rules: "insights#rules",
    maintenance: "insights#maintenance",
  },
  "/settings": {
    site: "settings#site",
    notifications: "settings#notifications",
    revisions: "settings#revisions",
    backups: "settings#backups",
    support: "settings#support",
  },
};

/**
 * The guide's section for a page: its tab's (`?tab=`), else its own, else its section's; the
 * guide's contents for a page the guide has no section for. `undefined` on the help center.
 */
export function helpTopic(pathname: string, search = ""): HelpTopic | undefined {
  const path = pathname.length > 1 ? pathname.replace(/\/+$/, "") : pathname;
  if (isHelpPath(path)) return undefined;
  const sub = subpages.find(([prefix]) => path.startsWith(prefix));
  const key = pages[path] ? path : sub?.[0];
  const tab = new URLSearchParams(search).get("tab");
  const ref = (key && tab && tabs[key]?.[tab]) || pages[path] || sub?.[1];
  return ref ? topic(ref) : { chapter: HELP_INDEX };
}

/** Every topic the pages point at, for the test that each exists in the guide. */
export function helpTopics(): HelpTopic[] {
  return [
    ...Object.values(pages),
    ...subpages.map(([, ref]) => ref),
    ...Object.values(tabs).flatMap((t) => Object.values(t)),
  ].map(topic);
}

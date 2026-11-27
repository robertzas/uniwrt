import {
  BookOpen,
  CircleHelp,
  Laptop,
  LogOut,
  Moon,
  Palette,
  Plus,
  RadioTower,
  Router,
  Sun,
  Upload,
} from "lucide";
import type { Client } from "../api/gen/Client";
import type { NodeSummary } from "../api/gen/NodeSummary";
import {
  type Skin,
  setCurrentSkin,
  setThemePreference,
  skins,
  type ThemePreference,
} from "../lib/appearance";
import { clientName, kindIcons } from "../lib/clients";
import { HELP_INDEX, HELP_PATH, type HelpChapter, type HelpTopic } from "../lib/help";
import { strings } from "../lib/strings";
import type { IconNode } from "../ui/Icon";
import { navItems } from "./nav";

/** An entry in the ⌘K palette. Features add theirs here as they land. */
export interface Command {
  id: string;
  group: string;
  label: string;
  icon: IconNode;
  /** Extra words that should find it, e.g. the page's description. */
  keywords?: string;
  /** Shown muted after the label, e.g. a guide section's chapter. */
  hint?: string;
  /** Listed only once the query finds it (the guide's sections), not in the empty palette. */
  searchOnly?: boolean;
  run: () => void;
}

const themeIcons: Record<ThemePreference, IconNode> = { system: Laptop, light: Sun, dark: Moon };

/** What the commands act on besides pages and appearance. */
export interface CommandContext {
  navigate: (path: string) => void;
  /** Adopted nodes; each gets a command that opens it, found by name, MAC, address or model. */
  devices?: readonly NodeSummary[];
  /** Known clients; each gets a command that opens it, found by name, MAC, address or vendor. */
  clients?: readonly Client[];
  signOut?: () => void;
  /** Admins also get the commands that change the network. */
  admin?: boolean;
  /** Opens the help panel, at the page's section or at `topic`. */
  openHelp?: (topic?: HelpTopic) => void;
  /** The guide's chapters; each chapter and section is found by its words and opens in the panel. */
  guide?: readonly HelpChapter[];
}

/**
 * The app's commands: every page, adoption, the devices and clients, then the skins, themes,
 * account and help, and the guide's sections once searched for.
 */
export function appCommands(context: CommandContext): Command[] {
  const p = strings.palette;
  const { navigate } = context;
  const roles = strings.devices.role;
  return [
    ...navItems.map((item) => ({
      id: `go:${item.path}`,
      group: p.groups.navigate,
      label: item.label,
      icon: item.icon,
      keywords: item.description,
      run: () => navigate(item.path),
    })),
    ...(context.admin
      ? [
          {
            id: "devices:adopt",
            group: p.groups.devices,
            label: p.adopt,
            icon: Plus,
            keywords: "add new router access point openwrt ssh discovered",
            run: () => navigate("/devices?adopt=1"),
          },
        ]
      : []),
    ...(context.admin && context.devices && !context.devices.some((n) => n.role === "gateway")
      ? [
          {
            id: "setup:router",
            group: p.groups.devices,
            label: strings.setup.command,
            icon: Router,
            keywords: "first run wizard gateway factory openwrt internet wifi bootstrap",
            run: () => navigate("/setup"),
          },
        ]
      : []),
    ...(context.admin
      ? [
          {
            id: "changes:review",
            group: p.groups.devices,
            label: strings.changes.command,
            icon: Upload,
            keywords: "pending apply review diff revision rollback discard",
            run: () => navigate("/changes"),
          },
        ]
      : []),
    ...(context.devices ?? []).map((node) => ({
      id: `device:${node.id}`,
      group: p.groups.devices,
      label: node.name,
      icon: node.role === "gateway" ? Router : RadioTower,
      keywords: [roles[node.role], node.mac, node.host, node.model ?? ""].join(" "),
      run: () => navigate(`/devices/${node.id}`),
    })),
    ...(context.clients ?? []).map((client) => ({
      id: `client:${client.mac}`,
      group: p.groups.clients,
      label: clientName(client),
      icon: kindIcons[client.kind],
      keywords: [client.hostname, client.mac, client.vendor, ...client.ips]
        .filter(Boolean)
        .join(" "),
      run: () => navigate(`/clients/${client.mac}`),
    })),
    ...skins.map((skin: Skin) => ({
      id: `skin:${skin}`,
      group: p.groups.appearance,
      label: p.skin(strings.skin[skin].label),
      icon: Palette,
      keywords: `${strings.skin.label} ${strings.skin[skin].description}`,
      run: () => setCurrentSkin(skin),
    })),
    ...(["system", "light", "dark"] as const).map((theme) => ({
      id: `theme:${theme}`,
      group: p.groups.appearance,
      label: p.theme(strings.theme[theme]),
      icon: themeIcons[theme],
      keywords: `${strings.theme.label} mode`,
      run: () => setThemePreference(theme),
    })),
    ...(context.signOut
      ? [
          {
            id: "account:sign-out",
            group: p.groups.account,
            label: strings.account.signOut,
            icon: LogOut,
            keywords: "log out logout",
            run: context.signOut,
          },
        ]
      : []),
    ...(context.openHelp ? helpCommands(context, context.openHelp) : []),
  ];
}

function helpCommands(context: CommandContext, open: (topic?: HelpTopic) => void): Command[] {
  const h = strings.help;
  const group = strings.palette.groups.help;
  return [
    {
      id: "help:page",
      group,
      label: h.commands.open,
      icon: CircleHelp,
      keywords: h.commands.keywords,
      run: () => open(),
    },
    {
      id: "help:center",
      group,
      label: h.commands.center,
      icon: BookOpen,
      keywords: h.commands.keywords,
      run: () => context.navigate(HELP_PATH),
    },
    ...(context.guide ?? []).flatMap((chapter) => [
      ...(chapter.slug === HELP_INDEX
        ? []
        : [
            {
              id: `help:${chapter.slug}`,
              group,
              label: chapter.title,
              hint: h.center,
              icon: BookOpen,
              keywords: "help guide chapter",
              searchOnly: true,
              run: () => open({ chapter: chapter.slug }),
            },
          ]),
      ...chapter.sections
        .filter((section) => section.depth <= 3)
        .map((section) => ({
          id: `help:${chapter.slug}#${section.anchor}`,
          group,
          label: section.text,
          hint: chapter.slug === HELP_INDEX ? h.contents : chapter.title,
          icon: BookOpen,
          keywords: `help guide ${chapter.title}`,
          searchOnly: true,
          run: () => open({ chapter: chapter.slug, anchor: section.anchor }),
        })),
    ]),
  ];
}

/**
 * The commands that match every word of the query, in any of label, group or keywords. Labels that
 * start with the query come first; groups stay together, led by the group of the best match.
 */
export function filterCommands(commands: readonly Command[], query: string): Command[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return commands.filter((c) => !c.searchOnly);
  const q = query.trim().toLowerCase();
  const ranked = commands
    .filter((c) => {
      const text = `${c.label} ${c.group} ${c.keywords ?? ""}`.toLowerCase();
      return words.every((word) => text.includes(word));
    })
    .map((c, i) => ({ c, i, rank: c.label.toLowerCase().startsWith(q) ? 0 : 1 }))
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map(({ c }) => c);
  const groups = new Map<string, Command[]>();
  for (const c of ranked) {
    groups.set(c.group, [...(groups.get(c.group) ?? []), c]);
  }
  return [...groups.values()].flat();
}

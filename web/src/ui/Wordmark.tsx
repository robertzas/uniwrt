import { strings } from "../lib/strings";

/** The uniwrt mark (a three-node mesh) and name; `compact` shows the mark alone. */
export function Wordmark(props: { compact?: boolean }) {
  return (
    <span class="inline-flex items-center gap-2.5">
      <svg viewBox="0 0 24 24" class="size-7 shrink-0" aria-hidden="true">
        <rect width="24" height="24" rx="6" class="fill-accent-fill" />
        <path
          d="M12 6.75 6.75 16.25h10.5Z"
          fill="none"
          stroke-width="1.5"
          stroke-linejoin="round"
          class="stroke-on-accent opacity-60"
        />
        <circle cx="12" cy="6.75" r="2.25" class="fill-on-accent" />
        <circle cx="6.75" cy="16.25" r="2.25" class="fill-on-accent" />
        <circle cx="17.25" cy="16.25" r="2.25" class="fill-on-accent" />
      </svg>
      <span class={["font-semibold text-[15px] tracking-tight", { "sr-only": props.compact }]}>
        {strings.appName}
      </span>
    </span>
  );
}

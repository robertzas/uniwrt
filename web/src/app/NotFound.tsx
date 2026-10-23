import { A } from "@solidjs/router";
import { SearchX } from "lucide-solid";
import { createEffect } from "solid-js";
import { strings } from "../lib/strings";
import { EmptyState } from "../ui/EmptyState";

export function NotFound() {
  const text = strings.page.notFound;
  createEffect(() => {
    document.title = `${text.title} · ${strings.appName}`;
  });
  return (
    <EmptyState icon={SearchX} title={text.title} description={text.description}>
      <A
        href="/"
        class="mt-6 inline-flex h-9 items-center rounded-lg bg-accent-fill px-4 font-medium text-on-accent text-sm hover:opacity-90"
      >
        {text.home}
      </A>
    </EmptyState>
  );
}

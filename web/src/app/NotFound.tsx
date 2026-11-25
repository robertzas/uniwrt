import { SearchX } from "lucide";
import { onSettled } from "solid-js";
import { strings } from "../lib/strings";
import { EmptyState } from "../ui/EmptyState";

export function NotFound() {
  const text = strings.page.notFound;
  onSettled(() => {
    document.title = `${text.title} · ${strings.appName}`;
  });
  return (
    <EmptyState
      icon={SearchX}
      title={text.title}
      description={text.description}
      actions={[{ label: text.home, href: "/" }]}
    />
  );
}
